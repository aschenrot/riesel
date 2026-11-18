mod dispatch;
mod memory;
mod server;

pub use memory::MemoryStore;
pub use server::run_storage_server;

use riesel_schema::{ContractError, ProviderDefinition};
use riesel_storage::{RecordStoreContract, StorageServiceInfo};

/// Metadata for the built-in memory provider. Its capability reference
/// comes directly from the generated Rust trait contract.
pub fn memory_provider_definition() -> Result<ProviderDefinition, ContractError> {
    Ok(ProviderDefinition {
        id: "riesel.storage.memory".into(),
        capabilities: vec![riesel_storage::RecordStoreContract::definition()?.capability],
    })
}

pub(crate) fn memory_service_info() -> Result<StorageServiceInfo, ContractError> {
    Ok(StorageServiceInfo {
        provider: memory_provider_definition()?,
        contract: RecordStoreContract::definition()?,
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
