use riesel_schema::ModelDefinition;
use riesel_storage::{
    MAX_PAGE_SIZE, RecordPage, RecordQuery, RecordQueryCursors, RecordQueryError, RecordQueryPage,
    RecordQueryRequest, RecordStore, StoreError, StoredRecord, record_matches_predicate,
    validate_record_query, validate_record_query_envelope,
};

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound;
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

type Records = BTreeMap<(String, Uuid), StoredRecord>;
const MAX_RECORD_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub struct MemoryStore {
    records: Mutex<Records>,
    models: Option<BTreeMap<String, ModelDefinition>>,
    readable_models: BTreeSet<String>,
    query_cursors: Mutex<RecordQueryCursors>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bind this instance to authoritative application model metadata.
    /// Standalone reference hosts without a schema remain permissive and
    /// must not be treated as production schema-enforcing providers.
    pub fn with_models(models: BTreeMap<String, ModelDefinition>) -> Self {
        Self {
            models: Some(models),
            ..Self::default()
        }
    }

    /// Grant the specified registered models to the first trusted linked
    /// record Query facet. Model metadata alone does not grant Query access.
    /// The read-only RPC facet is exposed separately by the provider factory.
    pub fn with_query_models(
        models: BTreeMap<String, ModelDefinition>,
        readable_models: BTreeSet<String>,
    ) -> Self {
        Self {
            models: Some(models),
            readable_models,
            ..Self::default()
        }
    }

    fn validate_data(&self, model: &str, data: &Value) -> Result<(), StoreError> {
        if !data.is_object() {
            return Err(StoreError::Failed(
                "record data must be a JSON object".into(),
            ));
        }
        let size = serde_json::to_vec(data)
            .map_err(|error| StoreError::Failed(error.to_string()))?
            .len();
        if size > MAX_RECORD_BYTES {
            return Err(StoreError::Failed("record exceeds 64 KiB".into()));
        }
        if let Some(models) = &self.models {
            let definition = models
                .get(model)
                .ok_or_else(|| StoreError::Failed(format!("unknown model: {model}")))?;
            definition
                .validate_record(data)
                .map_err(|error| StoreError::Failed(error.to_string()))?;
        }
        Ok(())
    }

    fn lock_records(&self) -> Result<MutexGuard<'_, Records>, StoreError> {
        self.records
            .lock()
            .map_err(|_| StoreError::Failed("storage lock poisoned".into()))
    }
}

impl RecordStore for MemoryStore {
    fn create(&self, model: &str, data: Value) -> Result<StoredRecord, StoreError> {
        self.validate_data(model, &data)?;
        let mut records = self.lock_records()?;

        loop {
            let id = Uuid::new_v4();
            let key = (model.to_owned(), id);

            if records.contains_key(&key) {
                continue;
            }

            let record = StoredRecord {
                id,
                data,
                revision: 1,
            };

            records.insert(key, record.clone());

            return Ok(record);
        }
    }

    fn get(&self, model: &str, id: Uuid) -> Result<Option<StoredRecord>, StoreError> {
        let records = self.lock_records()?;

        Ok(records.get(&(model.to_owned(), id)).cloned())
    }

    fn list_page(
        &self,
        model: &str,
        after: Option<Uuid>,
        limit: u64,
    ) -> Result<RecordPage, StoreError> {
        if !(1..=MAX_PAGE_SIZE).contains(&limit) {
            return Err(StoreError::InvalidPageSize);
        }
        let records = self.lock_records()?;
        let lower = match after {
            Some(cursor) => Bound::Excluded((model.to_owned(), cursor)),
            None => Bound::Included((model.to_owned(), Uuid::nil())),
        };
        let mut page: Vec<StoredRecord> = Vec::new();
        for ((name, _), record) in records.range((lower, Bound::Unbounded)) {
            if name != model {
                break;
            }
            if page.len() == limit as usize {
                let next_cursor = page.last().map(|record| record.id);
                return Ok(RecordPage {
                    records: page,
                    next_cursor,
                });
            }
            page.push(record.clone());
        }
        Ok(RecordPage {
            records: page,
            next_cursor: None,
        })
    }

