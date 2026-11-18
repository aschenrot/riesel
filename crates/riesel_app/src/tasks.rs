//! Runtime-owned application tasks. A task receives a shared cooperative
//! cancellation token and is joined before any provider is torn down.
use crate::runtime::{ConnectorError, RuntimeError};
use crate::supervisor::CancellationToken;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};

#[derive(Debug, Clone)]
struct TaskFailure {
    name: String,
    message: String,
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
                let message = match outcome {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error.to_string()),
                    Err(_) => Some("task panicked".into()),
                };
                if let Some(message) = message {
                    let mut recorded = failure.lock().unwrap_or_else(PoisonError::into_inner);
                    if recorded.is_none() {
                        *recorded = Some(TaskFailure {
                            name: task_name,
                            message,
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

    pub(crate) fn failure(&self) -> Option<RuntimeError> {
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .map(|failure| RuntimeError::TaskFailed {
                name: failure.name,
                message: failure.message,
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
                        message: "task worker panicked".into(),
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
            Err(RuntimeError::TaskFailed { name, message })
                if name == "failing" && message.contains("broken task")
        ));
        assert!(matches!(
            group.failure(),
            Some(RuntimeError::TaskFailed { .. })
        ));
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
