//! Provider-agnostic application composition.
//! Connectors implement transport-specific discovery and typed service creation;
//! this module owns validation, resolution, and the resulting service lifetimes.

use crate::plan::{DeploymentPlan, PlannedService, PlannedTarget};
use crate::supervisor::{CancellationToken, ServiceMonitor};
use crate::{
    App, DeploymentConfig, DeploymentError, ProviderRegistry, ResolutionError, ResolvedCapability,
    ServiceEndpoint,
};
use riesel_manager::{ProcessKey, ProcessRegistry};
use riesel_schema::{CapabilityContract, ProviderDefinition};
use serde::Deserialize;
use std::any::Any;
use std::collections::BTreeMap;
use std::error::Error;
use std::io::{self, BufRead, BufReader, Read};
use std::net::SocketAddr;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::Duration;
use thiserror::Error;

pub type ConnectorError = Box<dyn Error + Send + Sync>;

const MAX_READY_BYTES: u64 = 4 * 1024;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ShutdownMode {
    StdinLine,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ServiceReady {
    protocol: u32,
    address: SocketAddr,
    #[serde(default)]
    shutdown: Option<ShutdownMode>,
}

/// Read a single bounded startup frame from a private child stdout pipe.
/// Timeout kills the child through ProcessRegistry's drop cleanup.
fn await_ready<R: Read + Send + 'static>(stdout: R, timeout: Duration) -> io::Result<ServiceReady> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let result = (|| {
            let mut limited = (&mut reader).take(MAX_READY_BYTES + 1);
            let mut message = Vec::new();
            limited.read_until(b'\n', &mut message)?;
            if message.last() != Some(&b'\n') || message.len() as u64 > MAX_READY_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "missing or oversized RIESEL_READY announcement",
                ));
            }
            let line = std::str::from_utf8(&message)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            let payload = line.strip_prefix("RIESEL_READY ").ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid startup announcement prefix",
                )
            })?;
            let ready: ServiceReady = serde_json::from_str(payload)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
            if ready.protocol != 1 || !ready.address.ip().is_loopback() || ready.address.port() == 0
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unsupported or non-loopback ready endpoint",
                ));
            }
            Ok(ready)
        })();
        let ready = result.is_ok();
        // Keep stdout alive after readiness so subsequent service logs do not
        // trigger a broken pipe. Drain it for the lifetime of the child.
        if sender.send(result).is_ok() && ready {
            let _ = io::copy(&mut reader, &mut io::stderr());
        }
    });
    receiver.recv_timeout(timeout).map_err(|error| {
        let kind = match error {
            mpsc::RecvTimeoutError::Timeout => io::ErrorKind::TimedOut,
            mpsc::RecvTimeoutError::Disconnected => io::ErrorKind::BrokenPipe,
        };
        io::Error::new(kind, "waiting for managed service readiness")
    })?
}

/// An inspected, connected service. Each connector owns its transport-specific
/// adapter. The boxed Rust value is retrieved by the consumer using its type.
pub struct ConnectedProvider {
    pub definition: ProviderDefinition,
    pub contract: CapabilityContract,
    pub service: Box<dyn Any + Send + Sync>,
}

/// Bridge from a declared capability to one implementation-specific transport.
/// A connector is registered by its local Rust-generated capability contract.
pub trait CapabilityConnector: Send + Sync {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError>;
    fn connect(&self, endpoint: &ServiceEndpoint) -> Result<ConnectedProvider, ConnectorError>;
}

struct RegisteredConnector {
    contract: CapabilityContract,
    connector: Box<dyn CapabilityConnector>,
}

/// In-process provider metadata and its implementation factory.
/// Metadata is available without constructing a service or opening a socket.
pub trait LocalProviderFactory: Send + Sync {
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError>;
    fn contract(&self) -> Result<CapabilityContract, ConnectorError>;
    fn create(&self) -> Result<ConnectedProvider, ConnectorError>;
}

struct RegisteredLocalProvider {
    definition: ProviderDefinition,
    contract: CapabilityContract,
    factory: Box<dyn LocalProviderFactory>,
}

/// Explicit runtime adapter registry. Contract IDs (not Rust types or TCP
/// addresses) are the stable lookup keys.
#[derive(Default)]
pub struct ConnectorRegistry {
    adapters: BTreeMap<String, RegisteredConnector>,
    local: BTreeMap<String, RegisteredLocalProvider>,
}

