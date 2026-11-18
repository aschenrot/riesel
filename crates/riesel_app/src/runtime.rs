//! Provider-agnostic application composition.
//! Connectors implement transport-specific discovery and typed service creation;
//! this module owns validation, resolution, and the resulting service lifetimes.

use crate::plan::{DeploymentPlan, PlannedService, PlannedTarget};
use crate::recovery::run_with_recovery;
use crate::supervisor::{CancellationToken, ServiceMonitor};
use crate::tasks::TaskGroup;
use crate::{
    App, DeploymentConfig, DeploymentError, ProviderRegistry, ResolutionError, ResolvedCapability,
    ServiceEndpoint,
};
use riesel_manager::{ProcessKey, ProcessRegistry};
use riesel_protocol::SessionToken;
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
    fn connect(
        &self,
        endpoint: &ServiceEndpoint,
        session: &SessionToken,
    ) -> Result<ConnectedProvider, ConnectorError>;
}

struct RegisteredConnector {
    contract: CapabilityContract,
    connector: Box<dyn CapabilityConnector>,
}

/// One executable binding for a capability advertised by a local component.
pub struct LocalCapabilityBinding {
    pub contract: CapabilityContract,
    service: Box<dyn Any + Send + Sync>,
    guard: fn(
        Box<dyn Any + Send + Sync>,
        SessionToken,
    ) -> Result<Box<dyn Any + Send + Sync>, ConnectorError>,
}

impl LocalCapabilityBinding {
    /// Convert the registered typed local service into a session-scoped
    /// capability handle. Consumers cannot directly construct unguarded
    /// bindings through the public component factory API.
    fn attach(self, session: SessionToken) -> Result<Box<dyn Any + Send + Sync>, ConnectorError> {
        (self.guard)(self.service, session)
    }

    #[cfg(test)]
    fn raw(contract: CapabilityContract, service: Box<dyn Any + Send + Sync>) -> Self {
        Self {
            contract,
            service,
            guard: |service, _| Ok(service),
        }
    }
}

fn guard_local_binding<C: riesel_protocol::RpcClientBinding>(
    value: Box<dyn Any + Send + Sync>,
    session: SessionToken,
) -> Result<Box<dyn Any + Send + Sync>, ConnectorError> {
    let service = value.downcast::<C::Service>().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid typed local component binding",
        )
    })?;
    Ok(Box::new(C::guard_local(*service, session)))
}

/// An instantiated local component can expose multiple capability interfaces
/// backed by the same underlying state and lifetime.
pub struct LocalProviderInstance {
    pub definition: ProviderDefinition,
    pub bindings: Vec<LocalCapabilityBinding>,
}

impl LocalProviderInstance {
    pub fn new(definition: ProviderDefinition) -> Self {
        Self {
            definition,
            bindings: Vec::new(),
        }
    }

    pub fn bind<C: riesel_protocol::RpcClientBinding>(
        mut self,
        service: C::Service,
    ) -> Result<Self, ConnectorError> {
        self.bindings.push(LocalCapabilityBinding {
            contract: C::definition()?,
            service: Box::new(service),
            guard: guard_local_binding::<C>,
        });
        Ok(self)
    }
}

/// Provider registration knows all advertised contracts before startup.
pub trait LocalProviderFactory: Send + Sync {
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError>;
    fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError>;
    fn create(&self) -> Result<LocalProviderInstance, ConnectorError>;
}

/// Generic local component factory; avoids repeating three handwritten
/// metadata and instance methods for every component implementation.
pub struct LocalComponentFactory<F> {
    id: String,
    contracts: Vec<CapabilityContract>,
    create: F,
}

impl<F> LocalComponentFactory<F> {
    pub fn new(id: impl Into<String>, create: F) -> Self {
        Self {
            id: id.into(),
            contracts: Vec::new(),
            create,
        }
    }

