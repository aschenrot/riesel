//! One declarative component for local and managed execution.
//! An instance is constructed once per session; every capability receives
//! the same Arc-backed state in either mode.

#[cfg(feature = "async-tcp")]
use crate::ProviderBlocking;
use crate::{ConnectorError, LocalProviderFactory, LocalProviderInstance};
#[cfg(feature = "async-tcp")]
use riesel_protocol::SessionToken;
use riesel_protocol::{
    AsyncRpcRouter, ComponentAdapter, ComponentHandler, LocalBindingMode, RpcClientBinding,
    RpcError, RpcHandler, RpcRouter, serve_router,
};
use riesel_schema::{CapabilityContract, CapabilityRef, ProviderDefinition};
use std::io;
use std::sync::Arc;

type LocalBinding<S> = Box<
    dyn Fn(Arc<S>, LocalProviderInstance) -> Result<LocalProviderInstance, ConnectorError>
        + Send
        + Sync,
>;
type HostedBinding<S> = Box<dyn Fn(Arc<S>) -> Result<ComponentHandler, RpcError> + Send + Sync>;

struct ComponentCapability<S> {
    contract: CapabilityContract,
    mode: LocalBindingMode,
    local: LocalBinding<S>,
    hosted: HostedBinding<S>,
}

/// A component's one authoritative list of provided capability contracts.
pub trait DeclarativeComponent: Sized + Send + Sync + 'static {
    fn component_definition() -> Result<ComponentDefinition<Self>, ConnectorError>;
}

/// Either an ordinary constructor or a constructor that explicitly requests
/// lifetime-bound provider blocking work. Existing components are unaffected.
#[cfg(feature = "async-tcp")]
enum ComponentFactory<S> {
    Plain(Box<dyn Fn() -> S + Send + Sync>),
    WithBlocking(Box<dyn Fn(ProviderBlocking) -> S + Send + Sync>),
}

#[cfg(not(feature = "async-tcp"))]
type ComponentFactory<S> = Box<dyn Fn() -> S + Send + Sync>;

/// Component state and capability bindings shared by linked and managed modes.
pub struct ComponentDefinition<S> {
    id: String,
    create: ComponentFactory<S>,
    capabilities: Vec<ComponentCapability<S>>,
}