impl ConnectorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider without constructing the implementation. More than
    /// one provider may implement a capability; ambiguous selection is rejected.
    pub fn register_local(
        &mut self,
        factory: impl LocalProviderFactory + 'static,
    ) -> Result<(), RuntimeError> {
        let definition = factory
            .definition()
            .map_err(|source| RuntimeError::ProviderFactory { source })?;
        definition
            .validate()
            .map_err(|source| RuntimeError::InvalidProvider {
                requirement: definition.id.clone(),
                source,
            })?;
        let contract = factory
            .contract()
            .map_err(|source| RuntimeError::ProviderFactory { source })?;
        contract
            .validate()
            .map_err(|source| RuntimeError::InvalidContract { source })?;
        if !definition.capabilities.contains(&contract.capability) {
            return Err(RuntimeError::ProviderContractMismatch(definition.id));
        }
        let id = definition.id.clone();
        if self.local.contains_key(&id) {
            return Err(RuntimeError::DuplicateLocalProvider(id));
        }
        self.local.insert(
            id,
            RegisteredLocalProvider {
                definition,
                contract,
                factory: Box::new(factory),
            },
        );
        Ok(())
    }

    /// Whether a transport adapter has already been registered for this
    /// capability ID. Allows Rust-first builders to avoid duplicate wiring.
    pub fn has_connector(&self, capability_id: &str) -> bool {
        self.adapters.contains_key(capability_id)
    }

    pub fn register(
        &mut self,
        connector: impl CapabilityConnector + 'static,
    ) -> Result<(), RuntimeError> {
        let contract = connector
            .contract()
            .map_err(|source| RuntimeError::ConnectorContract { source })?;
        contract
            .validate()
            .map_err(|source| RuntimeError::InvalidContract { source })?;
        let id = contract.capability.id.clone();
        if self.adapters.contains_key(&id) {
            return Err(RuntimeError::DuplicateConnector(id));
        }
        self.adapters.insert(
            id,
            RegisteredConnector {
                contract,
                connector: Box::new(connector),
            },
        );
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("application has no loaded definition")]
    MissingDefinition,
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
    #[error("connector already registered for capability: {0}")]
    DuplicateConnector(String),
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
}

/// Active application with verified services and owned child processes.
/// ProcessRegistry guarantees best-effort cleanup if startup or execution fails.
pub struct RunningApp {
    app: App,
    resolved: Vec<ResolvedCapability>,
    services: BTreeMap<String, Arc<dyn Any + Send + Sync>>,
    processes: Arc<Mutex<ProcessRegistry>>,
    supervisor: ServiceMonitor,
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
        match self.supervisor.check_now() {
            Some(failure) => Err(failure),
            None => Ok(()),
        }
    }

    /// Share a cooperative cancellation token with long-running application
    /// work. When a required managed service exits, the token becomes set.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.supervisor.cancellation_token()
    }

    /// Join monitoring first, then drop clients and stop owned children in
    /// reverse startup order. External TCP services remain untouched.
    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
        let monitor = self.supervisor.stop();
        self.services.clear();
        let stopped = self
            .processes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stop_all()
            .map_err(RuntimeError::Shutdown);
        monitor?;
        stopped
    }

    /// Get a typed, already-verified service by its application requirement name.
    /// For shared Rust trait interfaces, request e.g. Arc<dyn RecordStore>.
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

impl App {
    /// Inspect the complete deployment without executing provider factories,
    /// starting processes, or connecting to network endpoints. Checks every
    /// requirement in dependency order to catch late configuration errors
    /// before the first component is created.
    pub fn preflight(
        &self,
        deployment: &DeploymentConfig,
        connectors: &ConnectorRegistry,
    ) -> Result<DeploymentPlan, RuntimeError> {
        let schema = self.definition().ok_or(RuntimeError::MissingDefinition)?;
        deployment.validate(schema)?;

        let mut services = Vec::with_capacity(schema.requires.len());
        for requirement in schema.startup_order().map_err(DeploymentError::from)? {
            let endpoint = &deployment.services[&requirement.name];
            let target = match endpoint {
                ServiceEndpoint::Local { provider } => {
                    let local = connectors
                        .local
                        .get(provider)
                        .ok_or_else(|| RuntimeError::UnknownLocalProvider(provider.clone()))?;
                    if !local
                        .definition
                        .capabilities
                        .contains(&requirement.capability)
                        || local.contract.capability != requirement.capability
                    {
                        return Err(RuntimeError::LocalContractMismatch {
                            requirement: requirement.name.clone(),
                        });
                    }
                    PlannedTarget::Local {
                        provider: provider.clone(),
                    }
                }
                ServiceEndpoint::Tcp { address } => {
                    Self::validate_connector(requirement, connectors)?;
                    PlannedTarget::Tcp { address: *address }
                }
                ServiceEndpoint::Managed {
                    program,
                    startup_timeout_ms,
                    shutdown_timeout_ms,
                    ..
                } => {
                    Self::validate_connector(requirement, connectors)?;
                    PlannedTarget::Managed {
                        program: program.clone(),
                        startup_timeout_ms: *startup_timeout_ms,
                        shutdown_timeout_ms: *shutdown_timeout_ms,
                    }
                }
            };
            services.push(PlannedService {
                requirement: requirement.name.clone(),
                capability: requirement.capability.clone(),
                depends_on: requirement.depends_on.clone(),
                target,
            });
        }
        Ok(DeploymentPlan { services })
    }

