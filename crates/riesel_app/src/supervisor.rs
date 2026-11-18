//! Monitor runtime-owned children independently of application business logic.
//! The manager owns the processes; the monitor only observes their exit state.
//! No restart is attempted. Required managed providers use fail-fast policy.

use crate::runtime::RuntimeError;
use riesel_manager::{ProcessKey, ProcessRegistry, ProcessStatus};
use std::collections::BTreeMap;
use std::io;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MONITOR_INTERVAL: Duration = Duration::from_millis(30);

#[derive(Default)]
struct CancellationState {
    cancelled: AtomicBool,
    gate: Mutex<()>,
    changed: Condvar,
}

/// Cooperative signal set when a required managed service exits unexpectedly.
/// The token does not forcibly interrupt application threads or blocking I/O.
#[derive(Clone, Default)]
pub struct CancellationToken {
    state: Arc<CancellationState>,
}

impl CancellationToken {
    /// Whether a required managed service failed or the runtime is stopping.
    pub fn is_cancelled(&self) -> bool {
        self.state.cancelled.load(Ordering::Acquire)
    }

    /// Block at most `timeout` waiting for cancellation. Returns true if
    /// cancellation has been requested, including when already cancelled.
    pub fn wait_cancelled_timeout(&self, timeout: Duration) -> bool {
        if self.is_cancelled() {
            return true;
        }
        let guard = self.state.gate.lock().unwrap_or_else(PoisonError::into_inner);
        let _waiting = self
            .state
            .changed
            .wait_timeout_while(guard, timeout, |_| !self.is_cancelled())
            .unwrap_or_else(PoisonError::into_inner);
        self.is_cancelled()
    }

    fn cancel(&self) {
        let _guard = self.state.gate.lock().unwrap_or_else(PoisonError::into_inner);
        if !self.state.cancelled.swap(true, Ordering::AcqRel) {
            self.state.changed.notify_all();
        }
    }
}

/// Cloneable snapshot. A plain io::Error is not cloneable, so retain its kind
/// and message when propagating an inspection error to multiple observers.
#[derive(Debug, Clone)]
enum ServiceFailure {
    Exited { requirement: String, status: ExitStatus },
    Status {
        requirement: String,
        kind: io::ErrorKind,
        message: String,
    },
    RegistryPoisoned,
}

impl ServiceFailure {
    fn into_runtime(self) -> RuntimeError {
        match self {
            Self::Exited { requirement, status } => {
                RuntimeError::ServiceExited { requirement, status }
            }
            Self::Status { requirement, kind, message } => RuntimeError::ServiceStatus {
                requirement,
                source: io::Error::new(kind, message),
            },
            Self::RegistryPoisoned => RuntimeError::MonitorRegistryPoisoned,
        }
    }
}

/// A single watcher per running application, rather than per managed child.
/// It never holds the process-registry lock while sleeping or notifying clients.
pub(crate) struct ServiceMonitor {
    processes: Arc<Mutex<ProcessRegistry>>,
    managed: BTreeMap<String, ProcessKey>,
    failure: Arc<Mutex<Option<ServiceFailure>>>,
    cancellation: CancellationToken,
    stop: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<()>>,
}

impl ServiceMonitor {
    pub(crate) fn start(
        processes: Arc<Mutex<ProcessRegistry>>,
        managed: BTreeMap<String, ProcessKey>,
    ) -> io::Result<Self> {
        let failure = Arc::new(Mutex::new(None));
        let cancellation = CancellationToken::default();
        let (stop, worker) = if managed.is_empty() {
            (None, None)
        } else {
            let processes_for_worker = Arc::clone(&processes);
            let managed_for_worker = managed.clone();
            let failure_for_worker = Arc::clone(&failure);
            let cancellation_for_worker = cancellation.clone();
            let (stop, signal) = mpsc::channel();
            let worker = thread::Builder::new()
                .name("riesel-service-monitor".into())
                .spawn(move || loop {
                    if let Some(observed) =
                        Self::scan(&processes_for_worker, &managed_for_worker)
                    {
                        Self::record(&failure_for_worker, &cancellation_for_worker, observed);
                        return;
                    }
                    match signal.recv_timeout(MONITOR_INTERVAL) {
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    }
                })?;
            (Some(stop), Some(worker))
        };
        Ok(Self {
            processes,
            managed,
            failure,
            cancellation,
            stop,
            worker,
        })
    }

    fn scan(
        processes: &Mutex<ProcessRegistry>,
        managed: &BTreeMap<String, ProcessKey>,
    ) -> Option<ServiceFailure> {
        let Ok(mut registry) = processes.lock() else {
            return Some(ServiceFailure::RegistryPoisoned);
        };
        for (requirement, key) in managed {
            match registry.status(key) {
                Ok(ProcessStatus::Running) => {}
                Ok(ProcessStatus::Exited(status)) => {
                    return Some(ServiceFailure::Exited {
                        requirement: requirement.clone(),
                        status,
                    });
                }
                Err(error) => {
                    return Some(ServiceFailure::Status {
                        requirement: requirement.clone(),
                        kind: error.kind(),
                        message: error.to_string(),
                    });
                }
            }
        }
        None
    }

    fn record(
        first: &Mutex<Option<ServiceFailure>>,
        cancellation: &CancellationToken,
        observed: ServiceFailure,
    ) {
        let mut guard = first.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.is_none() {
            *guard = Some(observed);
            cancellation.cancel();
        }
    }

    pub(crate) fn check_now(&self) -> Option<RuntimeError> {
        if let Some(failure) = self
            .failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
        {
            return Some(failure.into_runtime());
        }
        if let Some(observed) = Self::scan(&self.processes, &self.managed) {
            Self::record(&self.failure, &self.cancellation, observed);
        }
        self.failure
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .map(ServiceFailure::into_runtime)
    }

    pub(crate) fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }

    /// Join the watcher *before* terminating owned children, so intentional
    /// shutdown is never mistaken for an unexpected service crash.
    pub(crate) fn stop(&mut self) -> Result<(), RuntimeError> {
        self.stop.take();
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| RuntimeError::MonitorPanicked)?;
        }
        self.cancellation.cancel();
        Ok(())
    }
}

impl Drop for ServiceMonitor {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_waiters_are_notified_and_late_waiters_return_immediately() {
        let token = CancellationToken::default();
        assert!(!token.wait_cancelled_timeout(Duration::from_millis(1)));
        let subscriber = token.clone();
        let waiter = thread::spawn(move || subscriber.wait_cancelled_timeout(Duration::from_secs(2)));
        token.cancel();
        assert!(waiter.join().unwrap());
        assert!(token.wait_cancelled_timeout(Duration::from_secs(1)));
    }

    #[test]
    fn no_managed_services_do_not_trigger_cancellation() {
        let registry = Arc::new(Mutex::new(ProcessRegistry::new()));
        let mut supervisor = ServiceMonitor::start(registry, BTreeMap::new()).unwrap();
        assert!(supervisor.check_now().is_none());
        assert!(!supervisor.cancellation_token().is_cancelled());
        supervisor.stop().unwrap();
        assert!(supervisor.cancellation_token().is_cancelled());
    }
}
