//! Session-scoped, bounded Tokio background tasks.
//!
//! The async handler owns the Tokio executor. Tasks may be spawned only while
//! its scope is active; all tasks are cancelled and joined on that executor
//! before application providers are stopped or a new session is started.
use crate::runtime::{ConnectorError, RuntimeError};
use crate::supervisor::CancellationToken;
use riesel_protocol::SessionToken;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::io;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinHandle;

const MAX_ASYNC_TASKS: usize = 64;
/// Separate, deliberately smaller budget for OS-thread blocking work.
const MAX_BLOCKING_TASKS: usize = 8;

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
    executor: Option<tokio::runtime::Handle>,
}

/// Internal async task ownership. No detached application work may survive
/// a successfully completed async handler scope.
pub(crate) struct AsyncTaskGroup {
    state: Mutex<TaskState>,
    failure: Arc<Mutex<Option<TaskFailure>>>,
    session: SessionToken,
    cancellation: CancellationToken,
    blocking_slots: Arc<Semaphore>,
    stopping: AtomicBool,
    stop_signal: Notify,
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
                blocking_slots: Arc::new(Semaphore::new(MAX_BLOCKING_TASKS)),
                stopping: AtomicBool::new(false),
                stop_signal: Notify::new(),
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
        let (mut state, executor) = self.reserve(&name)?;
        let session = self.inner.session.clone();
        let stopping = Arc::clone(&self.inner);
        let worker = executor.spawn(async move {
            let mut work = std::pin::pin!(task);
            let mut expiry = std::pin::pin!(session.cancelled());
            let mut shutdown = std::pin::pin!(stopping.stop_requested());
            std::future::poll_fn(|cx| {
                use std::task::Poll;
                if let Poll::Ready(result) = work.as_mut().poll(cx) {
                    return Poll::Ready(Some(result));
                }
                if expiry.as_mut().poll(cx).is_ready()
                    || shutdown.as_mut().poll(cx).is_ready()
                {
                    return Poll::Ready(None);
                }
                Poll::Pending
            })
            .await
        });
        let watcher = self.watch(&executor, name.clone(), worker);
        state.handles.push((name, watcher));
        Ok(())
    }

    /// Explicit boundary for synchronous work in an async application.
    /// The closure runs on Tokio's blocking worker pool, not the session
    /// reactor. At most eight blocking jobs can be admitted concurrently.
    ///
    /// Started blocking jobs cannot be forcibly interrupted by Rust.
    /// Observe the supplied token, use bounded I/O and return promptly
    /// when cancelled. Riesel joins them before stopping providers.
    pub fn spawn_blocking(
        &self,
        name: impl Into<String>,
        task: impl FnOnce(CancellationToken) -> Result<(), ConnectorError> + Send + 'static,
    ) -> Result<(), RuntimeError> {
        let name = name.into();
        let (mut state, executor) = self.reserve(&name)?;
        let permit = Arc::clone(&self.inner.blocking_slots)
            .try_acquire_owned()
            .map_err(|_| RuntimeError::TaskLimitExceeded {
                limit: MAX_BLOCKING_TASKS,
            })?;
        let cancellation = self.inner.cancellation.clone();
        let worker = executor.spawn_blocking(move || {
            let _permit = permit;
            if cancellation.is_cancelled() {
                return Ok(());
            }
            task(cancellation)
        });
        let watcher = self.watch(
            &executor,
            name.clone(),
            async move { worker.await.map(Some) },
        );
        state.handles.push((name, watcher));
        Ok(())
    }

    fn reserve(
        &self,
        name: &str,
    ) -> Result<(MutexGuard<'_, TaskState>, tokio::runtime::Handle), RuntimeError> {
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
        // Finished watchers have recorded their outcome. Recycle active
        // names and admission slots without detaching live task futures.
        state.handles.retain(|(_, handle)| !handle.is_finished());
        if state.handles.len() >= MAX_ASYNC_TASKS {
            return Err(RuntimeError::TaskLimitExceeded {
                limit: MAX_ASYNC_TASKS,
            });
        }
        if name.is_empty()
            || name.len() > 128
            || name.chars().any(char::is_control)
            || state.handles.iter().any(|(existing, _)| existing == name)
        {
            return Err(RuntimeError::InvalidTaskName(name.into()));
        }
        let executor = state
            .executor
            .as_ref()
            .ok_or(RuntimeError::TaskGroupClosed)?
            .clone();
        Ok((state, executor))
    }

    fn watch<F>(
        &self,
        executor: &tokio::runtime::Handle,
        name: String,
        completed: F,
    ) -> JoinHandle<()>
    where
        F: Future<Output = Result<Option<Result<(), ConnectorError>>, tokio::task::JoinError>>
            + Send
            + 'static,
    {
        let inner = Arc::clone(&self.inner);
        executor.spawn(async move {
            let source: Option<Arc<dyn std::error::Error + Send + Sync>> = match completed.await {
                Ok(None | Some(Ok(()))) => None,
                Ok(Some(Err(error))) => Some(Arc::from(error)),
                Err(error) if error.is_cancelled() && !inner.session.is_active() => None,
                Err(error) => Some(Arc::new(error)),
            };
            if let Some(source) = source {
                inner.report(name, source);
            }
        })
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
        let executor =
            tokio::runtime::Handle::try_current().map_err(|error| RuntimeError::TaskStart {
                name: "async-scope".into(),
                source: io::Error::other(error),
            })?;
        state.executor = Some(executor);
        state.active = true;
        Ok(())
    }

    /// Stop admitting tasks, cancel and join owned work while capability handles
    /// are still valid for lifecycle cleanup hooks. A worker that ignores its
    /// cooperative cancellation token can still block this operation.
    #[doc(hidden)]
    pub async fn prepare_stop(&self) -> Result<(), RuntimeError> {
        let handles = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state.active = false;
            state.closed = true;
            state.executor = None;
            self.inner.stopping.store(true, Ordering::Release);
            self.inner.stop_signal.notify_waiters();
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

    /// The final scope boundary revokes all capability handles after any
    /// application stop hooks and owned background work have completed.
    #[doc(hidden)]
    pub async fn finish_scope(&self) -> Result<(), RuntimeError> {
        let result = self.prepare_stop().await;
        self.inner.session.invalidate();
        result
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
        state.executor = None;
        self.inner.stopping.store(true, Ordering::Release);
        self.inner.stop_signal.notify_waiters();
        self.inner.session.invalidate();
        self.inner.cancellation.cancel();
        for (_, watcher) in std::mem::take(&mut state.handles) {
            watcher.abort();
        }
    }
}

