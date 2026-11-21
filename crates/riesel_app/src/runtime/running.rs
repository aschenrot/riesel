use super::*;

/// Active application with verified services and owned child processes.
/// ProcessRegistry guarantees best-effort cleanup if startup or execution fails.
pub struct RunningApp {
    pub(super) app: App,
    pub(super) resolved: Vec<ResolvedCapability>,
    pub(super) services: BTreeMap<String, Arc<dyn Any + Send + Sync>>,
    pub(super) processes: Arc<Mutex<ProcessRegistry>>,
    pub(super) supervisor: ServiceMonitor,
    pub(super) tasks: TaskGroup,
    #[cfg(feature = "async-tcp")]
    pub(super) async_tasks: AsyncTasks,
    #[cfg(feature = "async-tcp")]
    pub(super) graph_admission: Arc<crate::execution_runner::SessionGraphAdmission>,
    #[cfg(feature = "async-tcp")]
    pub(super) local_blocking: Vec<ProviderBlocking>,
    pub(super) session: SessionToken,
}

impl RunningApp {
    pub fn app(&self) -> &App {
        &self.app
    }

    pub fn resolved(&self) -> &[ResolvedCapability] {
        &self.resolved
    }

    /// Check for already-observed failures and immediately inspect running
    /// managed processes. Failure remains observable until shutdown.
    pub fn check_processes(&self) -> Result<(), RuntimeError> {
        if let Some(failure) = self.supervisor.check_now() {
            return Err(failure);
        }
        if let Some(failure) = self.tasks.failure() {
            return Err(failure);
        }
        #[cfg(feature = "async-tcp")]
        if let Some(failure) = self.async_tasks.failure() {
            return Err(failure);
        }
        Ok(())
    }

    /// The revocable session identity shared by every capability handle.
    /// Managed-child failure and shutdown invalidate all of its clones.
    pub fn session_token(&self) -> SessionToken {
        self.session.clone()
    }

    /// Start an application-owned task. The task receives cooperative
    /// cancellation and is always joined before providers are stopped.
    /// Task errors and panics cancel the session and remain observable.
    pub fn spawn_task(
        &self,
        name: impl Into<String>,
        task: impl FnOnce(CancellationToken) -> Result<(), ConnectorError> + Send + 'static,
    ) -> Result<(), RuntimeError> {
        self.tasks.spawn(name.into(), task)
    }

    /// Obtain the session-owned async task spawner. Task admission is
    /// possible only inside the application's active async handler scope.
    #[cfg(feature = "async-tcp")]
    pub fn async_tasks(&self) -> AsyncTasks {
        self.async_tasks.clone()
    }

    /// Share a cooperative cancellation token with long-running application
    /// work. When a required managed service exits, the token becomes set.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.supervisor.cancellation_token()
    }

    /// Join monitoring first, then drop clients and stop owned children in
    /// reverse startup order. External TCP services remain untouched.
    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
        // Tasks receive cancellation before providers are stopped. Keep the
        // supervisor running during task joins: a managed child can still
        // exit unexpectedly while application-owned work is finishing.
        // Revoke capability handles before joining application tasks.
        // Pending async I/O must be cancelled promptly during shutdown.
        self.session.invalidate();
        #[cfg(feature = "async-tcp")]
        self.async_tasks.close_for_shutdown();
        let tasks = self.tasks.join();
        // The provider's own OS-thread workers are never detached from
        // abandoned RPC futures. Join before releasing local provider state.
        #[cfg(feature = "async-tcp")]
        let provider_failures: Vec<_> = self
            .local_blocking
            .drain(..)
            .filter_map(|blocking| {
                blocking
                    .close_and_join()
                    .err()
                    .map(|source| RuntimeError::ProviderBlockingShutdown { source })
            })
            .collect();
        // The monitor takes its final snapshot only after every task joined,
        // but before intentional managed-process teardown.
        let monitor = self.supervisor.stop();
        self.services.clear();
        let stopped = self
            .processes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_all()
            .map_err(RuntimeError::Shutdown);
        let mut failures = Vec::new();
        for result in [monitor, tasks, stopped] {
            if let Err(failure) = result {
                failures.push(failure);
            }
        }
        #[cfg(feature = "async-tcp")]
        if let Some(failure) = self.async_tasks.failure() {
            failures.push(failure);
        }
        #[cfg(feature = "async-tcp")]
        failures.extend(provider_failures);
        match failures.len() {
            0 => Ok(()),
            1 => Err(failures.remove(0)),
            _ => Err(RuntimeError::ShutdownFailures { errors: failures }),
        }
    }

    /// Get a typed, already-verified service by its application requirement name.
    /// For shared Rust trait interfaces, request e.g. `Arc<dyn RecordStore>`.
    pub fn service<T: Any + Send + Sync>(&self, requirement: &str) -> Result<&T, RuntimeError> {
        self.services
            .get(requirement)
            .ok_or_else(|| RuntimeError::MissingService(requirement.into()))?
            .downcast_ref::<T>()
            .ok_or_else(|| RuntimeError::WrongServiceType(requirement.into()))
    }
}

impl Drop for RunningApp {
    fn drop(&mut self) {
        // An explicitly started application must also clean up when it is
        // dropped without a corresponding run() or shutdown() call.
        let _ = self.shutdown();
    }
}

impl RunningApp {
    pub(crate) fn execute(
        mut running: RunningApp,
        entrypoint: impl FnOnce(&RunningApp) -> Result<(), ConnectorError>,
    ) -> Result<(), AppRunError> {
        let outcome = entrypoint(&running);
        // Shutdown joins the watcher and snapshots the final child status
        // before terminating children, avoiding a check/teardown race.
        let shutdown = running.shutdown().err();
        let failure = running.supervisor.observed_failure();
        let task_failure = running.tasks.failure();
        #[cfg(feature = "async-tcp")]
        let task_failure = task_failure.or_else(|| running.async_tasks.failure());

        if let Some(failure) = failure {
            return Err(AppRunError::Supervision {
                failure: Box::new(failure),
                entry: outcome.err(),
                shutdown: shutdown.map(Box::new),
            });
        }
        if let Some(failure) = task_failure {
            return Err(AppRunError::TaskFailure {
                failure: Box::new(failure),
                entry: outcome.err(),
                // The primary task error has its own field; do not duplicate
                // that same error as a secondary shutdown failure.
                shutdown: shutdown
                    .filter(|error| !matches!(error, RuntimeError::TaskFailed { .. }))
                    .map(Box::new),
            });
        }
        if outcome
            .as_ref()
            .is_err_and(|error| error.is::<LifecycleFailure>())
        {
            return Err(AppRunError::Lifecycle {
                failure: outcome.expect_err("lifecycle failure was checked"),
                shutdown: shutdown.map(Box::new),
            });
        }
        match (outcome, shutdown) {
            (Ok(()), None) => Ok(()),
            (Err(entry), None) => Err(AppRunError::Entrypoint(entry)),
            (Ok(()), Some(shutdown)) => Err(AppRunError::Shutdown(shutdown)),
            (Err(entry), Some(shutdown)) => {
                Err(AppRunError::EntrypointAndShutdown { entry, shutdown })
            }
        }
    }
}
