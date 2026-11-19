//! Shared startup state machine for synchronous and asynchronous connectors.
//! Preflight, local factory lifetime, managed process identity and contract
//! validation happen here once; the caller chooses only how to await I/O.
use super::managed::spawn_managed;
use super::registry::{RegisteredConnector, RunningLocalProvider};
use super::*;
use riesel_schema::CapabilityRequirement;

pub(super) struct StartupState<'a> {
    app: App,
    deployment: &'a DeploymentConfig,
    connectors: &'a ConnectorRegistry,
    plan: DeploymentPlan,
    pub(super) session: SessionToken,
    registry: ProviderRegistry,
    provider_definitions: BTreeMap<String, ProviderDefinition>,
    selected: BTreeMap<String, String>,
    services: BTreeMap<String, Arc<dyn Any + Send + Sync>>,
    local_instances: BTreeMap<String, RunningLocalProvider>,
    processes: ProcessRegistry,
    managed: BTreeMap<String, ProcessKey>,
    managed_instances: BTreeMap<String, (ProcessKey, SocketAddr)>,
}

pub(super) struct RemoteConnection {
    pub(super) requirement: CapabilityRequirement,
    pub(super) endpoint: ServiceEndpoint,
    pub(super) managed: Option<ProcessKey>,
}

#[derive(Clone)]
pub(super) struct PendingManaged {
    requirement: CapabilityRequirement,
    index: u64,
    instance: Option<String>,
    program: std::path::PathBuf,
    args: Vec<String>,
    startup_timeout_ms: u64,
    shutdown_timeout_ms: u64,
}

pub(super) enum StartupStep {
    Local,
    Remote(RemoteConnection),
    Managed(PendingManaged),
}

impl<'a> StartupState<'a> {
    pub(super) fn new(
        app: App,
        deployment: &'a DeploymentConfig,
        connectors: &'a ConnectorRegistry,
    ) -> Result<Self, RuntimeError> {
        // No state factory, process spawn or socket occurs before preflight.
        let plan = app.preflight(deployment, connectors)?;
        Ok(Self {
            app,
            deployment,
            connectors,
            plan,
            session: SessionToken::new(),
            registry: ProviderRegistry::new(),
            provider_definitions: BTreeMap::new(),
            selected: BTreeMap::new(),
            services: BTreeMap::new(),
            local_instances: BTreeMap::new(),
            processes: ProcessRegistry::new(),
            managed: BTreeMap::new(),
            managed_instances: BTreeMap::new(),
        })
    }

    /// Async preflight must detect incompatible connector implementations
    /// before creating local state or starting a managed service. The same
    /// deployment ordering and contract metadata were already validated by
    /// the synchronous preflight in new().
    #[cfg(feature = "async-tcp")]
    pub(super) fn new_async(
        app: App,
        deployment: &'a DeploymentConfig,
        connectors: &'a ConnectorRegistry,
    ) -> Result<Self, RuntimeError> {
        let startup = Self::new(app, deployment, connectors)?;
        for planned in &startup.plan.services {
            if !matches!(planned.target, PlannedTarget::Local { .. }) {
                let requirement = startup
                    .app
                    .definition()
                    .requires
                    .iter()
                    .find(|requirement| requirement.name == planned.requirement)
                    .expect("preflight must only plan registered requirements");
                if !startup
                    .connector(requirement)?
                    .connector
                    .supports_async_startup()
                {
                    return Err(RuntimeError::UnsupportedAsyncStartup {
                        requirement: requirement.name.clone(),
                    });
                }
            }
        }
        Ok(startup)
    }

    pub(super) fn len(&self) -> usize {
        self.plan.services.len()
    }

    fn requirement(&self, index: usize) -> CapabilityRequirement {
        let name = &self.plan.services[index].requirement;
        self.app
            .definition()
            .requires
            .iter()
            .find(|requirement| &requirement.name == name)
            .expect("preflight must only plan registered requirements")
            .clone()
    }

    pub(super) fn connector(
        &self,
        requirement: &CapabilityRequirement,
    ) -> Result<&RegisteredConnector, RuntimeError> {
        let key = (
            requirement.capability.id.clone(),
            requirement.capability.version,
        );
        let adapter =
            self.connectors
                .adapters
                .get(&key)
                .ok_or_else(|| RuntimeError::MissingConnector {
                    requirement: requirement.name.clone(),
                    id: requirement.capability.id.clone(),
                    version: requirement.capability.version,
                })?;
        if adapter.contract.capability != requirement.capability {
            return Err(RuntimeError::LocalContractMismatch {
                requirement: requirement.name.clone(),
            });
        }
        Ok(adapter)
    }

