//! Explicit, provider-owned blocking work for native async component methods.
//!
//! A started OS thread is not cancelled by dropping its RPC/request future.
//! The component's owner must close the group and join every worker before
//! releasing provider state or completing managed-host shutdown.

use crate::SessionToken;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::{Semaphore, oneshot};

const MAX_PROVIDER_BLOCKING: usize = 8;

/// Error at the native async provider's explicit blocking-work boundary.
#[derive(Debug, Error)]
pub enum ProviderBlockingError {
    #[error("provider blocking-work scope is closed or its session expired")]
    Closed,
    #[error("invalid provider blocking-work name: {0}")]
    InvalidName(String),
    #[error("provider blocking-work limit exceeded (maximum {limit} active workers)")]
    LimitExceeded { limit: usize },
    #[error("provider blocking worker {name} could not start: {source}")]
    ThreadStart {
        name: String,
        #[source]
        source: std::io::Error,
    },
    #[error("provider blocking worker {name} panicked")]
    Panicked { name: String },
    #[error("provider blocking worker {name} stopped without returning")]
    WorkerStopped { name: String },
}

#[derive(Clone)]
pub struct ProviderCancellation {
    session: SessionToken,
}

impl ProviderCancellation {
    /// Check shutdown without blocking.
    pub fn is_cancelled(&self) -> bool {
        !self.session.is_active()
    }

    /// Cooperatively wait for provider shutdown, waking at least every 10ms.
    /// The host cannot forcibly terminate arbitrary synchronous Rust code.
    pub fn wait_cancelled_timeout(&self, duration: Duration) -> bool {
        let start = Instant::now();
        loop {
            if self.is_cancelled() {
                return true;
            }
            let Some(remaining) = duration.checked_sub(start.elapsed()) else {
                return self.is_cancelled();
            };
            thread::sleep(remaining.min(Duration::from_millis(10)));
        }
    }
}

#[derive(Default)]
struct WorkerState {
    closed: bool,
    joined: bool,
    handles: Vec<JoinHandle<()>>,
}

struct ProviderBlockingGroup {
    session: SessionToken,
    workers: Mutex<WorkerState>,
    joined: Condvar,
    slots: Arc<Semaphore>,
    failures: Arc<Mutex<Vec<String>>>,
}

/// Component-scoped blocking resource. Clone into the component's state,
/// invoke `run` from its native async methods, and let the component runtime
/// own `close_and_join`. No worker can outlive a clean provider shutdown.
#[derive(Clone)]
pub struct ProviderBlocking {
    group: Arc<ProviderBlockingGroup>,
}

impl ProviderBlocking {
    /// Construct an isolated provider work scope tied to a session.
    /// Component factories receive this automatically when using the
    /// explicit `factory_with_blocking` declaration.
    pub fn new(session: SessionToken) -> Self {
        Self {
            group: Arc::new(ProviderBlockingGroup {
                session,
                workers: Mutex::new(WorkerState::default()),
                joined: Condvar::new(),
                slots: Arc::new(Semaphore::new(MAX_PROVIDER_BLOCKING)),
                failures: Arc::new(Mutex::new(Vec::new())),
            }),
        }
    }

