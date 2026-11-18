mod storage;

pub use storage::{
    MAX_PAGE_SIZE, RecordPage, RecordStore, RecordStoreContract, RecordStoreRpcClient,
    RecordStoreRpcDispatcher, StoreError, StoredRecord,
};
