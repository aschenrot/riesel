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
    #[error("could not launch managed service monitor: {0}")]
    MonitorStart(io::Error),
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
    #[error("application task limit exceeded (maximum {limit} concurrent tasks)")]
    TaskLimitExceeded { limit: usize },
    #[error("application task {name} failed: {message}")]
    TaskFailed { name: String, message: String },
    #[error("multiple application shutdown failures: {errors:?}")]
    ShutdownFailures { errors: Vec<RuntimeError> },
}

/// Errors from running application business logic and shutting down its services.
#[derive(Debug, Error)]
pub enum AppRunError {
    #[error("application startup failed: {0}")]
    Startup(#[from] RuntimeError),
    #[error("application entrypoint failed: {0}")]
    Entrypoint(ConnectorError),
    #[error("application entrypoint failed: {entry}; shutdown also failed: {shutdown}")]
    EntrypointAndShutdown {
        entry: ConnectorError,
        shutdown: RuntimeError,
    },
    #[error(
        "required managed service failed: {failure}; entrypoint: {entry:?}; shutdown: {shutdown:?}"
    )]
    Supervision {
        failure: Box<RuntimeError>,
        entry: Option<ConnectorError>,
        shutdown: Option<Box<RuntimeError>>,
    },
    #[error("application recovery exhausted after {restarts} restarts: {last}")]
    RecoveryExhausted {
        restarts: u32,
        last: Box<AppRunError>,
    },
    #[error(
        "application background task failed: {failure}; entrypoint: {entry:?}; shutdown: {shutdown:?}"
    )]
    TaskFailure {
        failure: Box<RuntimeError>,
        entry: Option<ConnectorError>,
        shutdown: Option<Box<RuntimeError>>,
    },
}
