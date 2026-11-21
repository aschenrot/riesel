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
