use super::*;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("invalid deployment: {0}")]
    Deployment(#[from] DeploymentError),
    #[error("capability resolution failed: {0}")]
    Resolution(#[from] ResolutionError),
    #[error("cannot obtain local connector contract: {source}")]
    ConnectorContract {
        #[source]
        source: ConnectorError,
    },
    #[error("invalid capability contract: {source}")]
    InvalidContract {
        #[source]
        source: riesel_schema::ContractError,
    },
    #[error("connector already registered for capability: {id}@{version}")]
    DuplicateConnector { id: String, version: u32 },
    #[error("no connector registered for {id}@{version} required as {requirement}")]
    MissingConnector {
        requirement: String,
        id: String,
        version: u32,
    },
    #[error("connector for {requirement} exposes a different local API or version")]
    LocalContractMismatch { requirement: String },
    #[error(
        "requirement {requirement} uses an async transport without a compatible local async binding"
    )]
    UnsupportedLocalBinding { requirement: String },
    #[error("connector for requirement {requirement} cannot perform nonblocking async startup")]
    UnsupportedAsyncStartup { requirement: String },
    #[error("connecting requirement {requirement} failed: {source}")]
    Connection {
        requirement: String,
        #[source]
        source: ConnectorError,
    },
    #[error("provider for {requirement} advertised an invalid definition: {source}")]
    InvalidProvider {
        requirement: String,
        #[source]
        source: riesel_schema::ProviderError,
    },
    #[error("provider for {requirement} advertises a different API or capability")]
    RemoteContractMismatch { requirement: String },
    #[error("provider ID {provider} advertises inconsistent definitions")]
    ConflictingProvider { provider: String },
    #[error(
        "provider identity mismatch for requirement {requirement}: expected {expected:?}, received {actual:?}"
    )]
    ProviderIdentityMismatch {
        requirement: String,
        expected: Box<ProviderDefinition>,
        actual: Box<ProviderDefinition>,
    },
    #[error("unknown running requirement: {0}")]
    MissingService(String),
    #[error("wrong service type for requirement: {0}")]
    WrongServiceType(String),
    #[error("local provider factory failed: {source}")]
    ProviderFactory {
        #[source]
        source: ConnectorError,
    },
    #[error("duplicate local provider: {0}")]
    DuplicateLocalProvider(String),
    #[error("provider {0} does not advertise its returned contract")]
    ProviderContractMismatch(String),
    #[error("no registered local provider implements {id}@{version} (requirement {requirement})")]
    NoLocalProvider {
        requirement: String,
        id: String,
        version: u32,
    },
    #[error("multiple local providers implement requirement {requirement}: {providers:?}")]
    AmbiguousLocalProvider {
        requirement: String,
        providers: Vec<String>,
    },
    #[error("record Query model {model} requires trusted linked placement for {requirement}")]
    UnsupportedLinkedRecordPlacement { model: String, requirement: String },
    #[error(
        "record Query model {model} links reader {reader} and writer {writer} to different providers"
    )]
    LinkedRecordOwnerMismatch {
        model: String,
        reader: String,
        writer: String,
    },
    #[error("record Query model {model} reader {requirement} must advertise read-only operations")]
    InvalidLinkedRecordReader { model: String, requirement: String },
    #[error("record Query model {model} writer {requirement} must advertise mutation operations")]
    InvalidLinkedRecordWriter { model: String, requirement: String },
    #[error("record Query model {model} lost its actual linked owner for {requirement}")]
    LinkedRecordRuntimeMismatch { model: String, requirement: String },
    #[error("unregistered local provider: {0}")]
    UnknownLocalProvider(String),
    #[error("local provider {0} returned different metadata from its registration")]
    LocalProviderChanged(String),
    #[error("could not start managed service {requirement}: {source}")]
    ServiceStart {
        requirement: String,
        #[source]
        source: io::Error,
    },
    #[error("managed service {requirement} exited: {status}")]
    ServiceExited {
        requirement: String,
        status: ExitStatus,
    },
    #[error("managed service {requirement} did not become ready within {timeout_ms} ms")]
    ReadinessTimeout {
        requirement: String,
        timeout_ms: u64,
    },
    #[error("cannot inspect managed service {requirement}: {source}")]
    ServiceStatus {
        requirement: String,
        #[source]
        source: io::Error,
    },
    #[error("failed to shut down managed processes: {0}")]
    Shutdown(io::Error),
    #[error("startup failed: {startup}; rollback also failed: {cleanup}")]
    StartupRollback {
        #[source]
        startup: Box<RuntimeError>,
        cleanup: Box<RuntimeError>,
    },
    #[cfg(feature = "async-tcp")]
    #[error("provider-owned blocking work failed to join cleanly: {source}")]
    ProviderBlockingShutdown {
        #[source]
        source: io::Error,
    },
    #[error("could not launch managed service monitor: {0}")]
    MonitorStart(io::Error),
    #[error("could not construct asynchronous startup executor: {0}")]
    StartupExecutor(io::Error),
    #[error("managed service monitor thread panicked")]
    MonitorPanicked,
    #[error("managed service registry was poisoned during monitoring")]
    MonitorRegistryPoisoned,
    #[error("cannot start application task {name}: {source}")]
    TaskStart {
        name: String,
        #[source]
        source: io::Error,
    },
    #[error("invalid or duplicate application task name: {0}")]
    InvalidTaskName(String),
    #[error("application task group is already closing")]
    TaskGroupClosed,
    #[cfg(feature = "async-tcp")]
    #[error("background graph runner belongs to a different application session")]
    GraphSessionMismatch,
    #[error("application task limit exceeded (maximum {limit} concurrent tasks)")]
    TaskLimitExceeded { limit: usize },
    #[error("application task {name} failed: {source}")]
    TaskFailed {
        name: String,
        #[source]
        source: Arc<dyn std::error::Error + Send + Sync>,
    },
    #[error("multiple application shutdown failures: {errors:?}")]
    ShutdownFailures { errors: Vec<RuntimeError> },
}

