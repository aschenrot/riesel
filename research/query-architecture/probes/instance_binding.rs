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

/// Research-only ergonomic adapter: a typed read-only parameter can be
/// injected with one alias, but current ApplicationParam::declare only
/// registers the capability. It has no model-only registration mechanism.
struct ResearchReadView<T: RieselModel> {
    reader: Capability<ResearchReadOnlyContract>,
    _model: std::marker::PhantomData<T>,
}

impl<T: RieselModel> ResearchReadView<T> {
    fn count(&self) -> Result<u64, StoreError> {
        self.reader.count_records(T::MODEL_NAME)
    }
}

impl<T: RieselModel> riesel::ApplicationParam for ResearchReadView<T> {
    fn default_alias() -> riesel::Result<String> {
        <Capability<ResearchReadOnlyContract> as riesel::ApplicationParam>::default_alias()
    }

    fn declare(application: Application, alias: &str) -> riesel::Result<Application> {
        <Capability<ResearchReadOnlyContract> as riesel::ApplicationParam>::declare(
            application,
            alias,
        )
    }

    fn extract(context: &riesel::Context<'_>, alias: &str) -> riesel::Result<Self> {
        Ok(Self {
            reader: <Capability<ResearchReadOnlyContract> as riesel::ApplicationParam>::extract(
                context, alias,
            )?,
            _model: std::marker::PhantomData,
        })
    }
}

