use riesel::storage::{RecordStore, StoreError, StoredRecord};

use riesel::RieselModel;
use riesel::schema::{ModelDefinition, RecordError};
use serde::{Deserialize, Serialize};
use serde_json::json;
use thiserror::Error;
use uuid::Uuid;

const TASK_MODEL: &str = "task";

#[derive(Debug, PartialEq, Eq)]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    pub completed: bool,
    pub revision: u64,
}

#[derive(Debug, Serialize, Deserialize, RieselModel)]
#[riesel(model = "task")]
#[serde(deny_unknown_fields)]
struct TaskData {
    title: String,
    completed: bool,
}

#[derive(Debug, Error)]
pub enum TodoError {
    #[error("task title must not be empty")]
    EmptyTitle,

    #[error("task not found")]
    NotFound,

    #[error(transparent)]
    InvalidRecord(#[from] RecordError),

    #[error(transparent)]
    Storage(#[from] StoreError),

    #[error("invalid stored task data: {0}")]
    InvalidStoredData(#[from] serde_json::Error),
}

pub struct Todo<S: RecordStore> {
    store: S,
    task_model: ModelDefinition,
}

impl<S: RecordStore> Todo<S> {
    pub fn new(store: S, task_model: ModelDefinition) -> Self {
        Self { store, task_model }
    }

    pub fn create_task(&self, title: impl Into<String>) -> Result<Task, TodoError> {
        let title = title.into();

        if title.trim().is_empty() {
            return Err(TodoError::EmptyTitle);
        }

        let data = json!({
            "title": title,
            "completed": false,
        });

        self.task_model.validate_record(&data)?;

        let record = self.store.create(TASK_MODEL, data)?;

        self.to_task(record)
    }

    pub fn get_task(&self, id: Uuid) -> Result<Option<Task>, TodoError> {
        self.store
            .get(TASK_MODEL, id)?
            .map(|record| self.to_task(record))
            .transpose()
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>, TodoError> {
        self.store
            .list(TASK_MODEL)?
            .into_iter()
            .map(|record| self.to_task(record))
            .collect()
    }

    pub fn set_completed(&self, id: Uuid, completed: bool) -> Result<Task, TodoError> {
        let record = self.store.get(TASK_MODEL, id)?.ok_or(TodoError::NotFound)?;

        self.task_model.validate_record(&record.data)?;

        let mut data = record.data;

        let fields = data.as_object_mut().ok_or(RecordError::ExpectedObject)?;

        fields.insert("completed".into(), json!(completed));

        self.task_model.validate_record(&data)?;

        let updated = self.store.update(TASK_MODEL, id, record.revision, data)?;

        self.to_task(updated)
    }

    fn to_task(&self, record: StoredRecord) -> Result<Task, TodoError> {
        self.task_model.validate_record(&record.data)?;

        let data: TaskData = serde_json::from_value(record.data)?;

        Ok(Task {
            id: record.id,
            title: data.title,
            completed: data.completed,
            revision: record.revision,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel::schema::parse_schema;
    use riesel_storage_component::MemoryStore;

    fn todo() -> Todo<MemoryStore> {
        let mut schema = parse_schema(include_str!("../app.json")).expect("Todo schema must parse");

        schema.validate().expect("Todo schema must be valid");

        let task_model = schema
            .models
            .remove("task")
            .expect("Todo must define a task model");

        Todo::new(MemoryStore::new(), task_model)
    }

    #[test]
    fn creates_a_task() {
        let todo = todo();

        let task = todo.create_task("Build Riesel").unwrap();

        assert_eq!(task.title, "Build Riesel");
        assert!(!task.completed);
        assert_eq!(task.revision, 1);

        let retrieved = todo.get_task(task.id).unwrap().unwrap();

        assert_eq!(retrieved, task);
    }

    #[test]
    fn rejects_empty_titles() {
        let todo = todo();

        let result = todo.create_task("   ");

        assert!(matches!(result, Err(TodoError::EmptyTitle)));
        assert!(todo.list_tasks().unwrap().is_empty());
    }

    #[test]
    fn completes_a_task() {
        let todo = todo();

        let created = todo.create_task("Build Riesel").unwrap();

        let completed = todo.set_completed(created.id, true).unwrap();

        assert!(completed.completed);
        assert_eq!(completed.id, created.id);
        assert_eq!(completed.revision, 2);

        let retrieved = todo.get_task(created.id).unwrap().unwrap();

        assert!(retrieved.completed);
    }

    #[test]
    fn lists_tasks() {
        let todo = todo();

        todo.create_task("First").unwrap();
        todo.create_task("Second").unwrap();

        let tasks = todo.list_tasks().unwrap();

        assert_eq!(tasks.len(), 2);
    }

    #[test]
    fn cannot_complete_missing_task() {
        let todo = todo();

        let result = todo.set_completed(Uuid::new_v4(), true);

        assert!(matches!(result, Err(TodoError::NotFound)));
    }

    #[test]
    fn resolves_and_injects_storage_without_coupling_todo_to_a_provider() {
        use riesel::{CapabilityBindings, ProviderRegistry};
        use std::collections::BTreeMap;
        use std::sync::Arc;

        let mut schema = parse_schema(include_str!("../app.json")).unwrap();
        let mut registry = ProviderRegistry::new();
        registry
            .register(riesel_storage_component::memory_provider_definition().unwrap())
            .unwrap();

        let resolved = registry.resolve(&schema, &BTreeMap::new()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].requirement, "storage");
        assert_eq!(resolved[0].provider, "riesel.storage.memory");

        let mut instances = CapabilityBindings::<dyn RecordStore>::new();
        instances
            .register("riesel.storage.memory", Arc::new(MemoryStore::new()))
            .unwrap();

        let store = instances.get(&resolved[0]).unwrap();
        let task_model = schema.models.remove("task").unwrap();
        let todo = Todo::new(store, task_model);

        let task = todo.create_task("Test provider injection").unwrap();
        assert_eq!(todo.get_task(task.id).unwrap().unwrap(), task);
    }
    #[test]
    fn rust_task_model_matches_declared_json_model() {
        let schema = parse_schema(include_str!("../app.json")).unwrap();
        assert_eq!(TaskData::model_definition(), schema.models["task"]);
    }
}
