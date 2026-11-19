use super::managed::spawn_managed;
use super::registry::RunningLocalProvider;
use super::*;

impl App {
    /// Start a validated application. For managed endpoints, spawn the
    /// configured trusted executable and wait for a valid provider handshake.
    /// On any startup failure, already spawned children are stopped on drop.
    pub fn start(
        self,
        deployment: &DeploymentConfig,
        connectors: &ConnectorRegistry,
    ) -> Result<RunningApp, RuntimeError> {
        let plan = self.preflight(deployment, connectors)?;
        let schema = self.definition();
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
        let mut managed_instances: BTreeMap<String, (ProcessKey, SocketAddr)> = BTreeMap::new();

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
                            || binding.mode != local.factory.binding_mode(&binding.contract.capability)
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
                    instance,
                    program,
                    args,
                    startup_timeout_ms,
                    shutdown_timeout_ms,
                } => {
                    let (key, address) = if let Some(existing) =
                        instance.as_ref().and_then(|id| managed_instances.get(id))
                    {
                        existing.clone()
                    } else {
                        let launched = spawn_managed(
                            &mut processes,
                            index as u64 + 1,
                            &requirement.name,
                            program,
                            args,
                            *startup_timeout_ms,
                            *shutdown_timeout_ms,
                        )?;
                        if let Some(instance) = instance {
                            managed_instances.insert(instance.clone(), launched.clone());
                        }
                        launched
                    };
                    // A readiness advertisement is not authenticated. The
                    // connector verifies the selected contract before launch
                    // ownership is recorded in the running session.
                    let connection = adapter
                        .connector
                        .connect(&ServiceEndpoint::Tcp { address }, &session)
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

        let mut resolved = registry.resolve(schema, &selected)?;
        for binding in &mut resolved {
            binding.instance = Some(match &deployment.services[&binding.requirement] {
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
        let processes = Arc::new(Mutex::new(processes));
        let supervisor = ServiceMonitor::start(Arc::clone(&processes), managed, session.clone())
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