    pub fn exposes<C: riesel_schema::CapabilityDescriptor>(
        mut self,
    ) -> Result<Self, ConnectorError> {
        self.contracts.push(C::definition()?);
        Ok(self)
    }
}

impl<F> LocalProviderFactory for LocalComponentFactory<F>
where
    F: Fn() -> Result<LocalProviderInstance, ConnectorError> + Send + Sync,
{
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
        Ok(ProviderDefinition {
            id: self.id.clone(),
            capabilities: self
                .contracts
                .iter()
                .map(|contract| contract.capability.clone())
                .collect(),
        })
    }

    fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
        Ok(self.contracts.clone())
    }

    fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
        (self.create)()
    }
}

/// Single-capability shortcut. The generated Rust contract is authoritative;
/// no manual provider metadata, boxed service or factory trait is needed.
pub struct SingleLocalProvider<C, F> {
    id: String,
    create: F,
    contract: std::marker::PhantomData<fn() -> C>,
}

pub fn local_provider<C, F>(id: impl Into<String>, create: F) -> SingleLocalProvider<C, F>
where
    C: riesel_protocol::RpcClientBinding + 'static,
    F: Fn() -> C::Service + Send + Sync + 'static,
{
    SingleLocalProvider {
        id: id.into(),
        create,
        contract: std::marker::PhantomData,
    }
}

impl<C, F> LocalProviderFactory for SingleLocalProvider<C, F>
where
    C: riesel_protocol::RpcClientBinding + 'static,
    F: Fn() -> C::Service + Send + Sync,
{
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
        Ok(ProviderDefinition {
            id: self.id.clone(),
            capabilities: vec![C::definition()?.capability],
        })
    }

    fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
        Ok(vec![C::definition()?])
    }

    fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
        LocalProviderInstance::new(self.definition()?).bind::<C>((self.create)())
    }
}

struct RegisteredLocalProvider {
    definition: ProviderDefinition,
    contracts: BTreeMap<(String, u32), CapabilityContract>,
    factory: Box<dyn LocalProviderFactory>,
}

struct RunningLocalProvider {
    services: BTreeMap<(String, u32), Arc<dyn Any + Send + Sync>>,
}

