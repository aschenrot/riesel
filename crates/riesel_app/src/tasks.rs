//! Runtime-owned application tasks. A task receives a shared cooperative
//! cancellation token and is joined before any provider is torn down.
use crate::runtime::{ConnectorError, RuntimeError};
use crate::supervisor::CancellationToken;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

/// Maximum number of concurrently registered application-owned OS threads.
/// They are joined on shutdown and cannot be forcibly interrupted.
const MAX_APPLICATION_TASKS: usize = 64;

#[derive(Debug, Clone)]
struct TaskFailure {
    name: String,
    source: Arc<dyn std::error::Error + Send + Sync>,
}

#[derive(Default)]
struct TaskState {
    handles: Vec<(String, JoinHandle<()>)>,
    closed: bool,
}

/// Task errors and panics cancel the session, rather than being silently
/// detached. Threads cannot be forcibly killed; tasks must cooperate.
pub(crate) struct TaskGroup {
    state: Mutex<TaskState>,
    failure: Arc<Mutex<Option<TaskFailure>>>,
    cancellation: CancellationToken,
}

impl TaskGroup {
    pub(crate) fn new(cancellation: CancellationToken) -> Self {
        Self {
            state: Mutex::new(TaskState::default()),
            failure: Arc::new(Mutex::new(None)),
            cancellation,
        }
    }

    pub(crate) fn spawn(
        &self,
        name: String,
        task: impl FnOnce(CancellationToken) -> Result<(), ConnectorError> + Send + 'static,
    ) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed || self.cancellation.is_cancelled() {
            return Err(RuntimeError::TaskGroupClosed);
        }
        // Finished workers no longer consume the concurrent task budget.
        // Join them before recycling slots so unexpected worker panics are
        // observed rather than silently detaching their JoinHandles.
        self.reap_finished(&mut state);
        if self.cancellation.is_cancelled() {
            return Err(RuntimeError::TaskGroupClosed);
        }
        if state.handles.len() >= MAX_APPLICATION_TASKS {
            return Err(RuntimeError::TaskLimitExceeded {
                limit: MAX_APPLICATION_TASKS,
            });
        }
        if name.is_empty()
            || name.len() > 128
            || name.chars().any(char::is_control)
            || state.handles.iter().any(|(existing, _)| existing == &name)
        {
            return Err(RuntimeError::InvalidTaskName(name));
        }
        let token = self.cancellation.clone();
        let failure = Arc::clone(&self.failure);
        let task_name = name.clone();
        let handle = thread::Builder::new()
            .name(format!("riesel-task-{name}"))
            .spawn(move || {
                let outcome = panic::catch_unwind(AssertUnwindSafe(|| task(token.clone())));
                let source: Option<Arc<dyn std::error::Error + Send + Sync>> = match outcome {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(Arc::from(error)),
                    Err(_) => Some(Arc::new(std::io::Error::other("task panicked"))),
                };
                if let Some(source) = source {
                    let mut recorded = failure.lock().unwrap_or_else(PoisonError::into_inner);
                    if recorded.is_none() {
                        *recorded = Some(TaskFailure {
                            name: task_name,
                            source,
                        });
                    }
                    token.cancel();
                }
            })
            .map_err(|source| RuntimeError::TaskStart {
                name: name.clone(),
                source,
            })?;
        state.handles.push((name, handle));
        Ok(())
    }

    fn reap_finished(&self, state: &mut TaskState) {
        let mut pending = Vec::with_capacity(state.handles.len());
        for (name, handle) in std::mem::take(&mut state.handles) {
            if handle.is_finished() {
                if handle.join().is_err() {
                    let mut failure = self.failure.lock().unwrap_or_else(PoisonError::into_inner);
                    if failure.is_none() {
                        *failure = Some(TaskFailure {
                            name,
                            source: Arc::new(std::io::Error::other("task worker panicked")),
                        });
                    }
                    self.cancellation.cancel();
                }
            } else {
                pending.push((name, handle));
            }
        }
        state.handles = pending;
    }

    pub(crate) fn failure(&self) -> Option<RuntimeError> {
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .map(|failure| RuntimeError::TaskFailed {
                name: failure.name,
                source: failure.source,
            })
    }

    /// Cancel and join every registered task. Do not return early on failure,
    /// so all task-owned references finish before provider teardown.
    pub(crate) fn join(&self) -> Result<(), RuntimeError> {
        let handles = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state.closed = true;
            self.cancellation.cancel();
            std::mem::take(&mut state.handles)
        };
        for (name, handle) in handles {
            if handle.join().is_err() {
                let mut failure = self.failure.lock().unwrap_or_else(PoisonError::into_inner);
                if failure.is_none() {
                    *failure = Some(TaskFailure {
                        name,
                        source: Arc::new(std::io::Error::other("task worker panicked")),
                    });
                }
            }
        }
        self.failure().map_or(Ok(()), Err)
    }
}