    /// Prepare one step in dependency order. A local component is constructed
    /// once; managed launches are returned as work for the caller to execute
    /// at an explicit blocking boundary.
    pub(super) fn prepare(&mut self, index: usize) -> Result<StartupStep, RuntimeError> {
        let requirement = self.requirement(index);
        let endpoint = self.deployment.services[&requirement.name].clone();
        match endpoint {
            ServiceEndpoint::Local { provider } => {
                self.attach_local(&requirement, &provider)?;
                Ok(StartupStep::Local)
            }
            endpoint @ ServiceEndpoint::Tcp { .. } => {
                self.connector(&requirement)?;
                Ok(StartupStep::Remote(RemoteConnection {
                    requirement,
                    endpoint,
                    managed: None,
                }))
            }
            ServiceEndpoint::Managed {
                instance,
                program,
                args,
                startup_timeout_ms,
                shutdown_timeout_ms,
            } => {
                self.connector(&requirement)?;
                if let Some((key, address)) = instance
                    .as_ref()
                    .and_then(|instance| self.managed_instances.get(instance))
                {
                    return Ok(StartupStep::Remote(RemoteConnection {
                        requirement,
                        endpoint: ServiceEndpoint::Tcp { address: *address },
                        managed: Some(key.clone()),
                    }));
                }
                Ok(StartupStep::Managed(PendingManaged {
                    requirement,
                    index: index as u64 + 1,
                    instance,
                    program,
                    args,
                    startup_timeout_ms,
                    shutdown_timeout_ms,
                }))
            }
        }
    }

    fn attach_local(
        &mut self,
        requirement: &CapabilityRequirement,
        provider: &str,
    ) -> Result<(), RuntimeError> {
        let local = self
            .connectors
            .local
            .get(provider)
            .ok_or_else(|| RuntimeError::UnknownLocalProvider(provider.into()))?;
        let key = (
            requirement.capability.id.clone(),
            requirement.capability.version,
        );
        if !local
            .definition
            .capabilities
            .contains(&requirement.capability)
            || !local.contracts.contains_key(&key)
        {
            return Err(RuntimeError::LocalContractMismatch {
                requirement: requirement.name.clone(),
            });
        }
        if !self.local_instances.contains_key(provider) {
            #[cfg(feature = "async-tcp")]
            let instance = local.factory.create_with_session(self.session.clone());
            #[cfg(not(feature = "async-tcp"))]
            let instance = local.factory.create();
            let instance = instance.map_err(|source| RuntimeError::Connection {
                requirement: requirement.name.clone(),
                source,
            })?;
            if instance.definition != local.definition {
                return Err(RuntimeError::LocalProviderChanged(provider.into()));
            }
            #[cfg(feature = "async-tcp")]
            let blocking = instance.blocking.clone();
            let mut bound = BTreeMap::new();
            for binding in instance.bindings {
                let binding_key = (
                    binding.contract.capability.id.clone(),
                    binding.contract.capability.version,
                );
                let registered = local.contracts.get(&binding_key);
                if !registered.is_some_and(|expected| binding.contract.has_same_api_shape(expected))
                    || binding.mode != local.factory.binding_mode(&binding.contract.capability)
                    || bound
                        .insert(
                            binding_key,
                            Arc::<dyn Any + Send + Sync>::from(
                                binding.attach(self.session.clone()).map_err(|_| {
                                    RuntimeError::LocalProviderChanged(provider.into())
                                })?,
                            ),
                        )
                        .is_some()
                {
                    return Err(RuntimeError::LocalProviderChanged(provider.into()));
                }
            }
            if bound.len() != local.contracts.len() {
                return Err(RuntimeError::LocalProviderChanged(provider.into()));
            }
            self.registry.register(instance.definition.clone())?;
            self.provider_definitions
                .insert(provider.into(), instance.definition);
            self.local_instances.insert(
                provider.into(),
                RunningLocalProvider {
                    services: bound,
                    #[cfg(feature = "async-tcp")]
                    blocking,
                },
            );
        }
        let service = Arc::clone(
            self.local_instances
                .get(provider)
                .and_then(|instance| instance.services.get(&key))
                .ok_or_else(|| RuntimeError::LocalProviderChanged(provider.into()))?,
        );
        self.selected
            .insert(requirement.name.clone(), provider.into());
        self.services.insert(requirement.name.clone(), service);
        Ok(())
    }

    fn managed_ready(
        &mut self,
        pending: PendingManaged,
        launched: (ProcessKey, SocketAddr),
    ) -> RemoteConnection {
        if let Some(instance) = pending.instance {
            self.managed_instances.insert(instance, launched.clone());
        }
        RemoteConnection {
            requirement: pending.requirement,
            endpoint: ServiceEndpoint::Tcp {
                address: launched.1,
            },
            managed: Some(launched.0),
        }
    }

