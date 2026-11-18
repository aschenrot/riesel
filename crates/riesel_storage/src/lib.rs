mod client;
mod contract;
mod protocol;
mod storage;

pub use client::TcpStore;
pub use contract::{RECORDS_CAPABILITY_ID, RECORDS_CAPABILITY_VERSION, records_contract};

pub use protocol::{
    PROTOCOL_VERSION, StorageMessage, StorageRequest, StorageResponse, StorageResult,
};

pub use storage::{RecordStore, StoreError, StoredRecord};
