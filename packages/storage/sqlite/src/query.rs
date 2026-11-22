//! Bounded, native SQLite provider-owned record Query execution.
//! Candidate selection uses the existing (model,id) primary-key order.
//! Field predicates run as exact typed Rust comparisons under the same owner
//! lock, avoiding SQLite JSON number precision and UUID spelling coercions.
use crate::database::SqliteStore;
use crate::records::decode_id;
use riesel_storage::{
    RecordQuery, RecordQueryError, RecordQueryPage, RecordQueryRequest, record_matches_predicate,
    validate_record_query,
};
use rusqlite::params;
// A nullable-cursor OR expression can prevent SQLite from seeking directly
// to a later (model,id) key, silently scanning an unbounded prefix. Use two
// explicit primary-key range queries instead, both with a native LIMIT.
const QUERY_SCAN_FIRST: &str =
    "SELECT id,revision,data FROM records WHERE model=?1 ORDER BY id LIMIT ?2";
const QUERY_SCAN_AFTER: &str =
    "SELECT id,revision,data FROM records WHERE model=?1 AND id>?2 ORDER BY id LIMIT ?3";

fn failed(source: impl std::fmt::Display) -> RecordQueryError {
    RecordQueryError::Failed(source.to_string())
}

impl RecordQuery for SqliteStore {
    fn query_page(&self, request: RecordQueryRequest) -> Result<RecordQueryPage, RecordQueryError> {
        let definition = self
            .models
            .get(&request.model)
            .filter(|_| self.readable_models.contains(&request.model))
            .ok_or(RecordQueryError::ModelDenied)?;
        validate_record_query(definition, &request)?;
        let after = self
            .query_cursors
            .lock()
            .map_err(|_| RecordQueryError::Failed("cursor lock poisoned".into()))?
            .resume(&request)?;

        let mut matches = Vec::new();
        let mut last = None;
        let mut inspected = 0;
        let mut exhausted = true;
        {
            let connection = self.lock().map_err(failed)?;
            // A budget bounds the number of actual record candidates read,
            // not merely the number returned after applying a predicate.
            // Never create a secondary index or silently scan the entire DB.
            let cursor = after.map(|id| id.as_bytes().to_vec());
            let sql = if cursor.is_some() {
                QUERY_SCAN_AFTER
            } else {
                QUERY_SCAN_FIRST
            };
            let mut stmt = connection.prepare(sql).map_err(failed)?;
            let mut rows = match cursor.as_deref() {
                Some(after) => {
                    stmt.query(params![&request.model, after, request.scan_budget as i64])
                }
                None => stmt.query(params![&request.model, request.scan_budget as i64]),
            }
            .map_err(failed)?;
            while let Some(row) = rows.next().map_err(failed)? {
                let id: Vec<u8> = row.get(0).map_err(failed)?;
                let revision: Vec<u8> = row.get(1).map_err(failed)?;
                let data: Vec<u8> = row.get(2).map_err(failed)?;
                let id = decode_id(&id).map_err(failed)?;
                let record = self
                    .decode(&request.model, id, revision, data)
                    .map_err(failed)?;
                inspected += 1;
                last = Some(id);
                if record_matches_predicate(&record.data, definition, request.predicate.as_ref())? {
                    matches.push(record);
                }
                if inspected == request.scan_budget || matches.len() == request.limit as usize {
                    // Result-limited and effort-limited are both potentially
                    // partial. No additional work/peek beyond the budget.
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
                    .issue(
                        &request,
                        last.expect("partial scan inspected one candidate"),
                    )?,
            )
        };
        Ok(RecordQueryPage {
            records: matches,
            next_cursor,
            exhausted,
            inspected,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_schema::{ModelDefinition, TypeRef};
    use riesel_storage::{RecordCompare, RecordPredicate, RecordStore};
    use serde_json::json;

    use std::collections::{BTreeMap, BTreeSet};
    use uuid::Uuid;

    fn models() -> BTreeMap<String, ModelDefinition> {
        BTreeMap::from([(
            "task".into(),
            ModelDefinition {
                fields: BTreeMap::from([
                    ("completed".into(), TypeRef::Bool),
                    ("big".into(), TypeRef::U64),
                    (
                        "note".into(),
                        TypeRef::Optional {
                            item: Box::new(TypeRef::String),
                        },
                    ),
                    ("external_id".into(), TypeRef::Uuid),
                ]),
            },
        )])
    }

    #[test]
    fn cursor_sql_plan_seeks_existing_primary_key_instead_of_scanning_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("query-plan.db");
        let store = SqliteStore::open(&db, models()).unwrap();
        let connection = store.lock().unwrap();

        let first_plan = format!("EXPLAIN QUERY PLAN {QUERY_SCAN_FIRST}");
        let details: Vec<String> = connection
            .prepare(&first_plan)
            .unwrap()
            .query_map(params!["task", 32_i64], |row| row.get(3))
            .unwrap()
            .map(|item| item.unwrap())
            .collect();
        assert!(
            details
                .iter()
                .any(|d| d.contains("PRIMARY KEY") && d.contains("model=?")),
            "initial bounded scan must use the existing model/id key: {details:?}"
        );

        let after_plan = format!("EXPLAIN QUERY PLAN {QUERY_SCAN_AFTER}");
        let cursor = Uuid::new_v4().as_bytes().to_vec();
        let details: Vec<String> = connection
            .prepare(&after_plan)
            .unwrap()
            .query_map(params!["task", cursor, 32_i64], |row| row.get(3))
            .unwrap()
            .map(|item| item.unwrap())
            .collect();
        assert!(
            details.iter().any(|d| {
                d.contains("PRIMARY KEY") && d.contains("model=?") && d.contains("id>?")
            }),
            "continuation must seek the keyed suffix rather than scan a growing prefix: {details:?}"
        );
    }

    #[test]
    fn native_sqlite_query_is_bounded_and_survives_confirmed_writes() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("records.db");
        let m = models();
        let store = SqliteStore::open(&db, m.clone())
            .unwrap()
            .with_query_grants(BTreeSet::from(["task".into()]))
            .unwrap();
        let uuid = Uuid::new_v4();
        let mut keys = Vec::new();
        for _ in 0..65 {
            keys.push(
                store
                    .create(
                        "task",
                        json!({"completed":true,"big":u64::MAX,
                               "external_id":uuid.to_string().to_uppercase()}),
                    )
                    .unwrap(),
            );
        }
        let last = keys.iter().max_by_key(|record| record.id).unwrap();
        store
            .update(
                "task",
                last.id,
                last.revision,
                json!({"completed":false,"big":u64::MAX,
                       "external_id":uuid.to_string().to_uppercase(),
                       "note":null}),
            )
            .unwrap();
        let mut q = RecordQueryRequest {
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
            let page = store.query_page(q.clone()).unwrap();
            assert!(page.records.is_empty());
            assert_eq!(page.inspected, 32);
            assert!(!page.exhausted);
            q.cursor = page.next_cursor;
        }
        let last_page = store.query_page(q).unwrap();
        assert_eq!(last_page.records.len(), 1);
        assert_eq!(last_page.records[0].id, last.id);
        assert!(last_page.exhausted);
        assert_eq!(last_page.inspected, 1);

        let equal = RecordQueryRequest {
            model: "task".into(),
            predicate: Some(RecordPredicate {
                field: "external_id".into(),
                operator: RecordCompare::Eq,
                value: Some(json!(uuid.to_string())),
            }),
            limit: 32,
            scan_budget: 32,
            cursor: None,
        };
        assert_eq!(store.query_page(equal).unwrap().records.len(), 32);
        let big = RecordQueryRequest {
            model: "task".into(),
            predicate: Some(RecordPredicate {
                field: "big".into(),
                operator: RecordCompare::Eq,
                value: Some(json!(u64::MAX - 1)),
            }),
            limit: 32,
            scan_budget: 32,
            cursor: None,
        };
        assert!(store.query_page(big).unwrap().records.is_empty());
        drop(store);

        // Query-only attachment to the existing file does not require a
        // writable Storage contract or create/upgrade the database.
        let read_only =
            SqliteStore::open_query_only(&db, m, BTreeSet::from(["task".into()])).unwrap();
        let q = RecordQueryRequest {
            model: "task".into(),
            predicate: Some(RecordPredicate {
                field: "note".into(),
                operator: RecordCompare::IsNull,
                value: None,
            }),
            limit: 32,
            scan_budget: 256,
            cursor: None,
        };
        let page = read_only.query_page(q).unwrap();
        assert_eq!(page.records.len(), 1);
    }

    #[test]
    fn sqlite_live_cursor_observes_confirmed_mutation_between_pages() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("live-scan.db");
        let store = SqliteStore::open(&db, models())
            .unwrap()
            .with_query_grants(BTreeSet::from(["task".into()]))
            .unwrap();
        let id = Uuid::new_v4();
        let mut records = Vec::new();
        for _ in 0..3 {
            records.push(
                store
                    .create(
                        "task",
                        json!({"completed":false, "big":u64::MAX,
                               "external_id":id.to_string()}),
                    )
                    .unwrap(),
            );
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
        let last = records.last().unwrap();
        store
            .update(
                "task",
                last.id,
                last.revision,
                json!({"completed":true, "big":u64::MAX,
                       "external_id":id.to_string()}),
            )
            .unwrap();
        let second = store.query_page(request.clone()).unwrap();
        assert!(second.records.is_empty());
        assert!(!second.exhausted);
        request.cursor = second.next_cursor;
        let final_page = store.query_page(request).unwrap();
        assert_eq!(final_page.records.len(), 1);
        assert_eq!(final_page.records[0].id, last.id);
        assert_eq!(final_page.records[0].data["completed"], true);
    }

    #[test]
    fn query_only_reads_its_model_without_enumerating_other_persisted_models() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi-model.db");
        let task_model = models().remove("task").unwrap();
        let all_models = BTreeMap::from([
            ("task".into(), task_model.clone()),
            (
                "secret".into(),
                ModelDefinition {
                    fields: BTreeMap::from([("token".into(), TypeRef::String)]),
                },
            ),
        ]);
        let writer = SqliteStore::open(&path, all_models).unwrap();
        writer
            .create(
                "task",
                json!({"completed":true,"big":u64::MAX,
                       "external_id":Uuid::new_v4().to_string()}),
            )
            .unwrap();
        writer
            .create("secret", json!({"token":"not-exposed"}))
            .unwrap();
        drop(writer);

        // The query client registers only its own model; it cannot enumerate
        // or read the other persisted schema/records.
        let reader = SqliteStore::open_query_only(
            &path,
            BTreeMap::from([("task".into(), task_model)]),
            BTreeSet::from(["task".into()]),
        )
        .unwrap();
        let read = RecordQueryRequest {
            model: "task".into(),
            predicate: None,
            limit: 16,
            scan_budget: 16,
            cursor: None,
        };
        assert_eq!(reader.query_page(read.clone()).unwrap().records.len(), 1);
        // The SQLite connection itself enforces read-only mode, in addition
        // to the absence of mutation methods in the published Query facet.
        let denied_write = reader.create(
            "task",
            json!({"completed":true,"big":u64::MAX,
                   "external_id":Uuid::new_v4().to_string()}),
        );
        assert!(denied_write.is_err());
        let mut denied = read;
        denied.model = "secret".into();
        assert!(matches!(
            reader.query_page(denied),
            Err(RecordQueryError::ModelDenied)
        ));
    }

    #[test]
    fn query_only_does_not_create_database_or_grant_unknown_model() {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("missing.db");
        assert!(
            SqliteStore::open_query_only(&db, models(), BTreeSet::from(["task".into()])).is_err()
        );
        assert!(!db.exists());
    }
}
