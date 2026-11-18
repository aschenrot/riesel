use riesel_schema::ModelDefinition;
use riesel_storage::{MAX_PAGE_SIZE, RecordPage, RecordStore, StoreError, StoredRecord};

use serde_json::Value;
use std::collections::BTreeMap;
use std::ops::Bound;
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

type Records = BTreeMap<(String, Uuid), StoredRecord>;
const MAX_RECORD_BYTES: usize = 64 * 1024;

#[derive(Default)]
pub struct MemoryStore {
    records: Mutex<Records>,
    models: Option<BTreeMap<String, ModelDefinition>>,
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
            records: Mutex::new(BTreeMap::new()),
            models: Some(models),
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
}
