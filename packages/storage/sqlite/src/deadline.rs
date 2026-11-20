//! Cooperative elapsed-time limits for offline SQLite maintenance.
use crate::maintenance::{SnapshotBudget, denied};
use riesel_storage::StoreError;
use rusqlite::Connection;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub(crate) struct Deadline {
    started: Instant,
    max_elapsed: Duration,
}

impl Deadline {
    pub(crate) fn start(budget: SnapshotBudget) -> Result<Self, StoreError> {
        if budget.max_records == 0
            || budget.max_json_bytes == 0
            || budget.max_snapshot_bytes == 0
            || budget.max_sidecar_bytes == 0
            || budget.max_elapsed.is_zero()
        {
            return Err(denied("all snapshot budgets must be positive"));
        }
        Ok(Self {
            started: Instant::now(),
            max_elapsed: budget.max_elapsed,
        })
    }

    pub(crate) fn check(self) -> Result<(), StoreError> {
        if self.started.elapsed() >= self.max_elapsed {
            return Err(denied("maintenance elapsed-time budget exceeded"));
        }
        Ok(())
    }

    /// Interrupt long SQLite statements. Blocked OS I/O is not preempted.
    pub(crate) fn install(self, connection: &mut Connection) -> Result<(), StoreError> {
        let started = self.started;
        let max_elapsed = self.max_elapsed;
        connection
            .progress_handler(1_000, Some(move || started.elapsed() >= max_elapsed))
            .map_err(crate::database::failed)
    }
}

#[cfg(test)]
mod tests {
    use super::Deadline;
    use rusqlite::Connection;
    use std::time::{Duration, Instant};

    #[test]
    fn expired_deadline_interrupts_long_sql_execution() {
        let mut connection = Connection::open_in_memory().unwrap();
        let deadline = Deadline {
            started: Instant::now() - Duration::from_secs(1),
            max_elapsed: Duration::from_millis(1),
        };
        deadline.install(&mut connection).unwrap();
        let result = connection.execute_batch(
            "WITH RECURSIVE seq(n) AS (
                 SELECT 1 UNION ALL SELECT n+1 FROM seq WHERE n<1000000
             ) SELECT sum(n) FROM seq;",
        );
        assert!(matches!(result, Err(rusqlite::Error::SqliteFailure(_, _))));
    }
}
