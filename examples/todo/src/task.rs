use riesel::storage::StoreError;
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

    /// One bounded page for transport adapters; domain model and storage
    /// semantics remain authoritative here, not in the HTTP layer.
    pub fn list_tasks_page(
        &self,
        after: Option<Uuid>,
        limit: u64,
    ) -> Result<riesel::RecordPage<TaskData>, TodoError> {
        Ok(self.records.list_page(after, limit)?)
    }

    /// Update completion only if the caller's observed revision is current.
    /// An obsolete browser or CLI client must not silently overwrite newer data.
    pub fn set_completed(
        &self,
        id: Uuid,
        expected_revision: u64,
        completed: bool,
    ) -> Result<Task, TodoError> {
        self.modify_task(id, expected_revision, |task| task.completed = completed)
    }

    /// Edit a title while preserving concurrent changes and rejecting empty titles.
    pub fn edit_title(
        &self,
        id: Uuid,
        expected_revision: u64,
        title: impl Into<String>,
    ) -> Result<Task, TodoError> {
        let title = title.into();
        if title.trim().is_empty() {
            return Err(TodoError::EmptyTitle);
        }
        self.modify_task(id, expected_revision, |task| task.title = title)
    }

    fn modify_task(
        &self,
        id: Uuid,
        expected_revision: u64,
        change: impl FnOnce(&mut TaskData),
    ) -> Result<Task, TodoError> {
        let mut current = self.get_task(id)?.ok_or(TodoError::NotFound)?;
        change(&mut current.value);
        self.records
            .update(id, expected_revision, current.value)
            .map_err(Self::map_storage_error)
    }

    pub fn delete_task(&self, id: Uuid, expected_revision: u64) -> Result<(), TodoError> {
        self.records
            .delete(id, expected_revision)
            .map_err(Self::map_storage_error)
    }

    fn map_storage_error(error: SdkError) -> TodoError {
        match error {
            SdkError::Store(StoreError::NotFound) => TodoError::NotFound,
            error => TodoError::Storage(error),
        }
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
        let completed = todo
            .set_completed(created.id, created.revision, true)
            .unwrap();
        assert!(completed.value.completed);
        assert_eq!(completed.id, created.id);
        assert_eq!(completed.revision, 2);
        assert!(todo.get_task(created.id).unwrap().unwrap().value.completed);
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
            todo.set_completed(Uuid::new_v4(), 1, true),
            Err(TodoError::NotFound)
        ));
    }

    #[test]
    fn lists_tasks_across_bounded_storage_pages() {
        let todo = todo();
        for index in 0..40 {
            todo.create_task(format!("Task {index}")).unwrap();
        }
        let tasks = todo.list_tasks().unwrap();
        assert_eq!(tasks.len(), 40);
    }

    #[test]
    fn delete_requires_the_observed_revision() {
        use riesel::storage::StoreError;

        let todo = todo();
        let created = todo.create_task("Remove me").unwrap();
        let changed = todo
            .set_completed(created.id, created.revision, true)
            .unwrap();
        assert!(matches!(
            todo.delete_task(created.id, created.revision),
            Err(TodoError::Storage(SdkError::Store(
                StoreError::RevisionConflict
            )))
        ));
        todo.delete_task(created.id, changed.revision).unwrap();
        assert!(todo.get_task(created.id).unwrap().is_none());
    }

    #[test]
    fn title_edit_is_validated_and_increments_revision() {
        let todo = todo();
        let created = todo.create_task("Original").unwrap();
        let edited = todo
            .edit_title(created.id, created.revision, "New title")
            .unwrap();
        assert_eq!(edited.value.title, "New title");
        assert_eq!(edited.id, created.id);
        assert_eq!(edited.revision, 2);
        assert!(!edited.value.completed);

        assert!(matches!(
            todo.edit_title(created.id, edited.revision, "   "),
            Err(TodoError::EmptyTitle)
        ));
        assert_eq!(todo.get_task(created.id).unwrap().unwrap(), edited);
    }

    #[test]
    fn stale_clients_cannot_overwrite_newer_title_or_completion() {
        use riesel::storage::StoreError;
        let todo = todo();
        let first_client = todo.create_task("Draft").unwrap();
        let second_client = todo.get_task(first_client.id).unwrap().unwrap();
        assert_eq!(first_client.revision, second_client.revision);

        let updated = todo
            .edit_title(first_client.id, first_client.revision, "Reviewed")
            .unwrap();
        assert_eq!(updated.revision, 2);
        assert!(matches!(
            todo.set_completed(second_client.id, second_client.revision, true),
            Err(TodoError::Storage(SdkError::Store(
                StoreError::RevisionConflict
            )))
        ));
        assert!(matches!(
            todo.edit_title(second_client.id, second_client.revision, "Stale overwrite"),
            Err(TodoError::Storage(SdkError::Store(
                StoreError::RevisionConflict
            )))
        ));
        assert!(matches!(
            todo.delete_task(second_client.id, second_client.revision),
            Err(TodoError::Storage(SdkError::Store(
                StoreError::RevisionConflict
            )))
        ));
        let saved = todo.get_task(first_client.id).unwrap().unwrap();
        assert_eq!(saved, updated);
        assert_eq!(saved.value.title, "Reviewed");
        assert!(!saved.value.completed);

        let completed = todo.set_completed(saved.id, saved.revision, true).unwrap();
        assert_eq!(completed.revision, 3);
        assert!(completed.value.completed);
        assert_eq!(completed.value.title, "Reviewed");
    }

    #[test]
    fn edit_and_delete_missing_tasks_return_the_domain_not_found_error() {
        let todo = todo();
        let id = Uuid::new_v4();
        assert!(matches!(
            todo.edit_title(id, 1, "Missing"),
            Err(TodoError::NotFound)
        ));
        assert!(matches!(todo.delete_task(id, 1), Err(TodoError::NotFound)));
    }

    #[test]
    fn rust_model_is_single_source_of_truth() {
        let schema = TaskData::model_definition();
        assert!(schema.fields.contains_key("title"));
        assert!(schema.fields.contains_key("completed"));
    }
}
