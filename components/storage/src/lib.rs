mod memory;
mod server;

pub use memory::MemoryStore;
pub use server::run_storage_server;

use riesel_schema::{ContractError, ProviderDefinition};
use riesel_storage::RecordStoreContract;

/// Metadata for the built-in memory provider. Its capability reference
/// comes directly from the generated Rust trait contract.
pub fn memory_provider_definition() -> Result<ProviderDefinition, ContractError> {
    Ok(ProviderDefinition {
        id: "riesel.storage.memory".into(),
        capabilities: vec![riesel_storage::RecordStoreContract::definition()?.capability],
    })
}

use riesel_app::{LocalProviderFactory, local_provider};
use riesel_storage::RecordStore;
use std::sync::Arc;

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
