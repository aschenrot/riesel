//! Provider-agnostic application composition.
//! Connectors implement transport-specific discovery and typed service creation;
//! this module owns validation, resolution, and the resulting service lifetimes.

use crate::{
    App, DeploymentConfig, DeploymentError, ProviderRegistry, ResolutionError, ResolvedCapability,
    ServiceEndpoint,
};
use riesel_schema::{CapabilityContract, ProviderDefinition};
use riesel_manager::{ProcessKey, ProcessRegistry, ProcessStatus};
use std::any::Any;
use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::net::TcpListener;
use std::process::{Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

pub type ConnectorError = Box<dyn Error + Send + Sync>;

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

/// Explicit runtime adapter registry. Contract IDs (not Rust types or TCP
/// addresses) are the stable lookup keys.
#[derive(Default)]
pub struct ConnectorRegistry {
    adapters: BTreeMap<String, RegisteredConnector>,
}

impl ConnectorRegistry {
    pub fn new() -> Self {
        Self::default()
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
}

/// Active application with verified services and owned child processes.
/// ProcessRegistry guarantees best-effort cleanup if startup or execution fails.
pub struct RunningApp {
    app: App,
    resolved: Vec<ResolvedCapability>,
    services: BTreeMap<String, Box<dyn Any + Send + Sync>>,
    processes: ProcessRegistry,
    managed: BTreeMap<String, ProcessKey>,
}

impl RunningApp {
    pub fn app(&self) -> &App {
        &self.app
    }

    pub fn resolved(&self) -> &[ResolvedCapability] {
        &self.resolved
    }

    /// Check currently owned child processes. This is an on-demand health
    /// inspection, not background supervision or automatic recovery.
    pub fn check_processes(&mut self) -> Result<(), RuntimeError> {
        for (requirement, key) in &self.managed {
            let status = self.processes.status(key).map_err(|source| RuntimeError::ServiceStatus {
                requirement: requirement.clone(),
                source,
            })?;
            if let ProcessStatus::Exited(status) = status {
                return Err(RuntimeError::ServiceExited {
                    requirement: requirement.clone(),
                    status,
                });
            }
        }
        Ok(())
    }

    /// Drop connected adapters and explicitly terminate all owned services.
    /// External TCP services remain untouched.
    pub fn shutdown(&mut self) -> Result<(), RuntimeError> {
        self.services.clear();
        self.processes.stop_all().map_err(RuntimeError::Shutdown)?;
        self.managed.clear();
        Ok(())
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

impl App {
    /// Start a validated application. For managed endpoints, spawn the
    /// configured trusted executable and wait for a valid provider handshake.
    /// On any startup failure, already spawned children are stopped on drop.
    pub fn start(
        self,
        deployment: &DeploymentConfig,
        connectors: &ConnectorRegistry,
    ) -> Result<RunningApp, RuntimeError> {
        let schema = self.definition().ok_or(RuntimeError::MissingDefinition)?;
        deployment.validate(schema)?;

        let mut registry = ProviderRegistry::new();
        let mut provider_definitions = BTreeMap::new();
        let mut selected = BTreeMap::new();
        let mut services = BTreeMap::new();
        let mut processes = ProcessRegistry::new();
        let mut managed = BTreeMap::new();

        for (index, requirement) in schema.requires.iter().enumerate() {
            let id = &requirement.capability.id;
            let adapter = connectors.adapters.get(id).ok_or_else(|| {
                RuntimeError::MissingConnector {
                    requirement: requirement.name.clone(),
                    id: id.clone(),
                    version: requirement.capability.version,
                }
            })?;

            if adapter.contract.capability != requirement.capability {
                return Err(RuntimeError::LocalContractMismatch {
                    requirement: requirement.name.clone(),
                });
            }
            let endpoint = &deployment.services[&requirement.name];
            let connected = match endpoint {
                ServiceEndpoint::Tcp { .. } => {
                    adapter.connector.connect(endpoint).map_err(|source| {
                        RuntimeError::Connection {
                            requirement: requirement.name.clone(),
                            source,
                        }
                    })?
                }
                ServiceEndpoint::ManagedTcp {
                    address,
                    program,
                    args,
                    startup_timeout_ms,
                } => {
                    // Fail before spawning if another listener already holds this
                    // address. The bind check is not an authorization boundary.
                    let reservation = TcpListener::bind(address).map_err(|source| {
                        RuntimeError::ServiceStart {
                            requirement: requirement.name.clone(),
                            source,
                        }
                    })?;
                    drop(reservation);

                    let key = ProcessKey::new(
                        index as u64 + 1,
                        Some(requirement.name.clone()),
                    );
                    let mut command = Command::new(program);
                    command.args(args);
                    processes.spawn(key.clone(), &mut command).map_err(|source| {
                        RuntimeError::ServiceStart {
                            requirement: requirement.name.clone(),
                            source,
                        }
                    })?;

                    let deadline = Instant::now() + Duration::from_millis(*startup_timeout_ms);
                    let verified_connection = loop {
                        if let ProcessStatus::Exited(status) =
                            processes.status(&key).map_err(|source| RuntimeError::ServiceStatus {
                                requirement: requirement.name.clone(),
                                source,
                            })?
                        {
                            return Err(RuntimeError::ServiceExited {
                                requirement: requirement.name.clone(),
                                status,
                            });
                        }
                        if let Ok(connection) = adapter.connector.connect(endpoint) {
                            break connection;
                        }
                        if Instant::now() >= deadline {
                            return Err(RuntimeError::ReadinessTimeout {
                                requirement: requirement.name.clone(),
                                timeout_ms: *startup_timeout_ms,
                            });
                        }
                        thread::sleep(Duration::from_millis(50));
                    };
                    managed.insert(requirement.name.clone(), key);
                    verified_connection
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
            services.insert(requirement.name.clone(), connected.service);
        }

        let resolved = registry.resolve(schema, &selected)?;
        Ok(RunningApp {
            app: self,
            resolved,
            services,
            processes,
            managed,
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
        let mut running = self.start(deployment, connectors)?;
        let outcome = entrypoint(&running);
        let shutdown = running.shutdown();

        match (outcome, shutdown) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(entry), Ok(())) => Err(AppRunError::Entrypoint(entry)),
            (Ok(()), Err(shutdown)) => Err(AppRunError::Startup(shutdown)),
            (Err(entry), Err(shutdown)) => {
                Err(AppRunError::EntrypointAndShutdown { entry, shutdown })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_schema::{
        CapabilityRef, OperationDefinition, OperationKind, TypeRef,
    };
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
        fn connect(&self, _endpoint: &ServiceEndpoint) -> Result<ConnectedProvider, ConnectorError> {
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
}
