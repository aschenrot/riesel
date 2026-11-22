use super::*;
use riesel_schema::OperationKind;

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
        let schema = self.definition();
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
                    if let Some(expected) = deployment.expected_providers.get(&requirement.name)
                        && !expected.has_same_definition(&local.definition)
                    {
                        return Err(RuntimeError::ProviderIdentityMismatch {
                            requirement: requirement.name.clone(),
                            expected: Box::new(expected.clone()),
                            actual: Box::new(local.definition.clone()),
                        });
                    }
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
                    if connectors
                        .adapters
                        .get(&(
                            requirement.capability.id.clone(),
                            requirement.capability.version,
                        ))
                        .is_some_and(|adapter| {
                            !adapter.connector.accepts_local_service()
                                || adapter.connector.local_binding_mode()
                                    != local.factory.binding_mode(&requirement.capability)
                        })
                    {
                        return Err(RuntimeError::UnsupportedLocalBinding {
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
                    instance,
                    program,
                    args,
                    startup_timeout_ms,
                    shutdown_timeout_ms,
                    ..
                } => {
                    Self::validate_connector(requirement, connectors)?;
                    PlannedTarget::Managed {
                        instance: instance.clone(),
                        program: program.clone(),
                        args: args.clone(),
                        startup_timeout_ms: *startup_timeout_ms,
                        shutdown_timeout_ms: *shutdown_timeout_ms,
                    }
                }
            };
            services.push(PlannedService {
                requirement: requirement.name.clone(),
                capability: requirement.capability.clone(),
                depends_on: requirement.depends_on.clone(),
                expected_provider: deployment
                    .expected_providers
                    .get(&requirement.name)
                    .cloned(),
                target,
            });
        }
        // A declared linked record Query is not two arbitrary capabilities:
        // preflight must prove one selected local registration before any
        // component factory or managed process can start.
        for (model, link) in &schema.record_query_links {
            let read = services
                .iter()
                .find(|service| service.requirement == link.reader)
                .expect("schema validation ensures a declared read requirement");
            let read_provider = match &read.target {
                PlannedTarget::Local { provider } => provider,
                _ => {
                    return Err(RuntimeError::UnsupportedLinkedRecordPlacement {
                        model: model.clone(),
                        requirement: link.reader.clone(),
                    });
                }
            };
            let local = connectors
                .local
                .get(read_provider)
                .expect("local requirement was verified during preflight");
            let read_contract = local
                .contracts
                .get(&(read.capability.id.clone(), read.capability.version))
                .expect("local capability contract was verified during preflight");
            if read_contract.operations.is_empty()
                || read_contract
                    .operations
                    .values()
                    .any(|operation| operation.kind != OperationKind::Query)
            {
                return Err(RuntimeError::InvalidLinkedRecordReader {
                    model: model.clone(),
                    requirement: link.reader.clone(),
                });
            }

            if let Some(writer) = &link.writer {
                let write = services
                    .iter()
                    .find(|service| &service.requirement == writer)
                    .expect("schema validation ensures a declared writer requirement");
                let write_provider = match &write.target {
                    PlannedTarget::Local { provider } => provider,
                    _ => {
                        return Err(RuntimeError::UnsupportedLinkedRecordPlacement {
                            model: model.clone(),
                            requirement: writer.clone(),
                        });
                    }
                };
                if write_provider != read_provider {
                    return Err(RuntimeError::LinkedRecordOwnerMismatch {
                        model: model.clone(),
                        reader: link.reader.clone(),
                        writer: writer.clone(),
                    });
                }
                let write_contract = local
                    .contracts
                    .get(&(write.capability.id.clone(), write.capability.version))
                    .expect("local capability contract was verified during preflight");
                if !write_contract
                    .operations
                    .values()
                    .any(|operation| operation.kind == OperationKind::Command)
                {
                    return Err(RuntimeError::InvalidLinkedRecordWriter {
                        model: model.clone(),
                        requirement: writer.clone(),
                    });
                }
            }
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
        let schema = self.definition();
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