    fn validate_connector(
        requirement: &riesel_schema::CapabilityRequirement,
        connectors: &ConnectorRegistry,
    ) -> Result<(), RuntimeError> {
        let id = &requirement.capability.id;
        let adapter =
            connectors
                .adapters
                .get(id)
                .ok_or_else(|| RuntimeError::MissingConnector {
                    requirement: requirement.name.clone(),
                    id: id.clone(),
                    version: requirement.capability.version,
                })?;
        if adapter.contract.capability != requirement.capability {
            return Err(RuntimeError::LocalContractMismatch {
                requirement: requirement.name.clone(),
            });
        }
        Ok(())
    }

    /// Automatically select local implementations for every declared
    /// capability. This does not allocate sockets or start subprocesses.
    /// Explicit selection remains possible using ServiceEndpoint::Local.
    pub fn start_local(self, connectors: &ConnectorRegistry) -> Result<RunningApp, RuntimeError> {
        let schema = self.definition().ok_or(RuntimeError::MissingDefinition)?;
        let mut services = BTreeMap::new();
        for requirement in &schema.requires {
            let compatible: Vec<String> = connectors
                .local
                .values()
                .filter(|local| {
                    local
                        .definition
                        .capabilities
                        .contains(&requirement.capability)
                        && local.contract.capability == requirement.capability
                })
                .map(|local| local.definition.id.clone())
                .collect();
            let provider = match compatible.as_slice() {
                [] => {
                    return Err(RuntimeError::NoLocalProvider {
                        requirement: requirement.name.clone(),
                        id: requirement.capability.id.clone(),
                        version: requirement.capability.version,
                    });
                }
                [only] => only.clone(),
                _ => {
                    return Err(RuntimeError::AmbiguousLocalProvider {
                        requirement: requirement.name.clone(),
                        providers: compatible,
                    });
                }
            };
            services.insert(
                requirement.name.clone(),
                ServiceEndpoint::Local { provider },
            );
        }
        self.start(&DeploymentConfig { services }, connectors)
    }

    /// Run an application with local providers and guaranteed cleanup.
    pub fn run_local(
        self,
        connectors: &ConnectorRegistry,
        entrypoint: impl FnOnce(&RunningApp) -> Result<(), ConnectorError>,
    ) -> Result<(), AppRunError> {
        let running = self.start_local(connectors)?;
        RunningApp::execute(running, entrypoint)
    }

