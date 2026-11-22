//! Deliberately opt-in, non-sensitive **demo-only** T1 Todo summary.
//!
//! This facade is a separately selected app-owned read model over the real
//! Todo domain and registered record provider. It does not expose raw task
//! titles, task IDs, record revisions, storage handles, or writes.
//! Even aggregate task status may be sensitive in a real installation:
//! never enable this on private data under shared-OS-user T1 without an
//! explicit application-level classification.
use crate::{TaskData, Todo};
use riesel::{Records, RieselType, capability};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The first *bounded page* of task statuses, never a claimed global total.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct TodoPublicSummary {
    pub inspected: u64,
    pub open_in_page: u64,
    pub completed_in_page: u64,
    pub more_available: bool,
}

/// An unavailable domain query does not imply that there are no records.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize, RieselType)]
pub enum TodoPublicReadError {
    #[error("Todo summary is unavailable: {0}")]
    Unavailable(String),
}

/// A versioned query-only app facade. Not a Storage/Query exporter.
#[capability(id = "riesel.todo.public_summary", version = 1)]
pub trait TodoPublicSummaryRead {
    /// Read up to one full Storage page. Never exports record contents.
    #[query]
    async fn snapshot(&self) -> Result<TodoPublicSummary, TodoPublicReadError>;
}

pub struct TodoPublicSummaryFacade {
    records: Records<TaskData>,
}

impl TodoPublicSummaryFacade {
    pub fn new(records: Records<TaskData>) -> Self {
        Self { records }
    }
}

impl TodoPublicSummaryRead for TodoPublicSummaryFacade {
    async fn snapshot(&self) -> Result<TodoPublicSummary, TodoPublicReadError> {
        // Storage caps one page to 32 records. This is not an unbounded
        // list_tasks() scan, and partial pages must not be called totals.
        let page = Todo::new(self.records.clone())
            .list_tasks_page(None, 32)
            .map_err(|error| TodoPublicReadError::Unavailable(error.to_string()))?;
        let count = page.records.len() as u64;
        let completed = page
            .records
            .iter()
            .filter(|task| task.value.completed)
            .count() as u64;
        Ok(TodoPublicSummary {
            inspected: count,
            open_in_page: count - completed,
            completed_in_page: completed,
            more_available: page.next_cursor.is_some(),
        })
    }
}