impl<S: Send + Sync + 'static> ComponentDefinition<S> {
    pub fn new(id: impl Into<String>, create: impl Fn() -> S + Send + Sync + 'static) -> Self {
        Self {
            id: id.into(),
            #[cfg(feature = "async-tcp")]
            create: ComponentFactory::Plain(Box::new(create)),
            #[cfg(not(feature = "async-tcp"))]
            create: Box::new(create),
            capabilities: Vec::new(),
        }
    }

    /// Opt into explicit blocking work owned by this component in both linked
    /// and managed modes. Factory injection does not invent a second runtime.
    #[cfg(feature = "async-tcp")]
    pub fn with_blocking(
        id: impl Into<String>,
        create: impl Fn(ProviderBlocking) -> S + Send + Sync + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            create: ComponentFactory::WithBlocking(Box::new(create)),
            capabilities: Vec::new(),
        }
    }

    #[cfg(feature = "async-tcp")]
    fn construct(&self, blocking: ProviderBlocking) -> S {
        match &self.create {
            ComponentFactory::Plain(create) => create(),
            ComponentFactory::WithBlocking(create) => create(blocking),
        }
    }

    #[cfg(not(feature = "async-tcp"))]
    fn construct(&self) -> S {
        (self.create)()
    }

    /// Explicit synchronous adapter for custom service factories. Generated
    /// components should use exposes_contract instead.
    pub fn exposes<C, H>(
        mut self,
        local: impl Fn(Arc<S>) -> C::Service + Send + Sync + 'static,
        hosted: impl Fn(Arc<S>) -> H + Send + Sync + 'static,
    ) -> Result<Self, ConnectorError>
    where
        C: RpcClientBinding + 'static,
        H: RpcHandler + 'static,
    {
        let contract = C::definition()?;
        let expected = contract.clone();
        self.capabilities.push(ComponentCapability {
            contract,
            mode: LocalBindingMode::Sync,
            local: Box::new(move |state, instance| instance.bind::<C>(local(state))),
            hosted: Box::new(move |state| {
                let handler = hosted(state);
                if !handler.contract()?.has_same_api_shape(&expected) {
                    return Err(RpcError::Contract(
                        "component dispatcher does not match its declared capability".into(),
                    ));
                }
                Ok(ComponentHandler::Sync(Box::new(handler)))
            }),
        });
        Ok(self)
    }

    /// Derive local/managed native async or sync bindings from the contract.
    /// The runtime attaches local handles to the active session token.
    pub fn exposes_contract<C>(mut self) -> Result<Self, ConnectorError>
    where
        C: ComponentAdapter<S> + 'static,
    {
        let contract = C::definition()?;
        let expected = contract.clone();
        self.capabilities.push(ComponentCapability {
            contract,
            mode: C::MODE,
            local: Box::new(|state, instance| instance.bind_component::<C, S>(state)),
            hosted: Box::new(move |state| {
                let handler = C::hosted(state);
                if !handler.contract()?.has_same_api_shape(&expected) {
                    return Err(RpcError::Contract(
                        "component dispatcher does not match its declared capability".into(),
                    ));
                }
                Ok(handler)
            }),
        });
        Ok(self)
    }

    fn provider_definition(&self) -> ProviderDefinition {
        ProviderDefinition {
            id: self.id.clone(),
            capabilities: self
                .capabilities
                .iter()
                .map(|binding| binding.contract.capability.clone())
                .collect(),
        }
    }

    /// Host all declared interfaces through one provider-owned process.
    /// Native async components use bounded Tokio I/O and supervised shutdown.
    /// Mixed sync/async components cannot use one blocking service endpoint.
    pub fn serve(&self, address: &str) -> io::Result<()> {
        let provider = self.provider_definition();
        provider.validate().map_err(io::Error::other)?;
        if self
            .capabilities
            .windows(2)
            .any(|pair| pair[0].mode != pair[1].mode)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mixed sync and async capabilities require separate managed endpoints",
            ));
        }
        #[cfg(feature = "async-tcp")]
        if matches!(self.create, ComponentFactory::WithBlocking(_))
            && self
                .capabilities
                .iter()
                .any(|capability| capability.mode != LocalBindingMode::Async)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "provider blocking work requires native async capability hosting",
            ));
        }
        let mut sync_router = RpcRouter::new(provider.clone()).map_err(io::Error::other)?;
        let mut async_router = AsyncRpcRouter::new(provider).map_err(io::Error::other)?;
        #[cfg(feature = "async-tcp")]
        let blocking = ProviderBlocking::new(SessionToken::new());
        #[cfg(feature = "async-tcp")]
        let state = Arc::new(self.construct(blocking.clone()));
        #[cfg(not(feature = "async-tcp"))]
        let state = Arc::new(self.construct());
        let mut sync = false;
        let mut asynchronous = false;

        for capability in &self.capabilities {
            match (capability.hosted)(Arc::clone(&state)).map_err(io::Error::other)? {
                ComponentHandler::Sync(handler) => {
                    sync = true;
                    sync_router = sync_router.route(handler).map_err(io::Error::other)?;
                }
                ComponentHandler::Async(handler) => {
                    asynchronous = true;
                    async_router = async_router.route(handler).map_err(io::Error::other)?;
                }
            }
        }
        if sync && asynchronous {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "mixed sync and async capabilities require separate managed endpoints",
            ));
        }
        if asynchronous {
            async_router.validate_routes().map_err(io::Error::other)?;
            #[cfg(feature = "async-tcp")]
            {
                let served =
                    riesel_protocol::run_async_tcp_rpc_host(address, Arc::new(async_router));
                // Aborted RPC futures may have started blocking OS work.
                // Join it while this scope still owns the provider state.
                let joined = blocking.close_and_join();
                return served.and(joined);
            }
            #[cfg(not(feature = "async-tcp"))]
            {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "enable async-tcp to host native async components",
                ));
            }
        }
        let served = serve_router(address, sync_router);
        #[cfg(feature = "async-tcp")]
        {
            return served.and(blocking.close_and_join());
        }
        #[cfg(not(feature = "async-tcp"))]
        served
    }
}

impl<S: Send + Sync + 'static> LocalProviderFactory for ComponentDefinition<S> {
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
        Ok(self.provider_definition())
    }

    fn contracts(&self) -> Result<Vec<CapabilityContract>, ConnectorError> {
        Ok(self
            .capabilities
            .iter()
            .map(|binding| binding.contract.clone())
            .collect())
    }

    fn binding_mode(&self, capability: &CapabilityRef) -> LocalBindingMode {
        self.capabilities
            .iter()
            .find(|binding| binding.contract.capability == *capability)
            .map_or(LocalBindingMode::Sync, |binding| binding.mode)
    }

    fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
        #[cfg(feature = "async-tcp")]
        {
            return self.create_with_session(SessionToken::new());
        }
        #[cfg(not(feature = "async-tcp"))]
        {
            let state = Arc::new(self.construct());
            let mut instance = LocalProviderInstance::new(self.provider_definition());
            for capability in &self.capabilities {
                instance = (capability.local)(Arc::clone(&state), instance)?;
            }
            Ok(instance)
        }
    }

    #[cfg(feature = "async-tcp")]
    fn create_with_session(
        &self,
        session: SessionToken,
    ) -> Result<LocalProviderInstance, ConnectorError> {
        let blocking = ProviderBlocking::new(session);
        let state = Arc::new(self.construct(blocking.clone()));
        let mut instance =
            LocalProviderInstance::new(self.provider_definition()).with_blocking(blocking);
        for capability in &self.capabilities {
            instance = (capability.local)(Arc::clone(&state), instance)?;
        }
        Ok(instance)
    }
}
