mod memory;
mod server;

pub use memory::MemoryStore;
pub use server::run_storage_server;

use riesel_schema::ProviderDefinition;
use riesel_storage::RecordStoreContract;

use riesel_app::{ComponentDefinition, ConnectorError, LocalProviderFactory};
use riesel_storage::{RecordStore, RecordStoreRpcDispatcher};
use std::sync::Arc;

/// Metadata for the reference provider comes from its typed registration.
pub fn memory_provider_definition() -> Result<ProviderDefinition, ConnectorError> {
    memory_storage_provider()?.definition()
}

/// One authoritative provider registration serves both local and managed
/// instances; the Storage implementation is constructed once per instance.
pub fn memory_storage_provider() -> Result<ComponentDefinition<MemoryStore>, ConnectorError> {
    ComponentDefinition::new("riesel.storage.memory", MemoryStore::new)
        .exposes::<RecordStoreContract, _>(
            |state| state as Arc<dyn RecordStore + Send + Sync>,
            RecordStoreRpcDispatcher::new,
        )
}

#[cfg(test)]
mod provider_tests {
    use super::*;

    #[test]
    fn memory_provider_matches_generated_storage_contract() {
        let provider = memory_provider_definition().unwrap();
        provider.validate().unwrap();
        let contract = riesel_storage::RecordStoreContract::definition().unwrap();
        assert_eq!(provider.capabilities, vec![contract.capability]);
    }
}
