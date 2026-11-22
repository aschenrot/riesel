//! #138 research-only: per-instance provider pairing / read-only contract shape.
//! This verifies a negative linked binding; it is NOT authorization or a
//! production Query implementation. No public API or provider is changed.
use riesel::storage::{RecordStore, RecordStoreContract, RecordStoreRpcDispatcher, StoreError};
use riesel::{Application, Capability, RieselModel, capability};
use riesel_app::{ComponentDefinition, LocalProviderFactory};
use riesel_storage_component::MemoryStore;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::Arc;
use todo::TaskData;

#[capability(id = "research.query.record.reader", version = 1, client_trait)]
trait ResearchReadOnly {
    /// Count the current model's records by scanning Storage v2 UUID pages.
    #[query]
    fn count_records(&self, model: &str) -> Result<u64, StoreError>;
}

impl ResearchReadOnly for MemoryStore {
    fn count_records(&self, model: &str) -> Result<u64, StoreError> {
        // Research-only v2 full scan. Not a native selective query or
        // a production count/consistency guarantee.
        let mut result = 0;
        let mut after = None;
        loop {
            let page = self.list_page(model, after, 32)?;
            result += page.records.len() as u64;
            match page.next_cursor {
                Some(next) => after = Some(next),
                None => return Ok(result),
            }
        }
    }
}

fn models() -> BTreeMap<String, riesel_schema::ModelDefinition> {
    BTreeMap::from([(
        TaskData::MODEL_NAME.to_string(),
        TaskData::model_definition(),
    )])
}

fn writer_only(id: &'static str) -> impl LocalProviderFactory {
    ComponentDefinition::new(id, || MemoryStore::with_models(models()))
        .exposes::<RecordStoreContract, _>(
            |state| state as Arc<dyn RecordStore + Send + Sync>,
            RecordStoreRpcDispatcher::new,
        )
        .unwrap()
}

fn reader_only(id: &'static str) -> impl LocalProviderFactory {
    ComponentDefinition::new(id, || MemoryStore::with_models(models()))
        .exposes::<ResearchReadOnlyContract, _>(
            |state| state as Arc<dyn ResearchReadOnly + Send + Sync>,
            ResearchReadOnlyRpcDispatcher::new,
        )
        .unwrap()
}

fn coexposed(id: &'static str) -> impl LocalProviderFactory {
    ComponentDefinition::new(id, || MemoryStore::with_models(models()))
        .exposes::<RecordStoreContract, _>(
            |state| state as Arc<dyn RecordStore + Send + Sync>,
            RecordStoreRpcDispatcher::new,
        )
        .unwrap()
        .exposes::<ResearchReadOnlyContract, _>(
            |state| state as Arc<dyn ResearchReadOnly + Send + Sync>,
            ResearchReadOnlyRpcDispatcher::new,
        )
        .unwrap()
}

#[test]
fn unrelated_linked_read_and_write_capabilities_can_resolve_to_distinct_state() {
    Application::new("ResearchSplitState")
        .handler_with_aliases(
            |writer: Capability<RecordStoreContract>,
             read: Capability<ResearchReadOnlyContract>|
             -> Result<(), StoreError> {
                assert_eq!(read.count_records(TaskData::MODEL_NAME)?, 0);
                writer.create(
                    TaskData::MODEL_NAME,
                    json!({"title": "writer-owned", "completed": false}),
                )?;
                assert_eq!(
                    writer
                        .list_page(TaskData::MODEL_NAME, None, 32)?
                        .records
                        .len(),
                    1
                );
                assert_eq!(
                    read.count_records(TaskData::MODEL_NAME)?,
                    0,
                    "reader's independent provider never observed writer mutation"
                );
                Ok(())
            },
            ["writer", "read"],
        )
        .unwrap()
        .provider(writer_only("research.writer.local"))
        .unwrap()
        .provider(reader_only("research.reader.local"))
        .unwrap()
        .run()
        .unwrap();
}

#[test]
fn one_coexposed_linked_component_can_give_read_your_writes() {
    Application::new("ResearchOneOwner")
        .handler_with_aliases(
            |writer: Capability<RecordStoreContract>,
             read: Capability<ResearchReadOnlyContract>|
             -> Result<(), StoreError> {
                assert_eq!(read.count_records(TaskData::MODEL_NAME)?, 0);
                writer.create(
                    TaskData::MODEL_NAME,
                    json!({"title": "shared-owner", "completed": false}),
                )?;
                assert_eq!(read.count_records(TaskData::MODEL_NAME)?, 1);
                Ok(())
            },
            ["writer", "read"],
        )
        .unwrap()
        .provider(coexposed("research.shared.local"))
        .unwrap()
        .run()
        .unwrap();
}

#[test]
fn reader_contract_has_no_mutation_operations_but_is_not_authorization() {
    use riesel_schema::OperationKind;
    let read = ResearchReadOnlyContract::definition().unwrap();
    let store = RecordStoreContract::definition().unwrap();
    assert_eq!(read.operations.len(), 1);
    assert_eq!(read.operations["count_records"].kind, OperationKind::Query);
    assert!(
        read.operations
            .values()
            .all(|op| op.kind != OperationKind::Command)
    );
    assert_eq!(store.operations["create"].kind, OperationKind::Command);
    // The absence of a command in a trait is NOT a guarantee that the
    // runtime refuses unauthorized clients or authenticates the service.
}