    pub(super) fn spawn_managed_sync(
        &mut self,
        pending: PendingManaged,
    ) -> Result<RemoteConnection, RuntimeError> {
        let launched = spawn_managed(
            &mut self.processes,
            pending.index,
            &pending.requirement.name,
            &pending.program,
            &pending.args,
            pending.startup_timeout_ms,
            pending.shutdown_timeout_ms,
        )?;
        Ok(self.managed_ready(pending, launched))
    }

    /// The blocking OS process handshake runs on a dedicated Tokio blocking
    /// worker; connector metadata I/O remains asynchronous on the event loop.
    /// If startup is cancelled, that worker still owns the registry and its
    /// Drop guard until its bounded readiness wait finishes.
    #[cfg(feature = "async-tcp")]
    pub(super) async fn spawn_managed_async(
        &mut self,
        pending: PendingManaged,
    ) -> Result<RemoteConnection, RuntimeError> {
        let processes = std::mem::replace(&mut self.processes, ProcessRegistry::new());
        let launch = pending.clone();
        let (processes, result) = tokio::task::spawn_blocking(move || {
            let mut processes = processes;
            let result = spawn_managed(
                &mut processes,
                launch.index,
                &launch.requirement.name,
                &launch.program,
                &launch.args,
                launch.startup_timeout_ms,
                launch.shutdown_timeout_ms,
            );
            (processes, result)
        })
        .await
        .map_err(|error| RuntimeError::ServiceStart {
            requirement: pending.requirement.name.clone(),
            source: io::Error::other(error),
        })?;
        self.processes = processes;
        Ok(self.managed_ready(pending, result?))
    }

    pub(super) fn accept_remote(
        &mut self,
        connection: RemoteConnection,
        connected: ConnectedProvider,
    ) -> Result<(), RuntimeError> {
        let requirement = connection.requirement;
        let expected = &self.connector(&requirement)?.contract;
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
        if let Some(expected) = self.deployment.expected_providers.get(&requirement.name)
            && !connected.definition.has_same_definition(expected)
        {
            return Err(RuntimeError::ProviderIdentityMismatch {
                requirement: requirement.name.clone(),
                expected: Box::new(expected.clone()),
                actual: Box::new(connected.definition),
            });
        }
        if !connected
            .definition
            .capabilities
            .contains(&requirement.capability)
            || !connected.contract.has_same_api_shape(expected)
        {
            return Err(RuntimeError::RemoteContractMismatch {
                requirement: requirement.name,
            });
        }
        let provider_id = connected.definition.id.clone();
        if let Some(registered) = self.provider_definitions.get(&provider_id) {
            if !registered.has_same_definition(&connected.definition) {
                return Err(RuntimeError::ConflictingProvider {
                    provider: provider_id,
                });
            }
        } else {
            self.registry.register(connected.definition.clone())?;
            self.provider_definitions
                .insert(provider_id.clone(), connected.definition);
        }
        if let Some(key) = connection.managed {
            self.managed.insert(requirement.name.clone(), key);
        }
        self.selected.insert(requirement.name.clone(), provider_id);
        self.services
            .insert(requirement.name, connected.service.into());
        Ok(())
    }

    pub(super) fn finish(self) -> Result<RunningApp, RuntimeError> {
        let mut resolved = self
            .registry
            .resolve(self.app.definition(), &self.selected)?;
        for binding in &mut resolved {
            binding.instance = Some(match &self.deployment.services[&binding.requirement] {
                ServiceEndpoint::Local { provider } => crate::InstanceIdentity::Local {
                    provider: provider.clone(),
                },
                ServiceEndpoint::Managed { instance, .. } => crate::InstanceIdentity::Managed {
                    name: instance
                        .clone()
                        .unwrap_or_else(|| binding.requirement.clone()),
                },
                ServiceEndpoint::Tcp { address } => {
                    crate::InstanceIdentity::ExternalTcp { address: *address }
                }
            });
        }
        let processes = Arc::new(Mutex::new(self.processes));
        let supervisor =
            ServiceMonitor::start(Arc::clone(&processes), self.managed, self.session.clone())
                .map_err(RuntimeError::MonitorStart)?;
        let tasks = TaskGroup::new(supervisor.cancellation_token());
        #[cfg(feature = "async-tcp")]
        let async_tasks = AsyncTasks::new(self.session.clone(), supervisor.cancellation_token());
        #[cfg(feature = "async-tcp")]
        let local_blocking: Vec<_> = self
            .local_instances
            .into_values()
            .filter_map(|local| local.blocking)
            .collect();
        Ok(RunningApp {
            app: self.app,
            resolved,
            services: self.services,
            processes,
            supervisor,
            tasks,
            #[cfg(feature = "async-tcp")]
            async_tasks,
            #[cfg(feature = "async-tcp")]
            local_blocking,
            session: self.session,
        })
    }
}
