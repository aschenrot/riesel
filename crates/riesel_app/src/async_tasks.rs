//! Session-scoped, bounded Tokio background tasks.
//!
//! The async handler owns the Tokio executor. Tasks may be spawned only while
//! its scope is active; all tasks are cancelled and joined on that executor
//! before application providers are stopped or a new session is started.
use crate::runtime::{ConnectorError, RuntimeError};
use crate::supervisor::CancellationToken;
use riesel_protocol::SessionToken;
use std::future::Future;
use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::task::JoinHandle;

const MAX_ASYNC_TASKS: usize = 64;

#[derive(Clone)]
struct TaskFailure {
    name: String,
    source: Arc<dyn std::error::Error + Send + Sync>,
}

#[derive(Default)]
struct TaskState {
    active: bool,
    closed: bool,
    handles: Vec<(String, JoinHandle<()>)>,
}

/// Internal async task ownership. No detached application work may survive
/// a successfully completed async handler scope.
pub(crate) struct AsyncTaskGroup {
    state: Mutex<TaskState>,
    failure: Arc<Mutex<Option<TaskFailure>>>,
    session: SessionToken,
    cancellation: CancellationToken,
}

/// Cloneable, session-bound task spawner for typed async application handlers.
/// The runtime owns task lifetimes; retaining a clone does not extend them.
#[derive(Clone)]
pub struct AsyncTasks {
    inner: Arc<AsyncTaskGroup>,
}

impl AsyncTasks {
    pub(crate) fn new(session: SessionToken, cancellation: CancellationToken) -> Self {
        Self {
            inner: Arc::new(AsyncTaskGroup {
                state: Mutex::new(TaskState::default()),
                failure: Arc::new(Mutex::new(None)),
                session,
                cancellation,
            }),
        }
    }

    /// Spawn nonblocking work in the active application's Tokio executor.
    /// Admission is bounded and names are unique among active tasks. A failed
    /// or panicking task revokes the application session and is reported by
    /// the owning application, even if the entrypoint is still pending.
    ///
    /// Cancellation drops the future; a command already sent to a provider
    /// may still have taken effect. Blocking I/O must use an explicit
    /// blocking boundary, not occupy the async executor thread.
    pub fn spawn(
        &self,
        name: impl Into<String>,
        task: impl Future<Output = Result<(), ConnectorError>> + Send + 'static,
    ) -> Result<(), RuntimeError> {
        let name = name.into();
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if !state.active
            || state.closed
            || !self.inner.session.is_active()
            || self.inner.cancellation.is_cancelled()
        {
            return Err(RuntimeError::TaskGroupClosed);
        }
        // Completed watchers have already recorded their result. Recycle
        // their names and budget slots without detaching live tasks.
        state.handles.retain(|(_, handle)| !handle.is_finished());
        if state.handles.len() >= MAX_ASYNC_TASKS {
            return Err(RuntimeError::TaskLimitExceeded {
                limit: MAX_ASYNC_TASKS,
            });
        }
        if name.is_empty()
            || name.len() > 128
            || name.chars().any(char::is_control)
            || state.handles.iter().any(|(existing, _)| existing == &name)
        {
            return Err(RuntimeError::InvalidTaskName(name));
        }
        let executor =
            tokio::runtime::Handle::try_current().map_err(|error| RuntimeError::TaskStart {
                name: name.clone(),
                source: io::Error::other(error),
            })?;
        let session = self.inner.session.clone();
        let worker = executor.spawn(async move {
            let mut work = std::pin::pin!(task);
            let mut expiry = std::pin::pin!(session.cancelled());
            std::future::poll_fn(|cx| {
                use std::task::Poll;
                if let Poll::Ready(result) = work.as_mut().poll(cx) {
                    return Poll::Ready(Some(result));
                }
                if expiry.as_mut().poll(cx).is_ready() {
                    return Poll::Ready(None);
                }
                Poll::Pending
            })
            .await
        });

        let recorded = Arc::clone(&self.inner.failure);
        let cancellation = self.inner.cancellation.clone();
        let session = self.inner.session.clone();
        let task_name = name.clone();
        let watcher = executor.spawn(async move {
            let source: Option<Arc<dyn std::error::Error + Send + Sync>> = match worker.await {
                Ok(None | Some(Ok(()))) => None,
                Ok(Some(Err(error))) => Some(Arc::from(error)),
                Err(error) if error.is_cancelled() && !session.is_active() => None,
                Err(error) => Some(Arc::new(error)),
            };
            if let Some(source) = source {
                let mut guard = recorded.lock().unwrap_or_else(PoisonError::into_inner);
                if guard.is_none() {
                    *guard = Some(TaskFailure {
                        name: task_name,
                        source,
                    });
                }
                drop(guard);
                session.invalidate();
                cancellation.cancel();
            }
        });
        state.handles.push((name, watcher));
        Ok(())
    }

    /// Enter one application-owned async executor. Cannot reopen a closed
    /// scope or attach tasks after session invalidation.
    #[doc(hidden)]
    pub fn begin_scope(&self) -> Result<(), RuntimeError> {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if state.active || state.closed || !self.inner.session.is_active() {
            return Err(RuntimeError::TaskGroupClosed);
        }
        state.active = true;
        Ok(())
    }

