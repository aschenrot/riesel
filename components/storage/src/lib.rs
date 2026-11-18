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

use riesel_app::{ConnectedProvider, ConnectorError, LocalProviderFactory};
use riesel_storage::RecordStore;
use std::sync::Arc;

/// Explicit, lightweight in-process MemoryStore provider for local development.
pub struct MemoryStoreFactory;

impl LocalProviderFactory for MemoryStoreFactory {
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
        Ok(memory_provider_definition()?)
    }

    fn contract(&self) -> Result<riesel_schema::CapabilityContract, ConnectorError> {
        Ok(RecordStoreContract::definition()?)
    }

    fn create(&self) -> Result<ConnectedProvider, ConnectorError> {
        Ok(ConnectedProvider {
            definition: memory_provider_definition()?,
            contract: RecordStoreContract::definition()?,
            service: Box::new(Arc::new(MemoryStore::new()) as Arc<dyn RecordStore + Send + Sync>),
        })
    }
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
