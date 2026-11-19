//! Startup execution modes share one validated provider/session state machine.
use super::bootstrap::{StartupState, StartupStep};
use super::*;

impl App {
    /// Start a validated application synchronously. This remains the low-level
    /// path for traditional clients; the async application SDK uses
    /// start_async instead of blocking its event loop during inspection.
    pub fn start(
        self,
        deployment: &DeploymentConfig,
        connectors: &ConnectorRegistry,
    ) -> Result<RunningApp, RuntimeError> {
        let mut startup = StartupState::new(self, deployment, connectors)?;
        for index in 0..startup.len() {
            let remote = match startup.prepare(index)? {
                StartupStep::Local => continue,
                StartupStep::Remote(remote) => remote,
                StartupStep::Managed(pending) => startup.spawn_managed_sync(pending)?,
            };
            let connected = startup
                .connector(&remote.requirement)?
                .connector
                .connect(&remote.endpoint, &startup.session)
                .map_err(|source| RuntimeError::Connection {
                    requirement: remote.requirement.name.clone(),
                    source,
                })?;
            startup.accept_remote(remote, connected)?;
        }
        startup.finish()
    }

    /// Full async connector inspection through the same resolver as start().
    /// Managed process spawn and readiness are isolated to a blocking worker,
    /// while metadata TCP I/O awaits Tokio nonblocking sockets. The caller
    /// owns the executor: no nested runtime or application-level wire logic.
    #[cfg(feature = "async-tcp")]
    pub async fn start_async(
        self,
        deployment: &DeploymentConfig,
        connectors: &ConnectorRegistry,
    ) -> Result<RunningApp, RuntimeError> {
        let mut startup = StartupState::new(self, deployment, connectors)?;
        for index in 0..startup.len() {
            let remote = match startup.prepare(index)? {
                StartupStep::Local => continue,
                StartupStep::Remote(remote) => remote,
                StartupStep::Managed(pending) => startup.spawn_managed_async(pending).await?,
            };
            let connected = startup
                .connector(&remote.requirement)?
                .connector
                .connect_async(&remote.endpoint, &startup.session)
                .await
                .map_err(|source| RuntimeError::Connection {
                    requirement: remote.requirement.name.clone(),
                    source,
                })?;
            startup.accept_remote(remote, connected)?;
        }
        startup.finish()
    }

    /// Run application logic with services available, then shut down owned
    /// processes whether the entrypoint succeeds or returns an error.
    pub fn run(
        self,
        deployment: &DeploymentConfig,
        connectors: &ConnectorRegistry,
        entrypoint: impl FnMut(&RunningApp) -> Result<(), ConnectorError>,
    ) -> Result<(), AppRunError> {
        run_with_recovery(
            &deployment.recovery,
            || self.clone().start(deployment, connectors),
            entrypoint,
        )
    }

    /// The high-level async entrypoint is synchronous to call, but each
    /// startup session uses an owned Tokio executor for real async I/O and
    /// managed readiness work; the business entrypoint uses its separate
    /// session-owned executor after startup completes.
    #[cfg(feature = "async-tcp")]
    pub fn run_with_async_startup(
        self,
        deployment: &DeploymentConfig,
        connectors: &ConnectorRegistry,
        entrypoint: impl FnMut(&RunningApp) -> Result<(), ConnectorError>,
    ) -> Result<(), AppRunError> {
        let startup_executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|source| RuntimeError::StartupExecutor(source))?;
        run_with_recovery(
            &deployment.recovery,
            || startup_executor.block_on(self.clone().start_async(deployment, connectors)),
            entrypoint,
        )
    }
}