    /// Run a synchronous operation away from Tokio's event loop, subject to
    /// a strict eight-worker admission limit. Rejected work is never started.
    /// A cancelled RPC may abandon its result but never its running worker:
    /// the provider scope still owns the worker's join handle.
    ///
    /// The user closure must observe `ProviderCancellation` and place
    /// boundaries on blocking I/O. A started OS thread cannot be preempted.
    pub async fn run<T, F>(
        &self,
        name: impl Into<String>,
        task: F,
    ) -> Result<T, ProviderBlockingError>
    where
        T: Send + 'static,
        F: FnOnce(ProviderCancellation) -> T + Send + 'static,
    {
        let name = name.into();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err(ProviderBlockingError::InvalidName(name));
        }
        let (sender, receiver) = oneshot::channel();
        {
            let mut workers = self
                .group
                .workers
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if workers.closed || !self.group.session.is_active() {
                return Err(ProviderBlockingError::Closed);
            }
            // Fail-fast admission while owning the state lock guarantees that
            // closed scopes always reject before reporting capacity.
            let permit = Arc::clone(&self.group.slots)
                .try_acquire_owned()
                .map_err(|_| ProviderBlockingError::LimitExceeded {
                    limit: MAX_PROVIDER_BLOCKING,
                })?;
            // Reap completed workers by joining, never by detaching their
            // JoinHandles. All still-running workers remain owned by this scope.
            let mut active = Vec::new();
            for handle in std::mem::take(&mut workers.handles) {
                if handle.is_finished() {
                    let _ = handle.join();
                } else {
                    active.push(handle);
                }
            }
            workers.handles = active;
            let cancellation = ProviderCancellation {
                session: self.group.session.clone(),
            };
            let failures = Arc::clone(&self.group.failures);
            let worker_name = name.clone();
            let handle = thread::Builder::new()
                .name(format!("riesel-provider-{name}"))
                .spawn(move || {
                    let _permit = permit;
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        if cancellation.is_cancelled() {
                            return None;
                        }
                        Some(task(cancellation))
                    }));
                    if result.is_err() {
                        failures
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(worker_name);
                    }
                    let _ = sender.send(result);
                })
                .map_err(|source| ProviderBlockingError::ThreadStart {
                    name: name.clone(),
                    source,
                })?;
            workers.handles.push(handle);
        }

        match receiver.await {
            Ok(Ok(Some(value))) if self.group.session.is_active() => Ok(value),
            Ok(Err(_)) => Err(ProviderBlockingError::Panicked { name }),
            Ok(Ok(_)) | Err(_) if !self.group.session.is_active() => {
                Err(ProviderBlockingError::Closed)
            }
            Ok(Ok(None)) | Err(_) => Err(ProviderBlockingError::WorkerStopped { name }),
            Ok(Ok(Some(_))) => Err(ProviderBlockingError::Closed),
        }
    }

    /// Stop admitting work, signal cooperative cancellation and synchronously
    /// join every started worker. The owning provider calls this before
    /// dropping its state, including after abnormal managed-host exit.
    ///
    /// If user-supplied blocking code ignores cancellation, joining waits
    /// for it to return. Managed application shutdown retains its external
    /// process-kill timeout as the last resort.
    pub fn close_and_join(&self) -> std::io::Result<()> {
        let work_to_join = {
            let mut state = self
                .group
                .workers
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if state.closed {
                // Concurrent callers must not return before the first owner
                // joins the workers it removed from the shared registry.
                while !state.joined {
                    state = self
                        .group
                        .joined
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                None
            } else {
                state.closed = true;
                self.group.session.invalidate();
                Some(std::mem::take(&mut state.handles))
            }
        };
        if let Some(workers) = work_to_join {
            for worker in workers {
                if worker.join().is_err() {
                    self.group
                        .failures
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .push("uncaught panic".into());
                }
            }
            let mut state = self
                .group
                .workers
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state.joined = true;
            self.group.joined.notify_all();
        }
        let failed = self
            .group
            .failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if failed.is_empty() {
            Ok(())
        } else {
            Err(std::io::Error::other(format!(
                "provider blocking workers failed: {}",
                failed.join(", ")
            )))
        }
    }
}

