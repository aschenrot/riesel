use super::*;

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
}
