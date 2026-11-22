//! Example-specific, opt-in *non-sensitive* Todo read projection for T1.
//! Only aggregate summary counts are published. Task titles, IDs,
//! private metadata and domain mutations are never part of this interface.
use crate::{TaskData, Todo};
use riesel::{Records, RieselType, capability};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct PublicTodoSummary {
    pub total: u64,
    pub completed: u64,
    pub pending: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType, Error)]
pub enum PublicTodoError {
    #[error("the app-owned Todo read is unavailable")]
    Unavailable,
    #[error("the bounded public read cannot represent the full Todo state")]
    Incomplete,
}

/// This is a **demo-specific** contract, not generic permission to publish a
/// user's private tasks. Apps must explicitly opt into T1 for this information.
#[capability(id = "example.todo.public_summary", version = 1)]
pub trait PublicTodoRead {
    /// Aggregate non-sensitive public *demo* task completion counts only.
    #[query]
    async fn summary(&self) -> Result<PublicTodoSummary, PublicTodoError>;
}

pub struct PublicTodoReadFacade {
    records: Records<TaskData>,
}

impl PublicTodoReadFacade {
    pub fn new(records: Records<TaskData>) -> Self {
        Self { records }
    }
}

impl PublicTodoRead for PublicTodoReadFacade {
    async fn summary(&self) -> Result<PublicTodoSummary, PublicTodoError> {
        // Bound both provider work and transient in-memory allocations.
        // Large/private Todo datasets are NOT silently enumerated/exported
        // or reported as an incomplete total.
        let page = Todo::new(self.records.clone())
            .list_tasks_page(None, 64)
            .map_err(|_| PublicTodoError::Unavailable)?;
        if page.next_cursor.is_some() {
            return Err(PublicTodoError::Incomplete);
        }
        let completed = page.records.iter().filter(|r| r.value.completed).count() as u64;
        let total = page.records.len() as u64;
        Ok(PublicTodoSummary {
            total,
            completed,
            pending: total - completed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel::storage::RecordStore;
    use riesel_storage_component::MemoryStore;
    use std::sync::Arc;

    fn records() -> Records<TaskData> {
        Records::from_store(
            Arc::new(MemoryStore::new()) as Arc<dyn RecordStore + Send + Sync>
        )
    }

    #[test]
    fn never_returns_an_incomplete_public_summary_as_a_complete_total() {
        let records = records();
        let todo = Todo::new(records.clone());
        for index in 0..65 {
            todo.create_task(format!("test task {index}")).unwrap();
        }
        let outcome = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(PublicTodoReadFacade::new(records).summary());
        assert_eq!(outcome, Err(PublicTodoError::Incomplete));
    }
}