/// Whether a named application lifecycle hook runs before or after the entrypoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPhase {
    Start,
    Stop,
}

impl std::fmt::Display for HookPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Start => "start",
            Self::Stop => "stop",
        })
    }
}

/// A hook failure preserves its phase, name and original typed error source.
#[derive(Debug, Error)]
#[error("{phase} hook {name} failed: {source}")]
pub struct HookFailure {
    pub phase: HookPhase,
    pub name: String,
    #[source]
    pub source: ConnectorError,
}

/// Original entrypoint failure and any hook cleanup failures are kept together.
#[derive(Debug)]
pub struct LifecycleFailure {
    pub entry: Option<ConnectorError>,
    pub hooks: Vec<HookFailure>,
}

impl std::fmt::Display for LifecycleFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "application lifecycle failed")?;
        if let Some(entry) = &self.entry {
            write!(f, "; entrypoint: {entry}")?;
        }
        for hook in &self.hooks {
            write!(f, "; {hook}")?;
        }
        Ok(())
    }
}

impl std::error::Error for LifecycleFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.entry
            .as_ref()
            .map(|source| source.as_ref() as &(dyn std::error::Error + 'static))
            .or_else(|| {
                self.hooks
                    .first()
                    .map(|failure| failure as &(dyn std::error::Error + 'static))
            })
    }
}

/// Errors from running application business logic and shutting down its services.
#[derive(Debug, Error)]
pub enum AppRunError {
    #[error("application startup failed: {0}")]
    Startup(#[from] RuntimeError),
    #[error("application entrypoint failed: {0}")]
    Entrypoint(#[source] ConnectorError),
    #[error("application shutdown failed: {0}")]
    Shutdown(#[source] RuntimeError),
    #[error("application lifecycle failed: {failure}; shutdown: {shutdown:?}")]
    Lifecycle {
        #[source]
        failure: ConnectorError,
        shutdown: Option<Box<RuntimeError>>,
    },
    #[error("application entrypoint failed: {entry}; shutdown also failed: {shutdown}")]
    EntrypointAndShutdown {
        #[source]
        entry: ConnectorError,
        shutdown: RuntimeError,
    },
    #[error(
        "required managed service failed: {failure}; entrypoint: {entry:?}; shutdown: {shutdown:?}"
    )]
    Supervision {
        #[source]
        failure: Box<RuntimeError>,
        entry: Option<ConnectorError>,
        shutdown: Option<Box<RuntimeError>>,
    },
    #[error("application recovery exhausted after {restarts} restarts: {last}")]
    RecoveryExhausted {
        restarts: u32,
        #[source]
        last: Box<AppRunError>,
    },
    #[error(
        "application background task failed: {failure}; entrypoint: {entry:?}; shutdown: {shutdown:?}"
    )]
    TaskFailure {
        #[source]
        failure: Box<RuntimeError>,
        entry: Option<ConnectorError>,
        shutdown: Option<Box<RuntimeError>>,
    },
}
