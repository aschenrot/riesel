//! One component declaration for both local and managed execution.
//! A state factory is evaluated once per live component instance. Each
//! capability binding uses that same Arc-backed state in either mode.

use crate::{ConnectorError, LocalProviderFactory, LocalProviderInstance};
use riesel_protocol::{RpcClientBinding, RpcError, RpcHandler, RpcRouter, serve_router};
use riesel_schema::{CapabilityContract, ProviderDefinition};
use std::io;
use std::sync::Arc;

type LocalBinding<S> = Box<
    dyn Fn(Arc<S>, LocalProviderInstance) -> Result<LocalProviderInstance, ConnectorError>
        + Send
        + Sync,
>;
type HostedBinding<S> =
    Box<dyn Fn(Arc<S>, RpcRouter) -> Result<RpcRouter, RpcError> + Send + Sync>;

struct ComponentCapability<S> {
    contract: CapabilityContract,
    local: LocalBinding<S>,
    hosted: HostedBinding<S>,
}

/// Declare component state and all its capability interfaces exactly once.
/// The same instance definition can be registered locally or hosted over
/// RPC; component code does not maintain a separate router registration.
pub struct ComponentDefinition<S> {
    id: String,
    create: Box<dyn Fn() -> S + Send + Sync>,
    capabilities: Vec<ComponentCapability<S>>,
}

impl<S: Send + Sync + 'static> ComponentDefinition<S> {
    pub fn new(id: impl Into<String>, create: impl Fn() -> S + Send + Sync + 'static) -> Self {
        Self {
            id: id.into(),
            create: Box::new(create),
            capabilities: Vec::new(),
        }
    }

    /// Attach one generated capability with its local trait view and generated
    /// dispatcher. Both views receive a clone of the same Arc<S> instance.
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
            local: Box::new(move |state, instance| instance.bind::<C>(local(state))),
            hosted: Box::new(move |state, router| {
                let handler = hosted(state);
                if !handler.contract()?.has_same_api_shape(&expected) {
                    return Err(RpcError::Contract(
                        "component dispatcher does not match its declared capability".into(),
                    ));
                }
                router.route(handler)
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

    /// Launch the same component as a managed RPC host, with one shared state
    /// instance for every declared capability. Routing and metadata derive
    /// from the local registration rather than a second component catalogue.
    pub fn serve(&self, address: &str) -> io::Result<()> {
        let state = Arc::new((self.create)());
        let mut router = RpcRouter::new(self.provider_definition()).map_err(io::Error::other)?;
        for capability in &self.capabilities {
            router = (capability.hosted)(Arc::clone(&state), router)
                .map_err(io::Error::other)?;
        }
        serve_router(address, router)
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

    fn create(&self) -> Result<LocalProviderInstance, ConnectorError> {
        let state = Arc::new((self.create)());
        let mut instance = LocalProviderInstance::new(self.provider_definition());
        for capability in &self.capabilities {
            instance = (capability.local)(Arc::clone(&state), instance)?;
        }
        Ok(instance)
    }
}
