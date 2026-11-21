//! #107 research-only: co-located typed status query and RecordStore v2.
//! This query scans UUID pages; it is NOT an optimized production query.
use riesel::storage::{RecordStore, RecordStoreContract, RecordStoreRpcDispatcher, StoreError};
use riesel::{Application, Capability, RieselModel, capability};
use riesel_app::{ComponentDefinition, LocalProviderFactory};
use riesel_storage_component::MemoryStore;
use riesel_storage_sqlite::SqliteStore;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};
use todo::TaskData;
use uuid::Uuid;

#[capability(id = "research.todo.records.status", version = 1, client_trait)]
trait ResearchStatusQuery {
    /// Return open record IDs by scanning the original record pages.
    #[query]
    fn open_record_ids(&self, after: Option<Uuid>, limit: u64) -> Result<Vec<Uuid>, StoreError>;
}

fn scan_open_ids(
    store: &impl RecordStore,
    after: Option<Uuid>,
    limit: u64,
) -> Result<Vec<Uuid>, StoreError> {
    if !(1..=32).contains(&limit) {
        return Err(StoreError::InvalidPageSize);
    }
    let mut found = Vec::new();
    let mut cursor = after;
    loop {
        let page = store.list_page(TaskData::MODEL_NAME, cursor, 32)?;
        for item in page.records {
            let data: TaskData = serde_json::from_value(item.data)
                .map_err(|error| StoreError::Failed(error.to_string()))?;
            if !data.completed {
                found.push(item.id);
                if found.len() >= limit as usize {
                    return Ok(found);
                }
            }
        }
        match page.next_cursor {
            Some(next) => cursor = Some(next),
            None => return Ok(found),
        }
    }
}

impl ResearchStatusQuery for MemoryStore {
    fn open_record_ids(&self, after: Option<Uuid>, limit: u64) -> Result<Vec<Uuid>, StoreError> {
        scan_open_ids(self, after, limit)
    }
}

impl ResearchStatusQuery for SqliteStore {
    fn open_record_ids(&self, after: Option<Uuid>, limit: u64) -> Result<Vec<Uuid>, StoreError> {
        scan_open_ids(self, after, limit)
    }
}

fn research_provider<S>(
    id: &str,
    create: impl Fn() -> S + Send + Sync + 'static,
) -> impl LocalProviderFactory
where
    S: RecordStore + ResearchStatusQuery + Send + Sync + 'static,
{
    ComponentDefinition::new(id, create)
        .exposes::<RecordStoreContract, _>(
            |state| state as Arc<dyn RecordStore + Send + Sync>,
            RecordStoreRpcDispatcher::new,
        )
        .unwrap()
        .exposes::<ResearchStatusQueryContract, _>(
            |state| state as Arc<dyn ResearchStatusQuery + Send + Sync>,
            ResearchStatusQueryRpcDispatcher::new,
        )
        .unwrap()
}

fn verify_shared_provider(provider: impl LocalProviderFactory + 'static) {
    Application::new("TodoResearchCoLocatedQueries")
        .handler_with_aliases(
            |writer: Capability<RecordStoreContract>,
             query: Capability<ResearchStatusQueryContract>|
             -> Result<(), StoreError> {
                let done: Value = json!({"title": "done", "completed": true});
                let open: Value = json!({"title": "open", "completed": false});
                let first = writer.create(TaskData::MODEL_NAME, done)?;
                let second = writer.create(TaskData::MODEL_NAME, open)?;

                assert_eq!(query.open_record_ids(None, 32)?, vec![second.id]);
                let updated = writer.update(
                    TaskData::MODEL_NAME,
                    first.id,
                    first.revision,
                    json!({"title": "done", "completed": false}),
                )?;
                let mut expected = vec![first.id, second.id];
                expected.sort_unstable();
                assert_eq!(query.open_record_ids(None, 32)?, expected);
                assert_eq!(updated.id, first.id);
                assert_eq!(updated.revision, first.revision + 1);
                assert!(matches!(
                    writer.update(
                        TaskData::MODEL_NAME,
                        first.id,
                        first.revision,
                        json!({"title": "stale", "completed": false}),
                    ),
                    Err(StoreError::RevisionConflict)
                ));
                assert_eq!(query.open_record_ids(None, 32)?, expected);
                Ok(())
            },
            ["writer", "query"],
        )
        .unwrap()
        .provider(provider)
        .unwrap()
        .run()
        .unwrap();
}

#[test]
fn memory_query_and_writer_observe_one_state() {
    let models = BTreeMap::from([(
        TaskData::MODEL_NAME.to_owned(),
        TaskData::model_definition(),
    )]);
    verify_shared_provider(research_provider("research.todo.memory", move || {
        MemoryStore::with_models(models.clone())
    }));
}