    /// Start a validated application. For managed endpoints, spawn the
    /// configured trusted executable and wait for a valid provider handshake.
    /// On any startup failure, already spawned children are stopped on drop.
    pub fn start(
        self,
        deployment: &DeploymentConfig,
        connectors: &ConnectorRegistry,
    ) -> Result<RunningApp, RuntimeError> {
        let _validated_plan = self.preflight(deployment, connectors)?;
        let schema = self.definition().ok_or(RuntimeError::MissingDefinition)?;

        let mut registry = ProviderRegistry::new();
        let mut provider_definitions = BTreeMap::new();
        let mut selected = BTreeMap::new();
        let mut services = BTreeMap::new();
        let mut local_instances: BTreeMap<String, Arc<dyn Any + Send + Sync>> = BTreeMap::new();
        let mut processes = ProcessRegistry::new();
        let mut managed = BTreeMap::new();

        for (index, requirement) in schema
            .startup_order()
            .map_err(DeploymentError::from)?
            .into_iter()
            .enumerate()
        {
            let endpoint = &deployment.services[&requirement.name];
            if let ServiceEndpoint::Local { provider } = endpoint {
                let local = connectors
                    .local
                    .get(provider)
                    .ok_or_else(|| RuntimeError::UnknownLocalProvider(provider.clone()))?;
                if !local
                    .definition
                    .capabilities
                    .contains(&requirement.capability)
                    || local.contract.capability != requirement.capability
                {
                    return Err(RuntimeError::LocalContractMismatch {
                        requirement: requirement.name.clone(),
                    });
                }
                let instance = if let Some(existing) = local_instances.get(provider) {
                    Arc::clone(existing)
                } else {
                    let connected =
                        local
                            .factory
                            .create()
                            .map_err(|source| RuntimeError::Connection {
                                requirement: requirement.name.clone(),
                                source,
                            })?;
                    if connected.definition != local.definition
                        || !connected.contract.has_same_api_shape(&local.contract)
                    {
                        return Err(RuntimeError::LocalProviderChanged(provider.clone()));
                    }
                    registry.register(connected.definition.clone())?;
                    provider_definitions.insert(provider.clone(), connected.definition);
                    let instance: Arc<dyn Any + Send + Sync> = connected.service.into();
                    local_instances.insert(provider.clone(), Arc::clone(&instance));
                    instance
                };
                selected.insert(requirement.name.clone(), provider.clone());
                services.insert(requirement.name.clone(), instance);
                continue;
            }
            let id = &requirement.capability.id;
            let adapter =
                connectors
                    .adapters
                    .get(id)
                    .ok_or_else(|| RuntimeError::MissingConnector {
                        requirement: requirement.name.clone(),
                        id: id.clone(),
                        version: requirement.capability.version,
                    })?;

            if adapter.contract.capability != requirement.capability {
                return Err(RuntimeError::LocalContractMismatch {
                    requirement: requirement.name.clone(),
                });
            }
            let connected = match endpoint {
                ServiceEndpoint::Local { .. } => unreachable!("local endpoint handled above"),
                ServiceEndpoint::Managed {
                    program,
                    args,
                    startup_timeout_ms,
                    shutdown_timeout_ms,
                } => {
                    let key = ProcessKey::new(index as u64 + 1, Some(requirement.name.clone()));
                    let mut command = Command::new(program);
                    command
                        .args(args)
                        .env("RIESEL_BIND_ADDRESS", "127.0.0.1:0")
                        .env("RIESEL_CONTROL_STDIN", "1")
                        .stdin(Stdio::piped())
                        .stdout(Stdio::piped());
                    processes
                        .spawn(key.clone(), &mut command)
                        .map_err(|source| RuntimeError::ServiceStart {
                            requirement: requirement.name.clone(),
                            source,
                        })?;

                    let stdout = processes.take_stdout(&key).map_err(|source| {
                        RuntimeError::ServiceStart {
                            requirement: requirement.name.clone(),
                            source,
                        }
                    })?;
                    let ready =
                        match await_ready(stdout, Duration::from_millis(*startup_timeout_ms)) {
                            Ok(ready) => ready,
                            Err(source) if source.kind() == io::ErrorKind::TimedOut => {
                                return Err(RuntimeError::ReadinessTimeout {
                                    requirement: requirement.name.clone(),
                                    timeout_ms: *startup_timeout_ms,
                                });
                            }
                            Err(source) => {
                                return Err(RuntimeError::ServiceStart {
                                    requirement: requirement.name.clone(),
                                    source,
                                });
                            }
                        };

                    if matches!(ready.shutdown, Some(ShutdownMode::StdinLine)) {
                        processes
                            .enable_graceful_shutdown(
                                &key,
                                Duration::from_millis(*shutdown_timeout_ms),
                            )
                            .map_err(|source| RuntimeError::ServiceStart {
                                requirement: requirement.name.clone(),
                                source,
                            })?;
                    }

                    // Connector still verifies the provider ID and exact Rust
                    // contract after the startup channel reports readiness.
                    let endpoint = ServiceEndpoint::Tcp {
                        address: ready.address,
                    };
                    let connection = adapter.connector.connect(&endpoint).map_err(|source| {
                        RuntimeError::Connection {
                            requirement: requirement.name.clone(),
                            source,
                        }
                    })?;
                    managed.insert(requirement.name.clone(), key);
                    connection
                }
                ServiceEndpoint::Tcp { .. } => {
                    adapter.connector.connect(endpoint).map_err(|source| {
                        RuntimeError::Connection {
                            requirement: requirement.name.clone(),
                            source,
                        }
                    })?
                }
            };

            connected
                .definition
                .validate()
                .map_err(|source| RuntimeError::InvalidProvider {
                    requirement: requirement.name.clone(),
                    source,
                })?;
            connected
                .contract
                .validate()
                .map_err(|source| RuntimeError::InvalidContract { source })?;
            if !connected
                .definition
                .capabilities
                .contains(&requirement.capability)
                || !connected.contract.has_same_api_shape(&adapter.contract)
            {
                return Err(RuntimeError::RemoteContractMismatch {
                    requirement: requirement.name.clone(),
                });
            }

            let provider_id = connected.definition.id.clone();
            if let Some(registered) = provider_definitions.get(&provider_id) {
                if registered != &connected.definition {
                    return Err(RuntimeError::ConflictingProvider {
                        provider: provider_id,
                    });
                }
            } else {
                registry.register(connected.definition.clone())?;
                provider_definitions.insert(provider_id.clone(), connected.definition);
            }

            selected.insert(requirement.name.clone(), provider_id);
            services.insert(requirement.name.clone(), connected.service.into());
        }

        let resolved = registry.resolve(schema, &selected)?;
        let processes = Arc::new(Mutex::new(processes));
        let supervisor = ServiceMonitor::start(Arc::clone(&processes), managed)
            .map_err(RuntimeError::MonitorStart)?;
        Ok(RunningApp {
            app: self,
            resolved,
            services,
            processes,
            supervisor,
        })
    }

    /// Run application logic with services available, then shut down owned
    /// processes whether the entrypoint succeeds or returns an error.
    pub fn run(
        self,
        deployment: &DeploymentConfig,
        connectors: &ConnectorRegistry,
        entrypoint: impl FnOnce(&RunningApp) -> Result<(), ConnectorError>,
    ) -> Result<(), AppRunError> {
        let running = self.start(deployment, connectors)?;
        RunningApp::execute(running, entrypoint)
    }
}