#[test]
fn research_typed_read_parameter_requires_no_writer_but_lacks_model_registration() {
    let ready = Application::new("ResearchOnlyReadView")
        .handler(
            |reader: ResearchReadView<TaskData>| -> Result<(), StoreError> {
                assert_eq!(reader.count()?, 0);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(ready.definition().requires.len(), 1);
    assert!(
        ready.definition().models.is_empty(),
        "research-only ApplicationParam did not register the typed model"
    );
    assert!(ready.definition().model_bindings.is_empty());
    ready
        .provider(reader_only("research.readonly.local"))
        .unwrap()
        .run()
        .unwrap();
}

#[test]
fn compared_authoring_shapes_show_distinct_model_and_alias_tradeoffs() {
    let writer = Application::new("ResearchWriterShape")
        .handler(|_records: riesel::Records<TaskData>| -> Result<(), StoreError> { Ok(()) })
        .unwrap();
    assert_eq!(writer.definition().requires.len(), 1);
    assert!(
        writer
            .definition()
            .models
            .contains_key(TaskData::MODEL_NAME)
    );
    assert_eq!(
        writer.definition().model_bindings[TaskData::MODEL_NAME],
        ["storage"]
    );
    // Records<T> also exposes create/update/delete, even for read-only use.

    let combined = Application::new("ResearchPairShape")
        .handler_with_aliases(
            |_writer: riesel::Records<TaskData>,
             _reader: ResearchReadView<TaskData>|
             -> Result<(), StoreError> { Ok(()) },
            ["writer", "read"],
        )
        .unwrap();
    assert_eq!(combined.definition().requires.len(), 2);
    assert_eq!(
        combined.definition().model_bindings[TaskData::MODEL_NAME],
        ["writer"]
    );
    // There is no relation in today's schema saying that "read" resolves
    // against the same logical data authority as "writer".
    assert!(
        combined
            .definition()
            .requires
            .iter()
            .all(|r| r.name == "writer" || r.name == "read")
    );
}

#[test]
fn requirement_startup_dependency_does_not_establish_shared_data_authority() {
    let prepared = Application::new("ResearchLifecycleNotAuthority")
        .rpc::<RecordStoreContract>("writer")
        .unwrap()
        .rpc::<ResearchReadOnlyContract>("read")
        .unwrap()
        .depends_on("read", ["writer"])
        .unwrap()
        .handler_with_aliases(
            |writer: Capability<RecordStoreContract>,
             reader: Capability<ResearchReadOnlyContract>|
             -> Result<(), StoreError> {
                assert_eq!(reader.count_records(TaskData::MODEL_NAME)?, 0);
                writer.create(
                    TaskData::MODEL_NAME,
                    json!({"title": "lifecycle-only", "completed": false}),
                )?;
                assert_eq!(
                    reader.count_records(TaskData::MODEL_NAME)?,
                    0,
                    "startup ordering does not imply shared storage state"
                );
                Ok(())
            },
            ["writer", "read"],
        )
        .unwrap();
    let read = prepared
        .definition()
        .requires
        .iter()
        .find(|r| r.name == "read")
        .unwrap();
    assert_eq!(read.depends_on, ["writer"]);
    prepared
        .provider(writer_only("research.lifecycle.writer"))
        .unwrap()
        .provider(reader_only("research.lifecycle.reader"))
        .unwrap()
        .run()
        .unwrap();
}

/// The existing serialized AppSchema supports a registered model and a
/// query-only capability requirement without declaring a Storage writer.
/// The high-level ApplicationParam path does not yet generate this schema.
#[test]
fn schema_allows_registered_read_only_model_without_storage_binding() {
    use riesel::schema::{AppDefinition, AppSchema, CapabilityRequirement};

    let query = ResearchReadOnlyContract::definition().unwrap().capability;
    let schema = AppSchema {
        version: 1,
        app: AppDefinition {
            name: "ResearchModelOnlySchema".into(),
        },
        models: models(),
        model_bindings: BTreeMap::new(),
        requires: vec![CapabilityRequirement {
            name: "read".into(),
            capability: query.clone(),
            depends_on: Vec::new(),
        }],
    };
    schema.validate().unwrap();
    assert_eq!(schema.requires.len(), 1);
    assert_eq!(schema.requires[0].capability, query);
    assert!(schema.models.contains_key(TaskData::MODEL_NAME));
    assert!(schema.model_bindings.is_empty());
    // This proves schema expressiveness only. It does not demonstrate that
    // an arbitrary reader is authorized to access the declared model.
}

/// Research-only composite injection: one Rust handler argument, but two
/// independent capability requirements. It is NOT a validated owner relation.
struct ResearchComposite<T>
where
    T: RieselModel + serde::Serialize + serde::de::DeserializeOwned,
{
    writer: riesel::Records<T>,
    reader: Capability<ResearchReadOnlyContract>,
}

impl<T> riesel::ApplicationParam for ResearchComposite<T>
where
    T: RieselModel + serde::Serialize + serde::de::DeserializeOwned,
{
    fn default_alias() -> riesel::Result<String> {
        Ok("storage".into())
    }

    fn declare(application: Application, alias: &str) -> riesel::Result<Application> {
        application
            .records_at::<T>(alias)?
            .rpc::<ResearchReadOnlyContract>(format!("{alias}_read"))
    }

    fn extract(context: &riesel::Context<'_>, alias: &str) -> riesel::Result<Self> {
        Ok(Self {
            writer: context.records_at::<T>(alias)?,
            reader: <Capability<ResearchReadOnlyContract> as riesel::ApplicationParam>::extract(
                context,
                &format!("{alias}_read"),
            )?,
        })
    }
}

#[test]
fn composite_authoring_reduces_handler_arguments_but_not_ownership_risk() {
    let paired = Application::new("ResearchCompositePaired")
        .handler(
            |combined: ResearchComposite<TaskData>| -> Result<(), StoreError> {
                combined
                    .writer
                    .create(TaskData {
                        title: "paired".into(),
                        completed: false,
                    })
                    .unwrap();
                assert_eq!(combined.reader.count_records(TaskData::MODEL_NAME)?, 1);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(paired.definition().requires.len(), 2);
    assert!(
        paired
            .definition()
            .models
            .contains_key(TaskData::MODEL_NAME)
    );
    assert_eq!(
        paired.definition().model_bindings[TaskData::MODEL_NAME],
        ["storage"]
    );
    assert!(
        paired
            .definition()
            .requires
            .iter()
            .any(|r| r.name == "storage_read")
    );
    paired
        .provider(coexposed("research.composite.shared"))
        .unwrap()
        .run()
        .unwrap();

    let split = Application::new("ResearchCompositeSplit")
        .handler(
            |combined: ResearchComposite<TaskData>| -> Result<(), StoreError> {
                combined
                    .writer
                    .create(TaskData {
                        title: "split".into(),
                        completed: false,
                    })
                    .unwrap();
                assert_eq!(
                    combined.reader.count_records(TaskData::MODEL_NAME)?,
                    0,
                    "one injected Rust parameter cannot prove shared data authority"
                );
                Ok(())
            },
        )
        .unwrap();
    split
        .provider(writer_only("research.composite.writer"))
        .unwrap()
        .provider(reader_only("research.composite.reader"))
        .unwrap()
        .run()
        .unwrap();
}

struct ResearchStorageReadProjection(riesel::Records<TaskData>);

impl riesel::ApplicationParam for ResearchStorageReadProjection {
    fn default_alias() -> riesel::Result<String> {
        <riesel::Records<TaskData> as riesel::ApplicationParam>::default_alias()
    }

    fn declare(application: Application, alias: &str) -> riesel::Result<Application> {
        <riesel::Records<TaskData> as riesel::ApplicationParam>::declare(application, alias)
    }

    fn extract(context: &riesel::Context<'_>, alias: &str) -> riesel::Result<Self> {
        Ok(Self(
            <riesel::Records<TaskData> as riesel::ApplicationParam>::extract(context, alias)?,
        ))
    }
}

impl ResearchStorageReadProjection {
    fn count_first_page(&self) -> usize {
        self.0.list_page(None, 32).unwrap().records.len()
    }
}

#[test]
fn rust_read_only_projection_does_not_attenuate_storage_rpc_permission() {
    let prepared = Application::new("ResearchProjectedStorageRead")
        .handler(
            |reader: ResearchStorageReadProjection| -> Result<(), StoreError> {
                assert_eq!(reader.count_first_page(), 0);
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(prepared.definition().requires.len(), 1);
    let expected = RecordStoreContract::definition().unwrap().capability;
    assert_eq!(prepared.definition().requires[0].capability, expected);
    let definition = RecordStoreContract::definition().unwrap();
    assert!(
        definition.operations.contains_key("create")
            && definition.operations.contains_key("update")
            && definition.operations.contains_key("delete"),
        "hiding mutation methods in the injected Rust wrapper does not remove them from the RPC capability"
    );
    prepared
        .provider(writer_only("research.projected.local"))
        .unwrap()
        .run()
        .unwrap();
}

#[test]
fn authority_relationship_needs_explicit_schema_evolution_not_reused_bindings() {
    let app = Application::new("ResearchAuthoritySchema")
        .records_at::<TaskData>("writer")
        .unwrap()
        .rpc::<ResearchReadOnlyContract>("read")
        .unwrap();
    let schema = app.definition().clone();
    schema.validate().unwrap();

    // Unknown capability relationship fields are rejected by today's
    // deny_unknown_fields AppSchema/CapabilityRequirement deserializer.
    // This demonstrates a forward-compatibility/versioning obligation,
    // not that a particular future JSON field or schema version is chosen.
    let mut encoded = serde_json::to_value(&schema).unwrap();
    let read_requirement = encoded["requires"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|item| item["name"] == "read")
        .unwrap();
    read_requirement["research_same_authority_as"] = json!("writer");
    assert!(riesel::schema::parse_schema(&encoded.to_string()).is_err());

    // Reusing today's model_bindings list is not a typed query-owner
    // relation: it accepts both aliases without distinguishing whether
    // one is a writer, read facet or a different data authority.
    let mut modified = schema;
    modified
        .model_bindings
        .get_mut(TaskData::MODEL_NAME)
        .unwrap()
        .push("read".into());
    modified.validate().unwrap();
    assert_eq!(modified.model_bindings[TaskData::MODEL_NAME], ["writer", "read"]);
    assert_eq!(modified.requires.len(), 2);
}