#[test]
fn sqlite_query_and_writer_observe_one_state() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tasks.db");
    let models = BTreeMap::from([(
        TaskData::MODEL_NAME.to_owned(),
        TaskData::model_definition(),
    )]);
    verify_shared_provider(research_provider("research.todo.sqlite", move || {
        SqliteStore::open(&path, models.clone()).unwrap()
    }));
}

/// #107 research-only candidate for a future fail-closed same-owner preflight.
/// This is deliberately NOT a production resolver or authenticated instance proof.
/// The runtime must separately verify its actual managed child/session at startup.
fn research_require_same_authoritative_instance(
    plan: &riesel_app::DeploymentPlan,
) -> Result<(), &'static str> {
    use riesel_app::PlannedTarget;
    let writer = plan
        .services
        .iter()
        .find(|service| service.requirement == "writer")
        .ok_or("missing writer")?;
    let query = plan
        .services
        .iter()
        .find(|service| service.requirement == "query")
        .ok_or("missing query")?;

    match (&writer.target, &query.target) {
        // A registered local provider ID refers to one provider instance in
        // the existing per-application linked component registry.
        (PlannedTarget::Local { provider: a }, PlannedTarget::Local { provider: b }) if a == b => {
            Ok(())
        }
        // Explicitly named managed requirements can share the same launch,
        // but matching the instance string alone is insufficient without
        // compatible pinned provider metadata at preflight.
        (
            PlannedTarget::Managed {
                instance: Some(a), ..
            },
            PlannedTarget::Managed {
                instance: Some(b), ..
            },
        ) if a == b
            && writer.expected_provider.is_some()
            && writer.expected_provider == query.expected_provider =>
        {
            Ok(())
        }
        // Unnamed managed requirements launch separately; external TCP
        // addresses are not authenticated instance identities.
        _ => Err("query and writer do not have a verified shared local/managed target"),
    }
}

#[test]
fn query_writer_pairing_can_be_checked_before_side_effects() {
    use riesel_app::{DeploymentConfig, ServiceEndpoint};
    use std::path::PathBuf;

    let models = BTreeMap::from([(
        TaskData::MODEL_NAME.to_owned(),
        TaskData::model_definition(),
    )]);
    let provider = research_provider("research.todo.memory", move || {
        MemoryStore::with_models(models.clone())
    });
    let definition = provider.definition().unwrap();
    let app = Application::new("TodoResearchPairing")
        .handler_with_aliases(
            |_writer: Capability<RecordStoreContract>,
             _query: Capability<ResearchStatusQueryContract>|
             -> Result<(), StoreError> { Ok(()) },
            ["writer", "query"],
        )
        .unwrap()
        .provider(provider)
        .unwrap();

    // The same local component instance is admissible.
    let local = DeploymentConfig::new(BTreeMap::from([
        (
            "writer".into(),
            ServiceEndpoint::Local {
                provider: "research.todo.memory".into(),
            },
        ),
        (
            "query".into(),
            ServiceEndpoint::Local {
                provider: "research.todo.memory".into(),
            },
        ),
    ]));
    let local_plan = app.deployment_plan(&local).unwrap();
    assert!(research_require_same_authoritative_instance(&local_plan).is_ok());

    // The existing planner deliberately permits independent managed aliases.
    // A future explicit pairing constraint must reject the split plan before
    // launching either managed child, even with identical provider pins.
    let managed = |instance: Option<&str>| ServiceEndpoint::Managed {
        instance: instance.map(str::to_owned),
        program: PathBuf::from("research-only-not-executed"),
        args: Vec::new(),
        startup_timeout_ms: 10_000,
        shutdown_timeout_ms: 2_000,
    };
    let mut shared = DeploymentConfig::new(BTreeMap::from([
        ("writer".into(), managed(Some("shared"))),
        ("query".into(), managed(Some("shared"))),
    ]));
    shared
        .expected_providers
        .insert("writer".into(), definition.clone());
    shared
        .expected_providers
        .insert("query".into(), definition.clone());
    let plan = app.deployment_plan(&shared).unwrap();
    assert!(research_require_same_authoritative_instance(&plan).is_ok());

    let mut split = shared.clone();
    split
        .services
        .insert("query".into(), managed(Some("other")));
    let plan = app.deployment_plan(&split).unwrap();
    assert!(research_require_same_authoritative_instance(&plan).is_err());

    let mut unnamed = shared.clone();
    unnamed.services.insert("query".into(), managed(None));
    let plan = app.deployment_plan(&unnamed).unwrap();
    assert!(research_require_same_authoritative_instance(&plan).is_err());

    // A matching service name without verified provider pins cannot satisfy
    // the proposed strict managed co-location predicate.
    let mut unpinned = shared;
    unpinned.expected_providers.clear();
    // If core preflight already rejects unpinned managed configuration,
    // that is also a safe failure; it is not evidence of an accepted pairing.
    match app.deployment_plan(&unpinned) {
        Ok(plan) => assert!(research_require_same_authoritative_instance(&plan).is_err()),
        Err(_) => {}
    }
}
