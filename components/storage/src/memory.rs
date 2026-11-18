use riesel_storage::{RecordStore, StoreError, StoredRecord};

use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard};
use uuid::Uuid;

type Records = BTreeMap<(String, Uuid), StoredRecord>;

#[derive(Default)]
pub struct MemoryStore {
    records: Mutex<Records>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn lock_records(&self) -> Result<MutexGuard<'_, Records>, StoreError> {
        self.records
            .lock()
            .map_err(|_| StoreError::Failed("storage lock poisoned".into()))
    }
}

impl RecordStore for MemoryStore {
    fn create(&self, model: &str, data: Value) -> Result<StoredRecord, StoreError> {
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

    fn list(&self, model: &str) -> Result<Vec<StoredRecord>, StoreError> {
        let records = self.lock_records()?;

        Ok(records
            .iter()
            .filter(|((name, _), _)| name == model)
            .map(|(_, record)| record.clone())
            .collect())
    }

    fn update(
        &self,
        model: &str,
        id: Uuid,
        expected_revision: u64,
        data: Value,
    ) -> Result<StoredRecord, StoreError> {
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

        assert_eq!(store.list("task").unwrap().len(), 1);
        assert!(store.list("note").unwrap().is_empty());

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
}