impl AsyncTaskGroup {
    async fn stop_requested(self: Arc<Self>) {
        loop {
            let notified = self.stop_signal.notified();
            let mut notified = std::pin::pin!(notified);
            notified.as_mut().enable();
            if self.stopping.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

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
    fn preparing_lifecycle_cleanup_drains_tasks_without_expiring_handles() {
        executor().block_on(async {
            let session = SessionToken::new();
            let cancellation = CancellationToken::default();
            let tasks = AsyncTasks::new(session.clone(), cancellation.clone());
            tasks.begin_scope().unwrap();
            tasks
                .spawn("pending", async {
                    std::future::pending::<()>().await;
                    Ok(())
                })
                .unwrap();
            tasks.prepare_stop().await.unwrap();
            assert!(session.is_active());
            assert!(cancellation.is_cancelled());
            assert!(matches!(
                tasks.spawn("late", async { Ok(()) }),
                Err(RuntimeError::TaskGroupClosed)
            ));
            tasks.finish_scope().await.unwrap();
            assert!(!session.is_active());
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
    fn cloned_spawner_uses_its_owner_executor_across_threads() {
        executor().block_on(async {
            let group = AsyncTasks::new(SessionToken::new(), CancellationToken::default());
            group.begin_scope().unwrap();
            let observed = Arc::new(AtomicBool::new(false));
            let observed_by_worker = Arc::clone(&observed);
            let spawner = group.clone();
            std::thread::spawn(move || {
                // The calling thread has no Tokio runtime. Work must still
                // enter the executor captured when the session began.
                spawner
                    .spawn("cross-thread", async move {
                        observed_by_worker.store(true, Ordering::Release);
                        Ok(())
                    })
                    .unwrap();
            })
            .join()
            .unwrap();
            tokio::time::timeout(Duration::from_secs(1), async {
                while !observed.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            group.finish_scope().await.unwrap();
        });
    }

    #[test]
    fn blocking_work_is_bounded_and_does_not_stall_the_async_executor() {
        executor().block_on(async {
            let group = AsyncTasks::new(SessionToken::new(), CancellationToken::default());
            group.begin_scope().unwrap();
            let completed = Arc::new(AtomicBool::new(false));
            let success = Arc::clone(&completed);
            group
                .spawn_blocking("bounded-io", move |cancellation| {
                    assert!(!cancellation.is_cancelled());
                    std::thread::sleep(Duration::from_millis(90));
                    success.store(true, Ordering::Release);
                    Ok(())
                })
                .unwrap();
            // The current-thread executor must make progress while the OS
            // worker is sleeping; blocking work must not run inline.
            tokio::time::timeout(
                Duration::from_millis(40),
                tokio::time::sleep(Duration::from_millis(10)),
            )
            .await
            .unwrap();
            tokio::time::timeout(Duration::from_secs(1), async {
                while !completed.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            group.finish_scope().await.unwrap();
        });
    }

    #[test]
    fn blocking_budget_and_cancellation_join_all_workers() {
        executor().block_on(async {
            let group = AsyncTasks::new(SessionToken::new(), CancellationToken::default());
            group.begin_scope().unwrap();
            let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            for index in 0..MAX_BLOCKING_TASKS {
                let flag = Arc::clone(&completed);
                let started_by_worker = Arc::clone(&started);
                group
                    .spawn_blocking(format!("thread-{index}"), move |cancel| {
                        started_by_worker.fetch_add(1, Ordering::AcqRel);
                        assert!(cancel.wait_cancelled_timeout(Duration::from_secs(5)));
                        flag.fetch_add(1, Ordering::AcqRel);
                        Ok(())
                    })
                    .unwrap();
            }
            tokio::time::timeout(Duration::from_secs(2), async {
                while started.load(Ordering::Acquire) != MAX_BLOCKING_TASKS {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(matches!(
                group.spawn_blocking("ninth", |_| Ok(())),
                Err(RuntimeError::TaskLimitExceeded {
                    limit: MAX_BLOCKING_TASKS
                })
            ));
            group.finish_scope().await.unwrap();
            assert_eq!(completed.load(Ordering::Acquire), MAX_BLOCKING_TASKS);
            assert!(matches!(
                group.spawn_blocking("late", |_| Ok(())),
                Err(RuntimeError::TaskGroupClosed)
            ));
        });
    }

    #[test]
    fn failed_blocking_worker_is_a_named_application_task_failure() {
        executor().block_on(async {
            let session = SessionToken::new();
            let group = AsyncTasks::new(session.clone(), CancellationToken::default());
            group.begin_scope().unwrap();
            group
                .spawn_blocking("blocking-failure", |_| {
                    Err(io::Error::other("blocking work failed").into())
                })
                .unwrap();
            tokio::time::timeout(Duration::from_secs(1), session.cancelled())
                .await
                .unwrap();
            assert!(matches!(
                group.finish_scope().await,
                Err(RuntimeError::TaskFailed { name, source })
                    if name == "blocking-failure" &&
                        source.to_string().contains("blocking work failed")
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
