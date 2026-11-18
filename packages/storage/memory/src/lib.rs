mod memory;
mod server;

pub use memory::MemoryStore;
pub use server::run_storage_server;

use riesel_schema::ProviderDefinition;
use riesel_storage::RecordStoreContract;

use riesel_app::{LocalProviderFactory, local_provider};
use riesel_storage::RecordStore;
use std::sync::Arc;

/// Metadata for the reference provider comes from its typed registration.
pub fn memory_provider_definition() -> Result<ProviderDefinition, riesel_app::ConnectorError> {
    memory_storage_provider().definition()
}

/// Register this implementation without manual provider/contract wiring.
pub fn memory_storage_provider() -> impl LocalProviderFactory {
    local_provider::<RecordStoreContract, _>("riesel.storage.memory", || {
        Arc::new(MemoryStore::new()) as Arc<dyn RecordStore + Send + Sync>
    })
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
