use super::*;

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

pub(super) struct RegisteredConnector {
    pub(super) contract: CapabilityContract,
    pub(super) connector: Box<dyn CapabilityConnector>,
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
    pub(super) fn attach(
        self,
        session: SessionToken,
    ) -> Result<Box<dyn Any + Send + Sync>, ConnectorError> {
        (self.guard)(self.service, session)
    }

    #[cfg(test)]
    pub(super) fn raw(contract: CapabilityContract, service: Box<dyn Any + Send + Sync>) -> Self {
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

pub(super) struct RegisteredLocalProvider {
    pub(super) definition: ProviderDefinition,
    pub(super) contracts: BTreeMap<(String, u32), CapabilityContract>,
    pub(super) factory: Box<dyn LocalProviderFactory>,
}

pub(super) struct RunningLocalProvider {
    pub(super) services: BTreeMap<(String, u32), Arc<dyn Any + Send + Sync>>,
}

/// Explicit runtime adapter registry. Contract IDs (not Rust types or TCP
/// addresses) are the stable lookup keys.
#[derive(Default)]
pub struct ConnectorRegistry {
    pub(super) adapters: BTreeMap<(String, u32), RegisteredConnector>,
    pub(super) local: BTreeMap<String, RegisteredLocalProvider>,
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