impl RunningApp {
    fn execute(
        mut running: RunningApp,
        entrypoint: impl FnOnce(&RunningApp) -> Result<(), ConnectorError>,
    ) -> Result<(), AppRunError> {
        let outcome = entrypoint(&running);
        // Shutdown joins the watcher and snapshots the final child status
        // before terminating children, avoiding a check/teardown race.
        let shutdown = running.shutdown().err();
        let failure = running.supervisor.observed_failure();

        if let Some(failure) = failure {
            return Err(AppRunError::Supervision {
                failure: Box::new(failure),
                entry: outcome.err(),
                shutdown: shutdown.map(Box::new),
            });
        }
        match (outcome, shutdown) {
            (Ok(()), None) => Ok(()),
            (Err(entry), None) => Err(AppRunError::Entrypoint(entry)),
            (Ok(()), Some(shutdown)) => Err(AppRunError::Startup(shutdown)),
            (Err(entry), Some(shutdown)) => {
                Err(AppRunError::EntrypointAndShutdown { entry, shutdown })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_schema::{CapabilityRef, OperationDefinition, OperationKind, TypeRef};
    use std::net::SocketAddr;
    use std::sync::Arc;

    fn contract(version: u32) -> CapabilityContract {
        CapabilityContract {
            capability: CapabilityRef {
                id: "example.counter".into(),
                version,
            },
            operations: BTreeMap::from([(
                "read".into(),
                OperationDefinition {
                    kind: OperationKind::Query,
                    description: "Read a value".into(),
                    inputs: vec![],
                    output: TypeRef::U64,
                    error: TypeRef::Unit,
                },
            )]),
            types: BTreeMap::new(),
        }
    }

    struct Dummy {
        local_version: u32,
        remote_version: u32,
    }

    impl CapabilityConnector for Dummy {
        fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
            Ok(contract(self.local_version))
        }
        fn connect(
            &self,
            _endpoint: &ServiceEndpoint,
        ) -> Result<ConnectedProvider, ConnectorError> {
            Ok(ConnectedProvider {
                definition: ProviderDefinition {
                    id: "example.counter.local".into(),
                    capabilities: vec![contract(self.remote_version).capability.clone()],
                },
                contract: contract(self.remote_version),
                service: Box::new(Arc::new(42_u64)),
            })
        }
    }

    fn app() -> App {
        App::from_schema(
            r#"{"version":1,"app":{"name":"Example"},"models":{},
                "requires":[{"name":"counter","capability":{
                    "id":"example.counter","version":1}}]}"#,
        )
        .unwrap()
    }

    fn deployment() -> DeploymentConfig {
        DeploymentConfig {
            services: BTreeMap::from([(
                "counter".into(),
                ServiceEndpoint::Tcp {
                    address: "127.0.0.1:47911".parse::<SocketAddr>().unwrap(),
                },
            )]),
        }
    }

    #[test]
    fn app_prepares_and_exposes_typed_provider() {
        let mut connectors = ConnectorRegistry::new();
        connectors
            .register(Dummy {
                local_version: 1,
                remote_version: 1,
            })
            .unwrap();
        let running = app().start(&deployment(), &connectors).unwrap();
        assert_eq!(running.resolved()[0].provider, "example.counter.local");
        assert_eq!(**running.service::<Arc<u64>>("counter").unwrap(), 42);
        assert!(matches!(
            running.service::<Arc<String>>("counter"),
            Err(RuntimeError::WrongServiceType(_))
        ));
    }

    #[test]
    fn app_rejects_missing_and_mismatched_connectors() {
        let empty = ConnectorRegistry::new();
        assert!(matches!(
            app().start(&deployment(), &empty),
            Err(RuntimeError::MissingConnector { .. })
        ));

        let mut mismatched = ConnectorRegistry::new();
        mismatched
            .register(Dummy {
                local_version: 2,
                remote_version: 2,
            })
            .unwrap();
        assert!(matches!(
            app().start(&deployment(), &mismatched),
            Err(RuntimeError::LocalContractMismatch { .. })
        ));

        let mut remote = ConnectorRegistry::new();
        remote
            .register(Dummy {
                local_version: 1,
                remote_version: 2,
            })
            .unwrap();
        assert!(matches!(
            app().start(&deployment(), &remote),
            Err(RuntimeError::RemoteContractMismatch { .. })
        ));
    }

    #[test]
    fn registry_rejects_duplicate_connectors() {
        let mut connectors = ConnectorRegistry::new();
        connectors
            .register(Dummy {
                local_version: 1,
                remote_version: 1,
            })
            .unwrap();
        assert!(matches!(
            connectors.register(Dummy {
                local_version: 1,
                remote_version: 1,
            }),
            Err(RuntimeError::DuplicateConnector(_))
        ));
    }
    struct Unavailable;

    impl CapabilityConnector for Unavailable {
        fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
            Ok(contract(1))
        }

        fn connect(
            &self,
            _endpoint: &ServiceEndpoint,
        ) -> Result<ConnectedProvider, ConnectorError> {
            Err(io::Error::new(io::ErrorKind::ConnectionRefused, "service not ready").into())
        }
    }

