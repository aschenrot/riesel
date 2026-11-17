use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredRecord {
    pub id: Uuid,
    pub data: Value,
    pub revision: u64,
}

#[derive(Debug, Error, Serialize, Deserialize)]
pub enum StoreError {
    #[error("record not found")]
    NotFound,

    #[error("record was modified since it was read")]
    RevisionConflict,

    #[error("storage operation failed: {0}")]
    Failed(String),
}

pub trait RecordStore {
    fn create(&self, model: &str, data: Value) -> Result<StoredRecord, StoreError>;

    fn get(&self, model: &str, id: Uuid) -> Result<Option<StoredRecord>, StoreError>;

    fn list(&self, model: &str) -> Result<Vec<StoredRecord>, StoreError>;

    fn update(
        &self,
        model: &str,
        id: Uuid,
        expected_revision: u64,
        data: Value,
    ) -> Result<StoredRecord, StoreError>;
}
