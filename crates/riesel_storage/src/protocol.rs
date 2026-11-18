use crate::storage::{StoreError, StoredRecord};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageMessage {
    pub version: u32,
    pub request: StorageRequest,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum StorageRequest {
    Create {
        model: String,
        data: Value,
    },
    Get {
        model: String,
        id: Uuid,
    },
    List {
        model: String,
    },
    Update {
        model: String,
        id: Uuid,
        expected_revision: u64,
        data: Value,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub enum StorageResult {
    Record(StoredRecord),
    OptionalRecord(Option<StoredRecord>),
    Records(Vec<StoredRecord>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum StorageResponse {
    Success(StorageResult),
    Failure(StoreError),
}