    fn update(
        &self,
        model: &str,
        id: Uuid,
        expected_revision: u64,
        data: Value,
    ) -> Result<StoredRecord, StoreError> {
        self.validate_data(model, &data)?;
        let mut records = self.lock_records()?;

        let record = records
            .get_mut(&(model.to_owned(), id))
            .ok_or(StoreError::NotFound)?;

        if record.revision != expected_revision {
            return Err(StoreError::RevisionConflict);
        }

        let next_revision = record
            .revision
            .checked_add(1)
            .ok_or_else(|| StoreError::Failed("revision overflow".into()))?;

        record.data = data;
        record.revision = next_revision;

        Ok(record.clone())
    }

    fn delete(&self, model: &str, id: Uuid, expected_revision: u64) -> Result<(), StoreError> {
        let mut records = self.lock_records()?;
        let key = (model.to_owned(), id);
        let current = records.get(&key).ok_or(StoreError::NotFound)?;
        if current.revision != expected_revision {
            return Err(StoreError::RevisionConflict);
        }
        records.remove(&key);
        Ok(())
    }
}

/// Provider-owned bounded iteration; never invokes the writable
/// RecordStore RPC or performs a consumer-side full scan.
impl RecordQuery for MemoryStore {
    fn query_page(&self, request: RecordQueryRequest) -> Result<RecordQueryPage, RecordQueryError> {
        validate_record_query_envelope(&request)?;
        let definition = self
            .models
            .as_ref()
            .and_then(|models| models.get(&request.model))
            .filter(|_| self.readable_models.contains(&request.model))
            .ok_or(RecordQueryError::ModelDenied)?;
        validate_record_query(definition, &request)?;

        let after = self
            .query_cursors
            .lock()
            .map_err(|_| RecordQueryError::Failed("cursor lock poisoned".into()))?
            .resume(&request)?;
        let lower = match after {
            Some(cursor) => Bound::Excluded((request.model.clone(), cursor)),
            None => Bound::Included((request.model.clone(), Uuid::nil())),
        };

        let mut records = Vec::new();
        let mut inspected = 0;
        let mut last = None;
        let mut exhausted = true;
        {
            let guard = self
                .records
                .lock()
                .map_err(|_| RecordQueryError::Failed("records lock poisoned".into()))?;
            for ((model, id), record) in guard.range((lower, Bound::Unbounded)) {
                if model != &request.model {
                    break;
                }
                inspected += 1;
                last = Some(*id);
                if record_matches_predicate(&record.data, definition, request.predicate.as_ref())? {
                    records.push(record.clone());
                }
                if inspected == request.scan_budget || records.len() == request.limit as usize {
                    // No unbudgeted lookahead. Another page may be empty,
                    // but an empty *partial* page is never globally empty.
                    exhausted = false;
                    break;
                }
            }
        }

        let next_cursor = if exhausted {
            None
        } else {
            Some(
                self.query_cursors
                    .lock()
                    .map_err(|_| RecordQueryError::Failed("cursor lock poisoned".into()))?
                    .issue(&request, last.expect("partial scan must inspect a record"))?,
            )
        };
        Ok(RecordQueryPage {
            records,
            next_cursor,
            exhausted,
            inspected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn creates_and_retrieves_records() {
        let store = MemoryStore::new();

        let data = json!({
            "title": "Build Riesel",
            "completed": false
        });

        let created = store.create("task", data.clone()).unwrap();

        assert_eq!(created.data, data);
        assert_eq!(created.revision, 1);

        let retrieved = store.get("task", created.id).unwrap().unwrap();

        assert_eq!(retrieved.id, created.id);
        assert_eq!(retrieved.data, data);
    }

    #[test]
    fn separates_models() {
        let store = MemoryStore::new();

        let task = store.create("task", json!({"title": "Test"})).unwrap();

        assert_eq!(
            store
                .list_page("task", None, MAX_PAGE_SIZE)
                .unwrap()
                .records
                .len(),
            1
        );
        assert!(
            store
                .list_page("note", None, MAX_PAGE_SIZE)
                .unwrap()
                .records
                .is_empty()
        );

        assert!(store.get("note", task.id).unwrap().is_none());
    }

    #[test]
    fn updates_records_with_revision_check() {
        let store = MemoryStore::new();

        let original = store
            .create(
                "task",
                json!({
                    "title": "Build Riesel",
                    "completed": false
                }),
            )
            .unwrap();

        let updated_data = json!({
            "title": "Build Riesel",
            "completed": true
        });

        let updated = store
            .update("task", original.id, original.revision, updated_data.clone())
            .unwrap();

        assert_eq!(updated.revision, 2);
        assert_eq!(updated.data, updated_data);

        let stale_update = store.update(
            "task",
            original.id,
            original.revision,
            json!({"title": "Outdated"}),
        );

        assert!(matches!(stale_update, Err(StoreError::RevisionConflict)));

        let current = store.get("task", original.id).unwrap().unwrap();

        assert_eq!(current.data, updated_data);
        assert_eq!(current.revision, 2);
    }

    #[test]
    fn updating_missing_record_returns_error() {
        let store = MemoryStore::new();

        let result = store.update("task", Uuid::new_v4(), 1, json!({"title": "Missing"}));

        assert!(matches!(result, Err(StoreError::NotFound)));
    }
    #[test]
    fn cursor_pagination_is_bounded_stable_and_model_scoped() {
        let store = MemoryStore::new();
        let mut ids = Vec::new();
        for index in 0..5 {
            let record = store.create("task", json!({"index": index})).unwrap();
            ids.push(record.id);
        }
        store.create("note", json!({"index": 99})).unwrap();
        ids.sort();
        let mut after = None;
        let mut visited = Vec::new();
        loop {
            let page = store.list_page("task", after, 2).unwrap();
            assert!(page.records.len() <= 2);
            visited.extend(page.records.iter().map(|record| record.id));
            if let Some(cursor) = page.next_cursor {
                after = Some(cursor);
            } else {
                break;
            }
        }
        assert_eq!(visited, ids);
        assert!(matches!(
            store.list_page("task", None, 0),
            Err(StoreError::InvalidPageSize)
        ));
        assert!(matches!(
            store.list_page("task", None, MAX_PAGE_SIZE + 1),
            Err(StoreError::InvalidPageSize)
        ));
    }

    #[test]
    fn deleting_records_requires_the_latest_revision() {
        let store = MemoryStore::new();
        let created = store.create("task", json!({"title": "Test"})).unwrap();
        let updated = store
            .update(
                "task",
                created.id,
                created.revision,
                json!({"title": "Updated"}),
            )
            .unwrap();
        assert!(matches!(
            store.delete("task", created.id, created.revision),
            Err(StoreError::RevisionConflict)
        ));
        store.delete("task", created.id, updated.revision).unwrap();
        assert!(store.get("task", created.id).unwrap().is_none());
        assert!(matches!(
            store.delete("task", created.id, updated.revision),
            Err(StoreError::NotFound)
        ));
    }

    #[test]
    fn model_aware_storage_rejects_invalid_and_unknown_records() {
        use riesel_schema::{ModelDefinition, TypeRef};
        let schema = ModelDefinition {
            fields: [
                ("title".into(), TypeRef::String),
                ("completed".into(), TypeRef::Bool),
            ]
            .into_iter()
            .collect(),
        };
        let store = MemoryStore::with_models([("task".into(), schema)].into_iter().collect());
        store
            .create("task", json!({"title": "Valid", "completed": false}))
            .unwrap();
        assert!(matches!(
            store.create("task", json!({"title": "Missing boolean"})),
            Err(StoreError::Failed(_))
        ));
        assert!(matches!(
            store.create("missing", json!({"title": "Unknown"})),
            Err(StoreError::Failed(_))
        ));
        assert!(matches!(
            store.create(
                "task",
                json!({"title": "a".repeat(70_000), "completed": false})
            ),
            Err(StoreError::Failed(_))
        ));
    }
    #[test]
    fn native_query_distinguishes_absent_null_and_present_optional_values() {
        use riesel_schema::TypeRef;
        use riesel_storage::{RecordCompare, RecordPredicate, RecordQueryRequest};

        let schema = ModelDefinition {
            fields: BTreeMap::from([
                ("flag".into(), TypeRef::Bool),
                (
                    "note".into(),
                    TypeRef::Optional {
                        item: Box::new(TypeRef::String),
                    },
                ),
            ]),
        };
        let store = MemoryStore::with_query_models(
            BTreeMap::from([("asset".into(), schema)]),
            BTreeSet::from(["asset".into()]),
        );
        let missing = store.create("asset", json!({"flag":true})).unwrap();
        let null = store
            .create("asset", json!({"flag":true,"note":null}))
            .unwrap();
        let present = store
            .create("asset", json!({"flag":true,"note":"ready"}))
            .unwrap();

        let fetch = |operator| {
            store
                .query_page(RecordQueryRequest {
                    model: "asset".into(),
                    predicate: Some(RecordPredicate {
                        field: "note".into(),
                        operator,
                        value: None,
                    }),
                    limit: 8,
                    scan_budget: 8,
                    cursor: None,
                })
                .unwrap()
        };
        let absent = fetch(RecordCompare::IsMissing);
        assert!(absent.exhausted);
        assert_eq!(absent.records.len(), 1);
        assert_eq!(absent.records[0].id, missing.id);

        let explicitly_null = fetch(RecordCompare::IsNull);
        assert_eq!(explicitly_null.records.len(), 1);
        assert_eq!(explicitly_null.records[0].id, null.id);

        let all_present = fetch(RecordCompare::IsPresent);
        assert_eq!(all_present.records.len(), 2);
        assert!(all_present.records.iter().any(|r| r.id == null.id));
        assert!(all_present.records.iter().any(|r| r.id == present.id));
    }

    #[test]
    fn native_bounded_sparse_query_returns_empty_partial_pages_then_match() {
        use riesel_schema::TypeRef;
        use riesel_storage::{RecordCompare, RecordPredicate, RecordQueryRequest};
        let definition = ModelDefinition {
            fields: BTreeMap::from([
                ("completed".into(), TypeRef::Bool),
                ("big".into(), TypeRef::U64),
            ]),
        };
        let store = MemoryStore::with_query_models(
            BTreeMap::from([("task".into(), definition)]),
            BTreeSet::from(["task".into()]),
        );
        let mut seeded = Vec::new();
        for _ in 0..65 {
            seeded.push(
                store
                    .create("task", json!({"completed":true,"big":u64::MAX}))
                    .unwrap(),
            );
        }
        let last = seeded.iter().max_by_key(|record| record.id).unwrap();
        store
            .update(
                "task",
                last.id,
                last.revision,
                json!({"completed":false,"big":u64::MAX}),
            )
            .unwrap();
        let mut request = RecordQueryRequest {
            model: "task".into(),
            predicate: Some(RecordPredicate {
                field: "completed".into(),
                operator: RecordCompare::Eq,
                value: Some(json!(false)),
            }),
            limit: 32,
            scan_budget: 32,
            cursor: None,
        };
        for _ in 0..2 {
            let page = store.query_page(request.clone()).unwrap();
            assert!(page.records.is_empty());
            assert_eq!(page.inspected, 32);
            assert!(!page.exhausted);
            request.cursor = page.next_cursor;
            assert!(request.cursor.is_some());
        }
        let final_page = store.query_page(request.clone()).unwrap();
        assert_eq!(final_page.records.len(), 1);
        assert_eq!(final_page.records[0].id, last.id);
        assert_eq!(final_page.inspected, 1);
        assert!(final_page.exhausted);
        assert!(final_page.next_cursor.is_none());

        let mut changed = request;
        changed.cursor = None;
        changed.predicate.as_mut().unwrap().value = Some(json!(true));
        let page = store.query_page(changed).unwrap();
        assert!(!page.records.is_empty());
        assert_eq!(page.inspected, 32);
    }

    #[test]
    fn live_cursor_sees_later_confirmed_mutation_without_claiming_snapshot() {
        use riesel_schema::TypeRef;
        use riesel_storage::{RecordCompare, RecordPredicate, RecordQueryRequest};
        let store = MemoryStore::with_query_models(
            BTreeMap::from([(
                "task".into(),
                ModelDefinition {
                    fields: BTreeMap::from([("completed".into(), TypeRef::Bool)]),
                },
            )]),
            BTreeSet::from(["task".into()]),
        );
        let mut records = Vec::new();
        for _ in 0..3 {
            records.push(store.create("task", json!({"completed":false})).unwrap());
        }
        records.sort_by_key(|record| record.id);
        let mut request = RecordQueryRequest {
            model: "task".into(),
            predicate: Some(RecordPredicate {
                field: "completed".into(),
                operator: RecordCompare::Eq,
                value: Some(json!(true)),
            }),
            limit: 1,
            scan_budget: 1,
            cursor: None,
        };
        let first = store.query_page(request.clone()).unwrap();
        assert!(first.records.is_empty());
        assert!(!first.exhausted);
        request.cursor = first.next_cursor;

        // The mutation happens after page 1. The provider must inspect the
        // current live state on later pages, not invent a stable snapshot.
        let last = records.last().unwrap();
        store
            .update("task", last.id, last.revision, json!({"completed":true}))
            .unwrap();
        let second = store.query_page(request.clone()).unwrap();
        assert!(second.records.is_empty());
        assert!(!second.exhausted);
        request.cursor = second.next_cursor;
        let last_page = store.query_page(request).unwrap();
        assert_eq!(last_page.records.len(), 1);
        assert_eq!(last_page.records[0].id, last.id);
        assert_eq!(last_page.records[0].data["completed"], true);
    }

    #[test]
    fn native_query_rejects_undeclared_read_models_and_wrong_cursor_query() {
        use riesel_schema::TypeRef;
        use riesel_storage::{
            RecordCompare, RecordPredicate, RecordQueryError, RecordQueryRequest,
        };
        let definition = ModelDefinition {
            fields: BTreeMap::from([("big".into(), TypeRef::U64)]),
        };
        let models = BTreeMap::from([
            ("task".into(), definition.clone()),
            ("asset".into(), definition),
        ]);
        let store = MemoryStore::with_query_models(models.clone(), BTreeSet::from(["task".into()]));
        let no_read_grant = MemoryStore::with_models(models);
        let request = RecordQueryRequest {
            model: "task".into(),
            predicate: Some(RecordPredicate {
                field: "big".into(),
                operator: RecordCompare::Eq,
                value: Some(json!(u64::MAX)),
            }),
            limit: 1,
            scan_budget: 1,
            cursor: None,
        };
        assert!(matches!(
            no_read_grant.query_page(request.clone()),
            Err(RecordQueryError::ModelDenied)
        ));
        assert!(matches!(
            store.query_page(RecordQueryRequest {
                model: "asset".into(),
                ..request.clone()
            }),
            Err(RecordQueryError::ModelDenied)
        ));
        store.create("task", json!({"big":u64::MAX})).unwrap();
        store.create("task", json!({"big":u64::MAX})).unwrap();
        let page = store.query_page(request.clone()).unwrap();
        assert_eq!(page.records.len(), 1);
        let mut wrong = request.clone();
        wrong.cursor = page.next_cursor.clone();
        wrong.predicate.as_mut().unwrap().value = Some(json!(u64::MAX - 1));
        assert!(matches!(
            store.query_page(wrong),
            Err(RecordQueryError::InvalidCursor)
        ));
        let mut correct = request;
        correct.cursor = page.next_cursor;
        assert_eq!(store.query_page(correct).unwrap().records.len(), 1);
    }
}
