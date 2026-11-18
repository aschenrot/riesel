use riesel::{Record, Records, RieselModel, SdkError};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// All persisted fields are described here; no separate JSON model is needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselModel)]
#[riesel(model = "task")]
#[serde(deny_unknown_fields)]
pub struct TaskData {
    pub title: String,
    pub completed: bool,
}

/// Task records already carry the domain value, identity, and revision.
pub type Task = Record<TaskData>;

#[derive(Debug, Error)]
pub enum TodoError {
    #[error("task title must not be empty")]
    EmptyTitle,
    #[error("task not found")]
    NotFound,
    #[error(transparent)]
    Storage(#[from] SdkError),
}

pub struct Todo {
    records: Records<TaskData>,
}

impl Todo {
    pub fn new(records: Records<TaskData>) -> Self {
        Self { records }
    }

    pub fn create_task(&self, title: impl Into<String>) -> Result<Task, TodoError> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(TodoError::EmptyTitle);
        }
        Ok(self.records.create(TaskData {
            title,
            completed: false,
        })?)
    }

    pub fn get_task(&self, id: Uuid) -> Result<Option<Task>, TodoError> {
        Ok(self.records.get(id)?)
    }

    pub fn list_tasks(&self) -> Result<Vec<Task>, TodoError> {
        Ok(self.records.list()?)
    }

    pub fn set_completed(&self, id: Uuid, completed: bool) -> Result<Task, TodoError> {
        let current = self.records.get(id)?.ok_or(TodoError::NotFound)?;
        let mut data = current.value;
        data.completed = completed;
        Ok(self.records.update(id, current.revision, data)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel::storage::RecordStore;
    use riesel_storage_component::MemoryStore;
    use std::sync::Arc;

    fn todo() -> Todo {
        let store = Arc::new(MemoryStore::new()) as Arc<dyn RecordStore + Send + Sync>;
        Todo::new(Records::from_store(store))
    }

    #[test]
    fn creates_a_task() {
        let todo = todo();
        let task = todo.create_task("Build Riesel").unwrap();
        assert_eq!(task.value.title, "Build Riesel");
        assert!(!task.value.completed);
        assert_eq!(task.revision, 1);
        assert_eq!(todo.get_task(task.id).unwrap().unwrap(), task);
    }

    #[test]
    fn rejects_empty_titles() {
        let todo = todo();
        assert!(matches!(
            todo.create_task("   "),
            Err(TodoError::EmptyTitle)
        ));
        assert!(todo.list_tasks().unwrap().is_empty());
    }

    #[test]
    fn completes_a_task() {
        let todo = todo();
        let created = todo.create_task("Build Riesel").unwrap();
        let completed = todo.set_completed(created.id, true).unwrap();
        assert!(completed.value.completed);
        assert_eq!(completed.id, created.id);
        assert_eq!(completed.revision, 2);
        assert!(todo.get_task(created.id).unwrap().unwrap().completed);
    }

    #[test]
    fn lists_tasks() {
        let todo = todo();
        todo.create_task("First").unwrap();
        todo.create_task("Second").unwrap();
        assert_eq!(todo.list_tasks().unwrap().len(), 2);
    }

    #[test]
    fn cannot_complete_missing_task() {
        let todo = todo();
        assert!(matches!(
            todo.set_completed(Uuid::new_v4(), true),
            Err(TodoError::NotFound)
        ));
    }

    #[test]
    fn rust_model_is_single_source_of_truth() {
        let schema = TaskData::model_definition();
        assert!(schema.fields.contains_key("title"));
        assert!(schema.fields.contains_key("completed"));
    }
}