    /// Cancel and await every owned task before the executor and providers
    /// are dropped. This is called even when the entrypoint itself fails.
    #[doc(hidden)]
    pub async fn finish_scope(&self) -> Result<(), RuntimeError> {
        let handles = {
            let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
            state.active = false;
            state.closed = true;
            self.inner.session.invalidate();
            self.inner.cancellation.cancel();
            std::mem::take(&mut state.handles)
        };
        for (name, watcher) in handles {
            if let Err(error) = watcher.await {
                self.inner.report(name, Arc::new(error));
            }
        }
        self.failure().map_or(Ok(()), Err)
    }

    pub(crate) fn failure(&self) -> Option<RuntimeError> {
        self.inner.failure()
    }

    /// A synchronous shutdown cannot poll Tokio tasks. Normal async handlers
    /// already joined them in finish_scope; emergency cleanup aborts any
    /// leftover watcher if the async executor unwound unexpectedly.
    pub(crate) fn close_for_shutdown(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        state.active = false;
        self.inner.session.invalidate();
        self.inner.cancellation.cancel();
        for (_, watcher) in std::mem::take(&mut state.handles) {
            watcher.abort();
        }
    }
}

impl AsyncTaskGroup {
    fn report(&self, name: String, source: Arc<dyn std::error::Error + Send + Sync>) {
        let mut failure = self.failure.lock().unwrap_or_else(PoisonError::into_inner);
        if failure.is_none() {
            *failure = Some(TaskFailure { name, source });
        }
        drop(failure);
        self.session.invalidate();
        self.cancellation.cancel();
    }

    fn failure(&self) -> Option<RuntimeError> {
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .map(|failure| RuntimeError::TaskFailed {
                name: failure.name,
                source: failure.source,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    fn executor() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn tasks_cancel_and_join_before_a_scope_exits() {
        executor().block_on(async {
            let session = SessionToken::new();
            let group = AsyncTasks::new(session.clone(), CancellationToken::default());
            assert!(matches!(
                group.spawn("early", async { Ok(()) }),
                Err(RuntimeError::TaskGroupClosed)
            ));
            group.begin_scope().unwrap();
            let dropped = Arc::new(AtomicBool::new(false));
            struct OnDrop(Arc<AtomicBool>);
            impl Drop for OnDrop {
                fn drop(&mut self) {
                    self.0.store(true, Ordering::Release);
                }
            }
            let guard = OnDrop(Arc::clone(&dropped));
            group
                .spawn("pending", async move {
                    let _guard = guard;
                    std::future::pending::<()>().await;
                    Ok(())
                })
                .unwrap();
            tokio::task::yield_now().await;
            group.finish_scope().await.unwrap();
            assert!(dropped.load(Ordering::Acquire));
            assert!(!session.is_active());
            assert!(matches!(
                group.spawn("late", async { Ok(()) }),
                Err(RuntimeError::TaskGroupClosed)
            ));
        });
    }

    #[test]
    fn failure_cancels_waiting_entrypoint_and_preserves_task_name() {
        executor().block_on(async {
            let session = SessionToken::new();
            let group = AsyncTasks::new(session.clone(), CancellationToken::default());
            group.begin_scope().unwrap();
            group
                .spawn("broken", async {
                    Err(io::Error::other("async failure").into())
                })
                .unwrap();
            tokio::time::timeout(Duration::from_secs(1), session.cancelled())
                .await
                .unwrap();
            assert!(matches!(
                group.finish_scope().await,
                Err(RuntimeError::TaskFailed { name, source })
                    if name == "broken" && source.to_string().contains("async failure")
            ));
        });
    }

    #[test]
    fn bounded_admission_and_panic_reporting() {
        executor().block_on(async {
            let session = SessionToken::new();
            let group = AsyncTasks::new(session.clone(), CancellationToken::default());
            group.begin_scope().unwrap();
            for i in 0..MAX_ASYNC_TASKS {
                group
                    .spawn(format!("pending-{i}"), async {
                        std::future::pending::<()>().await;
                        Ok(())
                    })
                    .unwrap();
            }
            assert!(matches!(
                group.spawn("overflow", async { Ok(()) }),
                Err(RuntimeError::TaskLimitExceeded {
                    limit: MAX_ASYNC_TASKS
                })
            ));
            assert!(matches!(
                group.spawn("pending-0", async { Ok(()) }),
                Err(RuntimeError::TaskLimitExceeded { .. })
            ));
            group.finish_scope().await.unwrap();

            let panic_group = AsyncTasks::new(SessionToken::new(), CancellationToken::default());
            panic_group.begin_scope().unwrap();
            panic_group
                .spawn("panic", async {
                    panic!("async task panicked");
                    #[allow(unreachable_code)]
                    Ok(())
                })
                .unwrap();
            tokio::time::timeout(
                Duration::from_secs(1),
                panic_group.inner.session.cancelled(),
            )
            .await
            .unwrap();
            assert!(matches!(
                panic_group.finish_scope().await,
                Err(RuntimeError::TaskFailed { name, .. }) if name == "panic"
            ));
        });
    }
}
