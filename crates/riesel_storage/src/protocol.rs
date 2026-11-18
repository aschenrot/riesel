use crate::storage::{StoreError, StoredRecord};
use riesel_schema::{CapabilityContract, ProviderDefinition};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u32 = 1;

/// Provider identity and the full Rust-derived contract exposed by this service.
/// This is a compatibility advertisement, not an authentication mechanism.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageServiceInfo {
    pub provider: ProviderDefinition,
    pub contract: CapabilityContract,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StorageMessage {
    pub version: u32,
    pub request: StorageRequest,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum StorageRequest {
    /// Inspect the service before constructing a remote capability binding.
    Describe,
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
    ServiceInfo(StorageServiceInfo),
    Record(StoredRecord),
    OptionalRecord(Option<StoredRecord>),
    Records(Vec<StoredRecord>),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum StorageResponse {
    Success(StorageResult),
    Failure(StoreError),
}