impl Drop for TaskGroup {
    fn drop(&mut self) {
        let _ = self.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::time::Duration;

    #[test]
    fn shutdown_signals_and_joins_cooperative_tasks() {
        let token = CancellationToken::default();
        let group = TaskGroup::new(token.clone());
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&finished);
        group
            .spawn("worker".into(), move |cancel| {
                assert!(cancel.wait_cancelled_timeout(Duration::from_secs(5)));
                flag.store(true, std::sync::atomic::Ordering::Release);
                Ok(())
            })
            .unwrap();
        group.join().unwrap();
        assert!(token.is_cancelled());
        assert!(finished.load(std::sync::atomic::Ordering::Acquire));
        assert!(matches!(
            group.spawn("late".into(), |_| Ok(())),
            Err(RuntimeError::TaskGroupClosed)
        ));
    }

    #[test]
    fn background_failure_cancels_entire_session_and_is_retained() {
        let token = CancellationToken::default();
        let group = TaskGroup::new(token.clone());
        group
            .spawn("failing".into(), |_| {
                Err(io::Error::other("broken task").into())
            })
            .unwrap();
        assert!(token.wait_cancelled_timeout(Duration::from_secs(5)));
        assert!(matches!(
            group.join(),
            Err(RuntimeError::TaskFailed { name, source })
                if name == "failing" && source.to_string().contains("broken task")
        ));
        assert!(matches!(
            group.failure(),
            Some(RuntimeError::TaskFailed { .. })
        ));
    }

    #[test]
    fn task_budget_blocks_unbounded_os_thread_creation() {
        let token = CancellationToken::default();
        let group = TaskGroup::new(token);
        for i in 0..MAX_APPLICATION_TASKS {
            group
                .spawn(format!("worker-{i}"), |cancel| {
                    cancel.wait_cancelled_timeout(Duration::from_secs(5));
                    Ok(())
                })
                .unwrap();
        }
        assert!(matches!(
            group.spawn("over-budget".into(), |_| Ok(())),
            Err(RuntimeError::TaskLimitExceeded {
                limit: MAX_APPLICATION_TASKS
            })
        ));
        group.join().unwrap();
    }

    #[test]
    fn completed_tasks_recycle_slots_and_names() {
        use std::thread;
        let group = TaskGroup::new(CancellationToken::default());
        for _ in 0..(MAX_APPLICATION_TASKS + 5) {
            group.spawn("reusable".into(), |_| Ok(())).unwrap();
            while !group
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .handles[0]
                .1
                .is_finished()
            {
                thread::yield_now();
            }
        }
        group.join().unwrap();
    }

    #[test]
    fn task_panics_are_reported_instead_of_detached() {
        let token = CancellationToken::default();
        let group = TaskGroup::new(token.clone());
        group
            .spawn("panic".into(), |_| panic!("test injected panic"))
            .unwrap();
        assert!(token.wait_cancelled_timeout(Duration::from_secs(5)));
        assert!(matches!(
            group.join(),
            Err(RuntimeError::TaskFailed { name, .. }) if name == "panic"
        ));
    }
}