/// Explicit runtime adapter registry. Contract IDs (not Rust types or TCP
/// addresses) are the stable lookup keys.
#[derive(Default)]
pub struct ConnectorRegistry {
    adapters: BTreeMap<(String, u32), RegisteredConnector>,
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
        let contracts = factory
            .contracts()
            .map_err(|source| RuntimeError::ProviderFactory { source })?;
        let mut available = BTreeMap::new();
        for contract in contracts {
            contract
                .validate()
                .map_err(|source| RuntimeError::InvalidContract { source })?;
            let key = (contract.capability.id.clone(), contract.capability.version);
            if available.insert(key, contract).is_some() {
                return Err(RuntimeError::ProviderContractMismatch(definition.id));
            }
        }
        if definition.capabilities.len() != available.len()
            || !definition.capabilities.iter().all(|capability| {
                available.contains_key(&(capability.id.clone(), capability.version))
            })
        {
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
                contracts: available,
                factory: Box::new(factory),
            },
        );
        Ok(())
    }

    /// Whether a transport adapter has already been registered for this
    /// capability ID. Allows Rust-first builders to avoid duplicate wiring.
    pub fn has_connector(&self, capability: &riesel_schema::CapabilityRef) -> bool {
        self.adapters
            .contains_key(&(capability.id.clone(), capability.version))
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
        let key = (contract.capability.id.clone(), contract.capability.version);
        if self.adapters.contains_key(&key) {
            return Err(RuntimeError::DuplicateConnector {
                id: key.0,
                version: key.1,
            });
        }
        self.adapters.insert(
            key,
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

/// Active application with verified services and owned child processes.
/// ProcessRegistry guarantees best-effort cleanup if startup or execution fails.
pub struct RunningApp {
    app: App,
    resolved: Vec<ResolvedCapability>,
    services: BTreeMap<String, Arc<dyn Any + Send + Sync>>,
    processes: Arc<Mutex<ProcessRegistry>>,
    supervisor: ServiceMonitor,
    tasks: TaskGroup,
    session: SessionToken,
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
        self.tasks.failure().map_or(Ok(()), Err)
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
        let tasks = self.tasks.join();
        self.session.invalidate();
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
                        || !local.contracts.contains_key(&(
                            requirement.capability.id.clone(),
                            requirement.capability.version,
                        ))
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
        let adapter = connectors
            .adapters
            .get(&(id.clone(), requirement.capability.version))
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
                        && local.contracts.contains_key(&(
                            requirement.capability.id.clone(),
                            requirement.capability.version,
                        ))
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
        self.start(&DeploymentConfig::new(services), connectors)
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
        let plan = self.preflight(deployment, connectors)?;
        let schema = self.definition().ok_or(RuntimeError::MissingDefinition)?;
        let requirements: BTreeMap<_, _> = schema
            .requires
            .iter()
            .map(|requirement| (requirement.name.as_str(), requirement))
            .collect();

        let session = SessionToken::new();
        let mut registry = ProviderRegistry::new();
        let mut provider_definitions = BTreeMap::new();
        let mut selected = BTreeMap::new();
        let mut services = BTreeMap::new();
        let mut local_instances: BTreeMap<String, RunningLocalProvider> = BTreeMap::new();
        let mut processes = ProcessRegistry::new();
        let mut managed = BTreeMap::new();

        for (index, planned) in plan.services.iter().enumerate() {
            let requirement = requirements[planned.requirement.as_str()];
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
                    || !local.contracts.contains_key(&(
                        requirement.capability.id.clone(),
                        requirement.capability.version,
                    ))
                {
                    return Err(RuntimeError::LocalContractMismatch {
                        requirement: requirement.name.clone(),
                    });
                }
                if !local_instances.contains_key(provider) {
                    let instance =
                        local
                            .factory
                            .create()
                            .map_err(|source| RuntimeError::Connection {
                                requirement: requirement.name.clone(),
                                source,
                            })?;
                    if instance.definition != local.definition {
                        return Err(RuntimeError::LocalProviderChanged(provider.clone()));
                    }
                    let mut bound = BTreeMap::new();
                    for binding in instance.bindings {
                        let key = (
                            binding.contract.capability.id.clone(),
                            binding.contract.capability.version,
                        );
                        let registered = local.contracts.get(&key);
                        if !registered
                            .is_some_and(|expected| binding.contract.has_same_api_shape(expected))
                            || bound
                                .insert(
                                    key,
                                    Arc::<dyn Any + Send + Sync>::from(
                                        binding.attach(session.clone()).map_err(|_| {
                                            RuntimeError::LocalProviderChanged(provider.clone())
                                        })?,
                                    ),
                                )
                                .is_some()
                        {
                            return Err(RuntimeError::LocalProviderChanged(provider.clone()));
                        }
                    }
                    if bound.len() != local.contracts.len() {
                        return Err(RuntimeError::LocalProviderChanged(provider.clone()));
                    }
                    registry.register(instance.definition.clone())?;
                    provider_definitions.insert(provider.clone(), instance.definition);
                    local_instances
                        .insert(provider.clone(), RunningLocalProvider { services: bound });
                }
                let key = (
                    requirement.capability.id.clone(),
                    requirement.capability.version,
                );
                let instance = Arc::clone(
                    local_instances
                        .get(provider)
                        .and_then(|local| local.services.get(&key))
                        .ok_or_else(|| RuntimeError::LocalProviderChanged(provider.clone()))?,
                );
                selected.insert(requirement.name.clone(), provider.clone());
                services.insert(requirement.name.clone(), instance);
                continue;
            }
            let id = &requirement.capability.id;
            let adapter = connectors
                .adapters
                .get(&(id.clone(), requirement.capability.version))
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
                    let connection =
                        adapter
                            .connector
                            .connect(&endpoint, &session)
                            .map_err(|source| RuntimeError::Connection {
                                requirement: requirement.name.clone(),
                                source,
                            })?;
                    managed.insert(requirement.name.clone(), key);
                    connection
                }
                ServiceEndpoint::Tcp { .. } => adapter
                    .connector
                    .connect(endpoint, &session)
                    .map_err(|source| RuntimeError::Connection {
                        requirement: requirement.name.clone(),
                        source,
                    })?,
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
        let tasks = TaskGroup::new(supervisor.cancellation_token());
        Ok(RunningApp {
            app: self,
            resolved,
            services,
            processes,
            supervisor,
            tasks,
            session,
        })
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
            _session: &SessionToken,
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
        DeploymentConfig::new(BTreeMap::from([(
            "counter".into(),
            ServiceEndpoint::Tcp {
                address: "127.0.0.1:47911".parse::<SocketAddr>().unwrap(),
            },
        )]))
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
            Err(RuntimeError::MissingConnector { .. })
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
    fn registry_supports_multiple_versions_of_the_same_capability() {
        let mut connectors = ConnectorRegistry::new();
        for version in [1, 2] {
            connectors
                .register(Dummy {
                    local_version: version,
                    remote_version: version,
                })
                .unwrap();
        }
        assert!(connectors.has_connector(&contract(1).capability));
        assert!(connectors.has_connector(&contract(2).capability));
        assert!(!connectors.has_connector(&contract(3).capability));

        let app = App::from_schema(
            r#"{"version":1,"app":{"name":"TwoVersions"},"models":{},
                "requires":[
                    {"name":"counter_v1","capability":{"id":"example.counter","version":1}},
                    {"name":"counter_v2","capability":{"id":"example.counter","version":2}}
                ]}"#,
        )
        .unwrap();
        let deployment = DeploymentConfig::new(BTreeMap::from([
            (
                "counter_v1".into(),
                ServiceEndpoint::Tcp {
                    address: "127.0.0.1:47911".parse().unwrap(),
                },
            ),
            (
                "counter_v2".into(),
                ServiceEndpoint::Tcp {
                    address: "127.0.0.1:47912".parse().unwrap(),
                },
            ),
        ]));
        assert_eq!(
            app.preflight(&deployment, &connectors)
                .unwrap()
                .services
                .len(),
            2
        );
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
            Err(RuntimeError::DuplicateConnector { .. })
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
            _session: &SessionToken,
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
        let tasks = TaskGroup::new(supervisor.cancellation_token());
        RunningApp {
            app: app(),
            resolved: vec![],
            services: BTreeMap::new(),
            processes,
            supervisor,
            tasks,
            session: SessionToken::new(),
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
    fn delayed_exit_child() {
        if std::env::var_os("RIESEL_RUNTIME_TEST_DELAYED_EXIT").is_some() {
            std::thread::sleep(Duration::from_millis(400));
        }
    }

    #[test]
    fn managed_failure_during_task_join_is_not_missed() {
        let key = ProcessKey::new(55, Some("counter".into()));
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "runtime::tests::delayed_exit_child"])
            .env("RIESEL_RUNTIME_TEST_DELAYED_EXIT", "1")
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
        let tasks = TaskGroup::new(supervisor.cancellation_token());
        let running = RunningApp {
            app: app(),
            resolved: vec![],
            services: BTreeMap::new(),
            processes,
            supervisor,
            tasks,
            session: SessionToken::new(),
        };
        let result = RunningApp::execute(running, |running| {
            running.spawn_task("finishing", |cancel| {
                assert!(cancel.wait_cancelled_timeout(Duration::from_secs(5)));
                // The child exits while this task is still joining. The
                // monitor must not stop until joining finishes.
                std::thread::sleep(Duration::from_millis(1_200));
                Ok(())
            })?;
            Ok(())
        });
        assert!(matches!(
            result,
            Err(AppRunError::Supervision {
                failure,
                entry: None,
                shutdown: None,
            }) if matches!(*failure, RuntimeError::ServiceExited { .. })
        ));
    }

    #[test]
    fn whole_application_restart_rebuilds_services_after_real_child_exit() {
        use crate::RecoveryPolicy;
        let mut connectors = ConnectorRegistry::new();
        connectors
            .register_local(LocalCounter("example.counter.memory"))
            .unwrap();
        let policy = RecoveryPolicy::RestartApplication {
            max_restarts: 2,
            initial_backoff_ms: 1,
            max_backoff_ms: 2,
        };
        let mut starts = 0;
        let mut entries = 0;
        crate::recovery::run_with_recovery(
            &policy,
            || {
                starts += 1;
                if starts == 1 {
                    Ok(with_exiting_managed_child())
                } else {
                    app().start_local(&connectors)
                }
            },
            |running| {
                entries += 1;
                if entries == 1 {
                    assert!(
                        running
                            .cancellation_token()
                            .wait_cancelled_timeout(Duration::from_secs(8))
                    );
                } else {
                    assert_eq!(**running.service::<Arc<u64>>("counter").unwrap(), 123);
                    assert!(!running.cancellation_token().is_cancelled());
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(starts, 2);
        assert_eq!(entries, 2);
    }

    #[test]
    fn whole_application_restart_is_bounded_after_repeated_real_process_failures() {
        use crate::RecoveryPolicy;
        let policy = RecoveryPolicy::RestartApplication {
            max_restarts: 2,
            initial_backoff_ms: 1,
            max_backoff_ms: 2,
        };
        let mut starts = 0;
        let result = crate::recovery::run_with_recovery(
            &policy,
            || {
                starts += 1;
                Ok(with_exiting_managed_child())
            },
            |running| {
                assert!(
                    running
                        .cancellation_token()
                        .wait_cancelled_timeout(Duration::from_secs(8))
                );
                Ok(())
            },
        );
        assert_eq!(starts, 3);
        assert!(matches!(
            result,
            Err(AppRunError::RecoveryExhausted {
                restarts: 2,
                last,
            }) if matches!(*last, AppRunError::Supervision { .. })
        ));
    }

    #[test]
    fn session_restart_does_not_replay_concurrent_business_errors() {
        use crate::RecoveryPolicy;
        let policy = RecoveryPolicy::RestartApplication {
            max_restarts: 3,
            initial_backoff_ms: 1,
            max_backoff_ms: 2,
        };
        let mut starts = 0;
        let result = crate::recovery::run_with_recovery(
            &policy,
            || {
                starts += 1;
                Ok(with_exiting_managed_child())
            },
            |running| {
                assert!(
                    running
                        .cancellation_token()
                        .wait_cancelled_timeout(Duration::from_secs(8))
                );
                Err(io::Error::other("business work cannot be replayed").into())
            },
        );
        assert_eq!(starts, 1);
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
    fn structured_task_failure_cancels_application_and_preserves_error() {
        let mut registry = ConnectorRegistry::new();
        registry
            .register_local(LocalCounter("example.counter.memory"))
            .unwrap();
        let result = app().run_local(&registry, |running| {
            running.spawn_task("failure", |_| {
                Err(io::Error::other("injected worker error").into())
            })?;
            assert!(
                running
                    .cancellation_token()
                    .wait_cancelled_timeout(Duration::from_secs(5))
            );
            assert!(matches!(
                running.check_processes(),
                Err(RuntimeError::TaskFailed { name, .. }) if name == "failure"
            ));
            Ok(())
        });
        assert!(matches!(
            result,
            Err(AppRunError::TaskFailure {
                failure,
                entry: None,
                ..
            }) if matches!(*failure, RuntimeError::TaskFailed { .. })
        ));
    }

    #[test]
    fn structured_tasks_are_joined_before_session_resources_are_destroyed() {
        let mut registry = ConnectorRegistry::new();
        registry
            .register_local(LocalCounter("example.counter.memory"))
            .unwrap();
        let joined = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&joined);
        app()
            .run_local(&registry, |running| {
                running.spawn_task("cooperative", move |token| {
                    token.wait_cancelled_timeout(Duration::from_secs(5));
                    flag.store(true, std::sync::atomic::Ordering::Release);
                    Ok(())
                })?;
                Ok(())
            })
            .unwrap();
        assert!(joined.load(std::sync::atomic::Ordering::Acquire));
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
            recovery: Default::default(),
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
            recovery: Default::default(),
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

        fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
            Ok(vec![contract(1)])
        }

        fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
            Ok(LocalProviderInstance {
                definition: self.definition()?,
                bindings: vec![LocalCapabilityBinding::raw(
                    contract(1),
                    Box::new(Arc::new(123_u64)),
                )],
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

        fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
            Ok(vec![contract(1)])
        }

        fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
            self.created.lock().unwrap().push(self.id.into());
            Ok(LocalProviderInstance {
                definition: self.definition()?,
                bindings: vec![LocalCapabilityBinding::raw(
                    contract(1),
                    Box::new(Arc::new(123_u64)),
                )],
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
            .start(&DeploymentConfig::new(services), &registry)
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

    struct MultiVersionLocal {
        created: Arc<std::sync::atomic::AtomicUsize>,
    }

    impl LocalProviderFactory for MultiVersionLocal {
        fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
            Ok(ProviderDefinition {
                id: "example.counter.multi".into(),
                capabilities: vec![contract(1).capability, contract(2).capability],
            })
        }

        fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
            Ok(vec![contract(1), contract(2)])
        }

        fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
            self.created
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let shared = Arc::new(47_u64);
            Ok(LocalProviderInstance {
                definition: self.definition()?,
                bindings: vec![1, 2]
                    .into_iter()
                    .map(|version| {
                        LocalCapabilityBinding::raw(
                            contract(version),
                            Box::new(Arc::clone(&shared)),
                        )
                    })
                    .collect(),
            })
        }
    }

    #[test]
    fn local_provider_exposes_multiple_capabilities_from_one_instance() {
        let app = App::from_schema(
            r#"{"version":1,"app":{"name":"Many"},"models":{},
                "requires":[
                    {"name":"counter_v1","capability":{"id":"example.counter","version":1}},
                    {"name":"counter_v2","capability":{"id":"example.counter","version":2}}
                ]}"#,
        )
        .unwrap();
        let created = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut registry = ConnectorRegistry::new();
        registry
            .register_local(MultiVersionLocal {
                created: Arc::clone(&created),
            })
            .unwrap();
        let running = app.start_local(&registry).unwrap();
        assert_eq!(created.load(std::sync::atomic::Ordering::SeqCst), 1);
        let first = running.service::<Arc<u64>>("counter_v1").unwrap();
        let second = running.service::<Arc<u64>>("counter_v2").unwrap();
        assert!(Arc::ptr_eq(first, second));
    }

    #[test]
    fn local_provider_rejects_unadvertised_or_duplicate_bindings() {
        struct Broken;
        impl LocalProviderFactory for Broken {
            fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
                Ok(ProviderDefinition {
                    id: "example.broken.provider".into(),
                    capabilities: vec![contract(1).capability],
                })
            }
            fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
                Ok(vec![contract(1)])
            }
            fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
                Ok(LocalProviderInstance::new(self.definition()?))
            }
        }
        let mut registry = ConnectorRegistry::new();
        registry.register_local(Broken).unwrap();
        assert!(matches!(
            app().start_local(&registry),
            Err(RuntimeError::LocalProviderChanged(id))
                if id == "example.broken.provider"
        ));
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
            recovery: Default::default(),
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