impl Drop for ProviderBlockingGroup {
    fn drop(&mut self) {
        self.session.invalidate();
        let workers = self
            .workers
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);
        for handle in std::mem::take(&mut workers.handles) {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn executor() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn blocking_provider_work_does_not_block_tokio_and_joins() {
        executor().block_on(async {
            let blocking = ProviderBlocking::new(SessionToken::new());
            let finished = Arc::new(AtomicBool::new(false));
            let observed = Arc::clone(&finished);
            let work = blocking.run("slow", move |_| {
                thread::sleep(Duration::from_millis(80));
                observed.store(true, Ordering::Release);
                17_u64
            });
            let mut work = Box::pin(work);
            assert!(
                tokio::time::timeout(Duration::from_millis(15), &mut work)
                    .await
                    .is_err()
            );
            assert_eq!(work.await.unwrap(), 17);
            blocking.close_and_join().unwrap();
            assert!(finished.load(Ordering::Acquire));
            assert!(matches!(
                blocking.run("late", |_| 1_u64).await,
                Err(ProviderBlockingError::Closed)
            ));
        });
    }

    #[test]
    fn dropping_waiter_does_not_detach_blocking_worker() {
        executor().block_on(async {
            let token = SessionToken::new();
            let blocking = ProviderBlocking::new(token);
            let finished = Arc::new(AtomicBool::new(false));
            let done = Arc::clone(&finished);
            let work = blocking.run("abandoned", move |cancel| {
                assert!(cancel.wait_cancelled_timeout(Duration::from_secs(2)));
                done.store(true, Ordering::Release);
            });
            let mut work = Box::pin(work);
            assert!(
                tokio::time::timeout(Duration::from_millis(15), &mut work)
                    .await
                    .is_err()
            );
            drop(work);
            blocking.close_and_join().unwrap();
            assert!(finished.load(Ordering::Acquire));
        });
    }

    #[test]
    fn concurrent_shutdown_callers_wait_for_the_same_worker_join() {
        executor().block_on(async {
            let blocking = ProviderBlocking::new(SessionToken::new());
            let started = Arc::new(AtomicBool::new(false));
            let finished = Arc::new(AtomicBool::new(false));
            let start_marker = Arc::clone(&started);
            let finish_marker = Arc::clone(&finished);
            let work = blocking.run("owned", move |cancellation| {
                start_marker.store(true, Ordering::Release);
                assert!(cancellation.wait_cancelled_timeout(Duration::from_secs(2)));
                thread::sleep(Duration::from_millis(140));
                finish_marker.store(true, Ordering::Release);
            });
            let mut work = Box::pin(work);
            assert!(
                tokio::time::timeout(Duration::from_millis(15), &mut work)
                    .await
                    .is_err()
            );
            assert!(started.load(Ordering::Acquire));
            let first_owner = blocking.clone();
            let first = thread::spawn(move || first_owner.close_and_join().unwrap());
            let deadline = Instant::now() + Duration::from_secs(1);
            while blocking.group.session.is_active() {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            let second_owner = blocking.clone();
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let second = thread::spawn(move || {
                sender.send(()).unwrap();
                second_owner.close_and_join().unwrap();
            });
            receiver.recv_timeout(Duration::from_secs(1)).unwrap();
            thread::sleep(Duration::from_millis(20));
            assert!(!second.is_finished(), "second closer returned before the join");
            first.join().unwrap();
            second.join().unwrap();
            assert!(finished.load(Ordering::Acquire));
        });
    }

    #[test]
    fn strict_capacity_and_panic_reporting() {
        executor().block_on(async {
            let blocking = ProviderBlocking::new(SessionToken::new());
            let started = Arc::new(AtomicUsize::new(0));
            let mut pending = Vec::new();
            for i in 0..MAX_PROVIDER_BLOCKING {
                let worker = blocking.clone();
                let started = Arc::clone(&started);
                pending.push(tokio::spawn(async move {
                    worker
                        .run(format!("worker-{i}"), move |cancel| {
                            started.fetch_add(1, Ordering::AcqRel);
                            assert!(cancel.wait_cancelled_timeout(Duration::from_secs(2)));
                        })
                        .await
                }));
            }
            tokio::time::timeout(Duration::from_secs(1), async {
                while started.load(Ordering::Acquire) < MAX_PROVIDER_BLOCKING {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(matches!(
                blocking.run("ninth", |_| ()).await,
                Err(ProviderBlockingError::LimitExceeded {
                    limit: MAX_PROVIDER_BLOCKING
                })
            ));
            blocking.close_and_join().unwrap();
            for job in pending {
                let _ = job.await.unwrap();
            }

            let failed = ProviderBlocking::new(SessionToken::new());
            let panic = failed
                .run("panic", |_| -> () { panic!("worker failed") })
                .await;
            assert!(matches!(panic, Err(ProviderBlockingError::Panicked { .. })));
            assert!(failed.close_and_join().is_err());
        });
    }
}