    #[test]
    fn runtime_propagates_entrypoint_errors() {
        let mut connectors = ConnectorRegistry::new();
        connectors
            .register(Dummy {
                local_version: 1,
                remote_version: 1,
            })
            .unwrap();

        let result = app().run(&deployment(), &connectors, |_running| {
            Err(io::Error::other("application failed").into())
        });
        assert!(matches!(result, Err(AppRunError::Entrypoint(_))));
    }

    #[test]
    fn child_exits_immediately() {
        // Invoked as a subprocess to test early managed-service termination.
    }

    #[test]
    fn runtime_rejects_managed_service_without_ready_announcement() {
        let mut config = deployment();
        config.services.insert(
            "counter".into(),
            ServiceEndpoint::Managed {
                program: std::env::current_exe().unwrap(),
                args: vec![
                    "--exact".into(),
                    "runtime::tests::child_exits_immediately".into(),
                ],
                startup_timeout_ms: 5_000,
                shutdown_timeout_ms: 2_000,
            },
        );
        let mut connectors = ConnectorRegistry::new();
        connectors.register(Unavailable).unwrap();

        assert!(matches!(
            app().start(&config, &connectors),
            Err(RuntimeError::ServiceStart { .. })
        ));
    }

    /// Construct an app backed by a real OS child that exits immediately.
    /// No fake process status or forced error injection is involved.
    fn with_exiting_managed_child() -> RunningApp {
        let key = ProcessKey::new(1, Some("counter".into()));
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "runtime::tests::child_exits_immediately"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut registry = ProcessRegistry::new();
        registry.spawn(key.clone(), &mut command).unwrap();
        let processes = Arc::new(Mutex::new(registry));
        let supervisor = ServiceMonitor::start(
            Arc::clone(&processes),
            BTreeMap::from([("counter".into(), key)]),
        )
        .unwrap();
        RunningApp {
            app: app(),
            resolved: vec![],
            services: BTreeMap::new(),
            processes,
            supervisor,
        }
    }

    #[test]
    fn managed_child_exit_cancels_application_and_returns_supervision_error() {
        let running = with_exiting_managed_child();
        let token = running.cancellation_token();
        let result = RunningApp::execute(running, |app| {
            assert!(token.wait_cancelled_timeout(Duration::from_secs(8)));
            assert!(matches!(
                app.check_processes(),
                Err(RuntimeError::ServiceExited { requirement, .. })
                    if requirement == "counter"
            ));
            Ok(())
        });
        match result {
            Err(AppRunError::Supervision {
                failure,
                entry: None,
                shutdown: None,
            }) => assert!(matches!(
                *failure,
                RuntimeError::ServiceExited { requirement, .. }
                    if requirement == "counter"
            )),
            other => panic!("expected managed service failure, received {other:?}"),
        }
    }

    #[test]
    fn supervision_preserves_concurrent_business_error() {
        let running = with_exiting_managed_child();
        let token = running.cancellation_token();
        let result = RunningApp::execute(running, |_| {
            assert!(token.wait_cancelled_timeout(Duration::from_secs(8)));
            Err(io::Error::other("entrypoint failed after cancellation").into())
        });
        assert!(matches!(
            result,
            Err(AppRunError::Supervision {
                entry: Some(_),
                shutdown: None,
                ..
            })
        ));
    }

