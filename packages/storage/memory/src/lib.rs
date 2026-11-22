mod memory;
mod server;

pub use memory::MemoryStore;
pub use server::run_storage_server;

use riesel_schema::{ModelDefinition, ProviderDefinition};
use riesel_storage::RecordStoreContract;
use std::collections::{BTreeMap, BTreeSet};

use riesel_app::{ComponentDefinition, ConnectorError, LocalProviderFactory};
use riesel_storage::{
    RecordQuery, RecordQueryContract, RecordQueryRpcDispatcher, RecordStore, RecordStoreRpcDispatcher,
};
use std::sync::Arc;

/// Metadata for the reference provider comes from its typed registration.
pub fn memory_provider_definition() -> Result<ProviderDefinition, ConnectorError> {
    memory_storage_provider()?.definition()
}

/// One authoritative provider registration serves both local and managed
/// instances; the Storage implementation is constructed once per instance.
pub fn memory_storage_provider() -> Result<ComponentDefinition<MemoryStore>, ConnectorError> {
    component(MemoryStore::new)
}

/// Development instances can enforce the exact registered Rust model shapes,
/// even for callers bypassing the typed Records handle.
pub fn memory_storage_provider_with_models(
    models: BTreeMap<String, ModelDefinition>,
) -> Result<ComponentDefinition<MemoryStore>, ConnectorError> {
    component(move || MemoryStore::with_models(models.clone()))
}

fn component(
    create: impl Fn() -> MemoryStore + Send + Sync + 'static,
) -> Result<ComponentDefinition<MemoryStore>, ConnectorError> {
    ComponentDefinition::new("riesel.storage.memory", create).exposes::<RecordStoreContract, _>(
        |state| state as Arc<dyn RecordStore + Send + Sync>,
        RecordStoreRpcDispatcher::new,
    )
}

/// Read-only linked record Query provider, with no mutating Storage RPC
/// contract advertised to a read-only application.
pub fn memory_record_query_provider_with_models(
    models: BTreeMap<String, ModelDefinition>,
    readable: BTreeSet<String>,
) -> Result<ComponentDefinition<MemoryStore>, ConnectorError> {
    query_component(models, readable, false)
}

/// One linked state owner co-exposing independent read-only Query and
/// writable Storage v2 contracts for confirmed sequential read-your-writes.
pub fn memory_record_query_and_storage_provider_with_models(
    models: BTreeMap<String, ModelDefinition>,
    readable: BTreeSet<String>,
) -> Result<ComponentDefinition<MemoryStore>, ConnectorError> {
    query_component(models, readable, true)
}

fn query_component(
    models: BTreeMap<String, ModelDefinition>,
    readable: BTreeSet<String>,
    writable: bool,
) -> Result<ComponentDefinition<MemoryStore>, ConnectorError> {
    let declared: BTreeSet<String> = models.keys().cloned().collect();
    if !readable.is_subset(&declared) || readable.is_empty() {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Query reader models must be a nonempty subset of registered schemas",
        )));
    }
    let provider = ComponentDefinition::new("riesel.storage.memory", move || {
        MemoryStore::with_query_models(models.clone(), readable.clone())
    })
    .exposes::<RecordQueryContract, _>(
        |state| state as Arc<dyn RecordQuery + Send + Sync>,
        RecordQueryRpcDispatcher::new,
    )?;
    if writable {
        provider.exposes::<RecordStoreContract, _>(
            |state| state as Arc<dyn RecordStore + Send + Sync>,
            RecordStoreRpcDispatcher::new,
        )
    } else {
        Ok(provider)
    }
}

#[cfg(test)]
mod provider_tests {
    use super::*;

    #[test]
    fn read_only_query_factory_does_not_advertise_mutating_storage() {
        use riesel_schema::TypeRef;

        let models = BTreeMap::from([(
            "task".into(),
            ModelDefinition {
                fields: BTreeMap::from([("completed".into(), TypeRef::Bool)]),
            },
        )]);
        let readable = BTreeSet::from(["task".into()]);
        let only = memory_record_query_provider_with_models(models.clone(), readable.clone())
            .unwrap()
            .definition()
            .unwrap();
        let both = memory_record_query_and_storage_provider_with_models(models, readable)
            .unwrap()
            .definition()
            .unwrap();
        let read = RecordQueryContract::definition().unwrap().capability;
        let write = RecordStoreContract::definition().unwrap().capability;
        assert!(only.capabilities.contains(&read));
        assert!(!only.capabilities.contains(&write));
        assert!(both.capabilities.contains(&read));
        assert!(both.capabilities.contains(&write));
    }

    #[test]
    fn memory_provider_matches_generated_storage_contract() {
        let provider = memory_provider_definition().unwrap();
        provider.validate().unwrap();
        let contract = riesel_storage::RecordStoreContract::definition().unwrap();
        assert_eq!(provider.capabilities, vec![contract.capability]);
    }
}