    #[test]
    fn preflight_orders_all_dependencies_without_connecting_or_starting() {
        let application = App::from_schema(
            r#"{"version":1,"app":{"name":"Example"},"models":{},
                "requires":[
                    {"name":"consumer","capability":{"id":"example.counter","version":1},
                     "depends_on":["storage"]},
                    {"name":"storage","capability":{"id":"example.counter","version":1}}
                ]}"#,
        )
        .unwrap();
        let deployment = DeploymentConfig {
            services: BTreeMap::from([
                (
                    "consumer".into(),
                    ServiceEndpoint::Tcp {
                        address: "127.0.0.1:43911".parse().unwrap(),
                    },
                ),
                (
                    "storage".into(),
                    ServiceEndpoint::Managed {
                        program: "file-that-does-not-exist".into(),
                        args: vec![],
                        startup_timeout_ms: 1_000,
                        shutdown_timeout_ms: 2_000,
                    },
                ),
            ]),
        };
        let mut connectors = ConnectorRegistry::new();
        connectors
            .register(Dummy {
                local_version: 1,
                remote_version: 1,
            })
            .unwrap();
        let plan = application.preflight(&deployment, &connectors).unwrap();
        assert_eq!(plan.services.len(), 2);
        assert_eq!(plan.services[0].requirement, "storage");
        assert_eq!(plan.services[1].requirement, "consumer");
        assert_eq!(plan.services[1].depends_on, ["storage"]);
        assert!(matches!(
            &plan.services[0].target,
            PlannedTarget::Managed { program, .. }
                if program.to_str() == Some("file-that-does-not-exist")
        ));
        let json = serde_json::to_string(&plan).unwrap();
        assert!(json.contains("example.counter"));
        assert!(!json.contains("args"));
    }

    #[test]
    fn preflight_rejects_a_late_missing_connector_before_any_process_spawn() {
        let application = App::from_schema(
            r#"{"version":1,"app":{"name":"Example"},"models":{},
                "requires":[
                    {"name":"first","capability":{"id":"example.counter","version":1}},
                    {"name":"second","capability":{"id":"example.other","version":1}}
                ]}"#,
        )
        .unwrap();
        let config = DeploymentConfig::from_json(
            r#"{"services":{
                "first":{"transport":"managed","program":"nonexistent-first-child"},
                "second":{"transport":"managed","program":"nonexistent-second-child"}
            }}"#,
        )
        .unwrap();
        let mut connectors = ConnectorRegistry::new();
        connectors
            .register(Dummy {
                local_version: 1,
                remote_version: 1,
            })
            .unwrap();
        assert!(matches!(
            application.start(&config, &connectors),
            Err(RuntimeError::MissingConnector { requirement, .. })
                if requirement == "second"
        ));
    }

    #[test]
    fn preflight_rejects_invalid_local_provider_before_remote_io() {
        let application = App::from_schema(
            r#"{"version":1,"app":{"name":"Example"},"models":{},
                "requires":[
                    {"name":"first","capability":{"id":"example.counter","version":1}},
                    {"name":"second","capability":{"id":"example.counter","version":1}}
                ]}"#,
        )
        .unwrap();
        let config = DeploymentConfig {
            services: BTreeMap::from([
                (
                    "first".into(),
                    ServiceEndpoint::Tcp {
                        address: "127.0.0.1:43911".parse().unwrap(),
                    },
                ),
                (
                    "second".into(),
                    ServiceEndpoint::Local {
                        provider: "missing.provider".into(),
                    },
                ),
            ]),
        };
        let mut connectors = ConnectorRegistry::new();
        connectors
            .register(Dummy {
                local_version: 1,
                remote_version: 1,
            })
            .unwrap();
        assert!(matches!(
            application.start(&config, &connectors),
            Err(RuntimeError::UnknownLocalProvider(provider))
                if provider == "missing.provider"
        ));
    }

    struct LocalCounter(&'static str);

    impl LocalProviderFactory for LocalCounter {
        fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
            Ok(ProviderDefinition {
                id: self.0.into(),
                capabilities: vec![contract(1).capability],
            })
        }

        fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
            Ok(contract(1))
        }

        fn create(&self) -> Result<ConnectedProvider, ConnectorError> {
            Ok(ConnectedProvider {
                definition: self.definition()?,
                contract: self.contract()?,
                service: Box::new(Arc::new(123_u64)),
            })
        }
    }

    struct TrackedLocal {
        id: &'static str,
        created: Arc<std::sync::Mutex<Vec<String>>>,
    }

    impl LocalProviderFactory for TrackedLocal {
        fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
            Ok(ProviderDefinition {
                id: self.id.into(),
                capabilities: vec![contract(1).capability],
            })
        }

        fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
            Ok(contract(1))
        }

        fn create(&self) -> Result<ConnectedProvider, ConnectorError> {
            self.created.lock().unwrap().push(self.id.into());
            Ok(ConnectedProvider {
                definition: self.definition()?,
                contract: self.contract()?,
                service: Box::new(Arc::new(123_u64)),
            })
        }
    }

    #[test]
    fn dependency_order_controls_actual_provider_startup() {
        let app = App::from_schema(
            r#"{"version":1,"app":{"name":"Example"},"models":{},
                "requires":[
                    {"name":"frontend","capability":{"id":"example.counter","version":1},
                     "depends_on":["worker"]},
                    {"name":"worker","capability":{"id":"example.counter","version":1},
                     "depends_on":["storage"]},
                    {"name":"storage","capability":{"id":"example.counter","version":1}}
                ]}"#,
        )
        .unwrap();
        let created = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut registry = ConnectorRegistry::new();
        let mut services = BTreeMap::new();
        for alias in ["frontend", "worker", "storage"] {
            let provider = format!("example.provider.{alias}");
            registry
                .register_local(TrackedLocal {
                    id: match alias {
                        "frontend" => "example.provider.frontend",
                        "worker" => "example.provider.worker",
                        _ => "example.provider.storage",
                    },
                    created: Arc::clone(&created),
                })
                .unwrap();
            services.insert(alias.into(), ServiceEndpoint::Local { provider });
        }
        let running = app
            .start(&DeploymentConfig { services }, &registry)
            .unwrap();
        assert_eq!(
            *created.lock().unwrap(),
            [
                "example.provider.storage",
                "example.provider.worker",
                "example.provider.frontend"
            ]
        );
        assert_eq!(running.resolved().len(), 3);
    }

    #[test]
    fn starts_local_service_without_tcp_or_deployment_json() {
        let mut connectors = ConnectorRegistry::new();
        connectors
            .register_local(LocalCounter("example.counter.memory"))
            .unwrap();
        let running = app().start_local(&connectors).unwrap();
        assert_eq!(running.resolved()[0].provider, "example.counter.memory");
        assert_eq!(**running.service::<Arc<u64>>("counter").unwrap(), 123);
    }

    #[test]
    fn local_selection_rejects_missing_and_ambiguous_providers() {
        let empty = ConnectorRegistry::new();
        assert!(matches!(
            app().start_local(&empty),
            Err(RuntimeError::NoLocalProvider { .. })
        ));
        let mut connectors = ConnectorRegistry::new();
        connectors
            .register_local(LocalCounter("example.counter.memory"))
            .unwrap();
        connectors
            .register_local(LocalCounter("example.counter.other"))
            .unwrap();
        assert!(matches!(
            app().start_local(&connectors),
            Err(RuntimeError::AmbiguousLocalProvider { .. })
        ));
        let explicit = DeploymentConfig {
            services: BTreeMap::from([(
                "counter".into(),
                ServiceEndpoint::Local {
                    provider: "example.counter.other".into(),
                },
            )]),
        };
        let running = app().start(&explicit, &connectors).unwrap();
        assert_eq!(running.resolved()[0].provider, "example.counter.other");
    }

    #[test]
    fn local_registry_rejects_duplicate_providers() {
        let mut connectors = ConnectorRegistry::new();
        connectors
            .register_local(LocalCounter("example.counter.memory"))
            .unwrap();
        assert!(matches!(
            connectors.register_local(LocalCounter("example.counter.memory")),
            Err(RuntimeError::DuplicateLocalProvider(_))
        ));
    }

    #[test]
    fn two_local_requirements_reuse_the_same_provider_instance() {
        let app = App::from_schema(
            r#"{"version":1,"app":{"name":"Example"},"models":{},
                "requires":[
                    {"name":"first","capability":{"id":"example.counter","version":1}},
                    {"name":"second","capability":{"id":"example.counter","version":1}}
                ]}"#,
        )
        .unwrap();
        let mut registry = ConnectorRegistry::new();
        registry
            .register_local(LocalCounter("example.counter.memory"))
            .unwrap();

        let running = app.start_local(&registry).unwrap();
        assert_eq!(running.resolved().len(), 2);
        let first = running.service::<Arc<u64>>("first").unwrap();
        let second = running.service::<Arc<u64>>("second").unwrap();
        assert!(Arc::ptr_eq(first, second));
    }

    #[test]
    fn startup_message_accepts_only_versioned_loopback_endpoints() {
        let valid = b"RIESEL_READY {\"protocol\":1,\"address\":\"127.0.0.1:52111\"}\n";
        let ready =
            await_ready(std::io::Cursor::new(valid.to_vec()), Duration::from_secs(1)).unwrap();
        assert_eq!(ready.address.port(), 52111);
        assert!(ready.shutdown.is_none());

        let cooperative = b"RIESEL_READY {\"protocol\":1,\"address\":\"127.0.0.1:52111\",\"shutdown\":\"stdin_line\"}\n";
        let ready = await_ready(
            std::io::Cursor::new(cooperative.to_vec()),
            Duration::from_secs(1),
        )
        .unwrap();
        assert!(matches!(ready.shutdown, Some(ShutdownMode::StdinLine)));

        for invalid in [
            "RIESEL_READY {\"protocol\":2,\"address\":\"127.0.0.1:52111\"}\n",
            "RIESEL_READY {\"protocol\":1,\"address\":\"0.0.0.0:52111\"}\n",
            "RIESEL_READY {\"protocol\":1,\"address\":\"127.0.0.1:0\"}\n",
            "UNTRUSTED_READY {\"protocol\":1,\"address\":\"127.0.0.1:52111\"}\n",
            "RIESEL_READY {\"protocol\":1,\"address\":\"127.0.0.1:52111\",\"shutdown\":\"unknown\"}\n",
        ] {
            assert!(
                await_ready(
                    std::io::Cursor::new(invalid.as_bytes().to_vec()),
                    Duration::from_secs(1),
                )
                .is_err()
            );
        }
    }

    #[test]
    fn startup_message_is_length_bounded() {
        let mut payload = vec![b'a'; 8192];
        payload.push(b'\n');
        assert!(await_ready(std::io::Cursor::new(payload), Duration::from_secs(1),).is_err());
    }
}
