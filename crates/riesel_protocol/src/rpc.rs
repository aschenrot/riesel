//! Minimal, transport-neutral capability invocation.
//!
//! The Rust capability contract remains the source of truth. Generated
//! dispatchers and clients use this interface without creating a second
//! hand-maintained method catalogue.

use riesel_schema::{CapabilityContract, CapabilityDescriptor, CapabilityRef, ProviderDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::any::Any;
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;

/// Failures outside the domain method's declared error type.
#[derive(Debug, Error, PartialEq, Eq, Serialize, Deserialize)]
pub enum RpcError {
    #[error("RPC encoding or decoding failed: {0}")]
    Codec(String),
    #[error("invalid arguments: {0}")]
    InvalidArguments(String),
    #[error("unknown operation: {0}")]
    UnknownOperation(String),
    #[error("wrong capability: expected {expected}, received {received}")]
    WrongCapability { expected: String, received: String },
    #[error("capability contract cannot be constructed: {0}")]
    Contract(String),
    #[error("RPC transport failed: {0}")]
    Transport(String),
    #[error("capability handle belongs to an expired application session")]
    SessionExpired,
}

/// A domain error is distinct from a wire, codec, or transport failure.
/// This is why generated clients do not silently implement the original
/// capability trait: its method error type cannot represent arbitrary
/// transport failures without an explicit conversion policy.
#[derive(Debug)]
pub enum CallError<E> {
    Rpc(RpcError),
    Domain(E),
}

impl<E: fmt::Display> fmt::Display for CallError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rpc(error) => write!(f, "{error}"),
            Self::Domain(error) => write!(f, "{error}"),
        }
    }
}

impl<E: Error + 'static> Error for CallError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Rpc(error) => Some(error),
            Self::Domain(error) => Some(error),
        }
    }
}

impl<E> From<RpcError> for CallError<E> {
    fn from(error: RpcError) -> Self {
        Self::Rpc(error)
    }
}

/// A transport invokes one named operation of a versioned capability.
/// Implementations can use in-memory dispatch, TCP, pipes, etc.
pub trait RpcTransport: Send + Sync {
    fn call(
        &self,
        capability: &CapabilityRef,
        operation: &str,
        arguments: Value,
    ) -> Result<Value, RpcError>;
}

/// A shared transport session can be passed into any generated typed binding.
/// Arc forwarding avoids TCP-specific generic parameters at SDK boundaries.
impl<T: RpcTransport + ?Sized> RpcTransport for Arc<T> {
    fn call(
        &self,
        capability: &CapabilityRef,
        operation: &str,
        arguments: Value,
    ) -> Result<Value, RpcError> {
        self.as_ref().call(capability, operation, arguments)
    }
}

/// A nonblocking transport that can be used by generated typed async clients.
///
/// Futures are *not* a wrapper around the synchronous RpcTransport. Each
/// implementation must perform I/O without blocking the calling executor.
/// Dropping the returned future cancels its pending request; callers must not
/// infer whether a previously sent command executed.
pub trait AsyncRpcTransport: Send + Sync {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityRef,
        operation: &'a str,
        arguments: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send + 'a>>;
}

impl<T: AsyncRpcTransport + ?Sized> AsyncRpcTransport for Arc<T> {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityRef,
        operation: &'a str,
        arguments: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send + 'a>> {
        self.as_ref().call(capability, operation, arguments)
    }
}

/// The generated typed async binding for one versioned contract. A transport
/// instance is explicit and may be tied to a revocable application session.
/// A generated client keeps domain and RPC errors separate.
pub trait AsyncRpcClientBinding: CapabilityDescriptor {
    type Service: Send + Sync + 'static;

    fn bind_async(transport: Arc<dyn AsyncRpcTransport>) -> Self::Service;
}

/// A generated native async capability handle that can be injected directly
/// into an application, without selecting its transport or repeating the
/// published contract identifier.
pub trait NativeAsyncHandle: Clone + Send + Sync + 'static {
    type Contract: AsyncRpcClientBinding<Service = Self> + 'static;
}

/// A Send future returned by native async capability dispatch.
pub type AsyncRpcFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send + 'a>>;

/// Handler for a native asynchronous capability. Its generated implementation
/// invokes normal Rust async methods without blocking a runtime worker.
pub trait AsyncRpcHandler: Send + Sync {
    fn contract(&self) -> Result<CapabilityContract, RpcError>;

    /// Advertised provider metadata is optional and is not authentication.
    fn provider(&self) -> Option<ProviderDefinition> {
        None
    }

    fn dispatch<'a>(&'a self, operation: &'a str, arguments: Value) -> AsyncRpcFuture<'a>;

    /// Validate the complete hosted declaration before accepting connections.
    fn validate_host(&self) -> Result<(), RpcError> {
        self.contract()?
            .validate()
            .map_err(|error| RpcError::Contract(error.to_string()))
    }

    /// Select the contract advertised at a multiplexed endpoint.
    fn contract_for(&self, capability: &CapabilityRef) -> Result<CapabilityContract, RpcError> {
        let contract = self.contract()?;
        if &contract.capability == capability {
            Ok(contract)
        } else {
            Err(RpcError::WrongCapability {
                expected: format!("{}@{}", contract.capability.id, contract.capability.version),
                received: format!("{}@{}", capability.id, capability.version),
            })
        }
    }

    fn dispatch_for<'a>(
        &'a self,
        capability: &'a CapabilityRef,
        operation: &'a str,
        arguments: Value,
    ) -> AsyncRpcFuture<'a> {
        Box::pin(async move {
            self.contract_for(capability)?;
            self.dispatch(operation, arguments).await
        })
    }
}

/// Server-side adapter generated for a single capability trait.
pub trait RpcHandler: Send + Sync {
    fn contract(&self) -> Result<CapabilityContract, RpcError>;
    fn dispatch(&self, operation: &str, arguments: Value) -> Result<Value, RpcError>;

    fn provider(&self) -> Option<ProviderDefinition> {
        None
    }

    /// Resolve the exact capability served at this endpoint. Implementations
    /// that multiplex capability interfaces override this operation.
    fn contract_for(&self, capability: &CapabilityRef) -> Result<CapabilityContract, RpcError> {
        let contract = self.contract()?;
        if &contract.capability == capability {
            Ok(contract)
        } else {
            Err(RpcError::WrongCapability {
                expected: format!("{}@{}", contract.capability.id, contract.capability.version),
                received: format!("{}@{}", capability.id, capability.version),
            })
        }
    }

    fn dispatch_for(
        &self,
        capability: &CapabilityRef,
        operation: &str,
        arguments: Value,
    ) -> Result<Value, RpcError> {
        self.contract_for(capability)?;
        self.dispatch(operation, arguments)
    }
}

/// A generated RPC binding translates a transport into a typed service
/// without writing one adapter method for every capability operation.
///
/// Bindings that omit session guarding are rejected at compile time:
///
/// ```compile_fail
/// use riesel_protocol::{RpcClientBinding, RpcTransport};
/// use riesel_schema::{CapabilityContract, CapabilityDescriptor, ContractError};
///
/// struct UnsafeBinding;
/// impl CapabilityDescriptor for UnsafeBinding {
///     fn definition() -> Result<CapabilityContract, ContractError> {
///         unimplemented!()
///     }
/// }
/// impl RpcClientBinding for UnsafeBinding {
///     type Service = ();
///     fn bind(_: std::sync::Arc<dyn RpcTransport>) -> Self::Service {}
///     // Missing guard_local must be a compilation error.
/// }
/// ```
pub trait RpcClientBinding: CapabilityDescriptor {
    type Service: Any + Send + Sync;

    fn bind(transport: Arc<dyn RpcTransport>) -> Self::Service;

    /// Every binding must return a session-aware service handle. The
    /// contract may use a generated scoped trait proxy, but it must never
    /// silently pass an unguarded provider through to application code.
    /// A breaking compile-time requirement is safer than stale handles.
    fn guard_local(service: Self::Service, session: crate::SessionToken) -> Self::Service;
}

/// Kind of typed local service supplied by a provider. Checked at preflight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalBindingMode {
    Sync,
    Async,
}

/// Hosted view generated from one capability declaration.
pub enum ComponentHandler {
    Sync(Box<dyn RpcHandler>),
    Async(Box<dyn AsyncRpcHandler>),
}

impl ComponentHandler {
    pub fn contract(&self) -> Result<CapabilityContract, RpcError> {
        match self {
            Self::Sync(handler) => handler.contract(),
            Self::Async(handler) => handler.contract(),
        }
    }
}

/// Generated adapter sharing one state between local and hosted execution.
pub trait ComponentAdapter<S>: CapabilityDescriptor {
    type Service: Any + Send + Sync + 'static;
    const MODE: LocalBindingMode;

    fn local(state: Arc<S>, session: crate::SessionToken) -> Self::Service;
    fn hosted(state: Arc<S>) -> ComponentHandler;
}

impl<T: RpcHandler + ?Sized> RpcHandler for Box<T> {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        self.as_ref().contract()
    }

    fn dispatch(&self, operation: &str, arguments: Value) -> Result<Value, RpcError> {
        self.as_ref().dispatch(operation, arguments)
    }

    fn provider(&self) -> Option<ProviderDefinition> {
        self.as_ref().provider()
    }

    fn contract_for(&self, capability: &CapabilityRef) -> Result<CapabilityContract, RpcError> {
        self.as_ref().contract_for(capability)
    }

    fn dispatch_for(
        &self,
        capability: &CapabilityRef,
        operation: &str,
        arguments: Value,
    ) -> Result<Value, RpcError> {
        self.as_ref().dispatch_for(capability, operation, arguments)
    }
}

impl<T: AsyncRpcHandler + ?Sized> AsyncRpcHandler for Box<T> {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        self.as_ref().contract()
    }

    fn provider(&self) -> Option<ProviderDefinition> {
        self.as_ref().provider()
    }

    fn validate_host(&self) -> Result<(), RpcError> {
        self.as_ref().validate_host()
    }

    fn contract_for(&self, capability: &CapabilityRef) -> Result<CapabilityContract, RpcError> {
        self.as_ref().contract_for(capability)
    }

    fn dispatch<'a>(&'a self, operation: &'a str, arguments: Value) -> AsyncRpcFuture<'a> {
        self.as_ref().dispatch(operation, arguments)
    }

    fn dispatch_for<'a>(
        &'a self,
        capability: &'a CapabilityRef,
        operation: &'a str,
        arguments: Value,
    ) -> AsyncRpcFuture<'a> {
        self.as_ref().dispatch_for(capability, operation, arguments)
    }
}

/// Host-supplied provider identity and generated dispatcher, checked once.
pub struct RpcHost<H> {
    provider: ProviderDefinition,
    contract: CapabilityContract,
    handler: H,
}

impl<H: RpcHandler> RpcHost<H> {
    pub fn new(provider: ProviderDefinition, handler: H) -> Result<Self, RpcError> {
        provider
            .validate()
            .map_err(|error| RpcError::Contract(error.to_string()))?;
        let contract = handler.contract()?;
        contract
            .validate()
            .map_err(|error| RpcError::Contract(error.to_string()))?;
        if !provider.capabilities.contains(&contract.capability) {
            return Err(RpcError::Contract(
                "provider does not advertise the hosted capability".into(),
            ));
        }
        Ok(Self {
            provider,
            contract,
            handler,
        })
    }
}

impl<H: RpcHandler> RpcHandler for RpcHost<H> {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        Ok(self.contract.clone())
    }

    fn dispatch(&self, operation: &str, arguments: Value) -> Result<Value, RpcError> {
        self.handler.dispatch(operation, arguments)
    }

    fn provider(&self) -> Option<ProviderDefinition> {
        Some(self.provider.clone())
    }
}

/// A nonblocking, multi-capability host for one provider-owned instance.
/// Native async dispatchers can share an Arc-backed state, process and port.
/// The router validates exact advertised contract versions before listening.
pub struct AsyncRpcRouter {
    provider: ProviderDefinition,
    routes: BTreeMap<(String, u32), (CapabilityContract, Box<dyn AsyncRpcHandler>)>,
}

impl AsyncRpcRouter {
    pub fn new(provider: ProviderDefinition) -> Result<Self, RpcError> {
        provider
            .validate()
            .map_err(|error| RpcError::Contract(error.to_string()))?;
        Ok(Self {
            provider,
            routes: BTreeMap::new(),
        })
    }

    pub fn route(mut self, handler: impl AsyncRpcHandler + 'static) -> Result<Self, RpcError> {
        let contract = handler.contract()?;
        contract
            .validate()
            .map_err(|error| RpcError::Contract(error.to_string()))?;
        if !self.provider.capabilities.contains(&contract.capability) {
            return Err(RpcError::Contract(
                "handler capability is not advertised by provider".into(),
            ));
        }
        let key = (contract.capability.id.clone(), contract.capability.version);
        if self.routes.contains_key(&key) {
            return Err(RpcError::Contract(
                "provider capability registered more than once".into(),
            ));
        }
        self.routes.insert(key, (contract, Box::new(handler)));
        Ok(self)
    }

    /// Reject incomplete registrations before a host starts accepting calls.
    pub fn validate_routes(&self) -> Result<(), RpcError> {
        if self.routes.len() != self.provider.capabilities.len() {
            return Err(RpcError::Contract(
                "provider has an unbound capability".into(),
            ));
        }
        Ok(())
    }

    fn route_for(
        &self,
        capability: &CapabilityRef,
    ) -> Result<&(CapabilityContract, Box<dyn AsyncRpcHandler>), RpcError> {
        self.routes
            .get(&(capability.id.clone(), capability.version))
            .ok_or_else(|| RpcError::WrongCapability {
                expected: self
                    .provider
                    .capabilities
                    .iter()
                    .map(|cap| format!("{}@{}", cap.id, cap.version))
                    .collect::<Vec<_>>()
                    .join(", "),
                received: format!("{}@{}", capability.id, capability.version),
            })
    }
}

impl AsyncRpcHandler for AsyncRpcRouter {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        if self.routes.len() != 1 {
            return Err(RpcError::Contract(
                "multi-capability async router requires a capability selector".into(),
            ));
        }
        Ok(self.routes.values().next().expect("one route").0.clone())
    }

    fn validate_host(&self) -> Result<(), RpcError> {
        self.validate_routes()
    }

    fn contract_for(&self, capability: &CapabilityRef) -> Result<CapabilityContract, RpcError> {
        Ok(self.route_for(capability)?.0.clone())
    }

    fn dispatch<'a>(&'a self, operation: &'a str, arguments: Value) -> AsyncRpcFuture<'a> {
        Box::pin(async move {
            if self.routes.len() != 1 {
                return Err(RpcError::Contract(
                    "multi-capability async router requires a capability selector".into(),
                ));
            }
            self.routes
                .values()
                .next()
                .expect("one route")
                .1
                .dispatch(operation, arguments)
                .await
        })
    }

    fn dispatch_for<'a>(
        &'a self,
        capability: &'a CapabilityRef,
        operation: &'a str,
        arguments: Value,
    ) -> AsyncRpcFuture<'a> {
        Box::pin(async move {
            self.route_for(capability)?
                .1
                .dispatch(operation, arguments)
                .await
        })
    }

    fn provider(&self) -> Option<ProviderDefinition> {
        Some(self.provider.clone())
    }
}

/// Validated provider instance serving multiple versioned capability contracts
/// through one shared transport endpoint and lifecycle. Handlers can share
/// the same Arc-backed implementation state.
pub struct RpcRouter {
    provider: ProviderDefinition,
    routes: BTreeMap<(String, u32), (CapabilityContract, Box<dyn RpcHandler>)>,
}

impl RpcRouter {
    pub fn new(provider: ProviderDefinition) -> Result<Self, RpcError> {
        provider
            .validate()
            .map_err(|error| RpcError::Contract(error.to_string()))?;
        Ok(Self {
            provider,
            routes: BTreeMap::new(),
        })
    }

    pub fn route(mut self, handler: impl RpcHandler + 'static) -> Result<Self, RpcError> {
        let contract = handler.contract()?;
        contract
            .validate()
            .map_err(|error| RpcError::Contract(error.to_string()))?;
        if !self.provider.capabilities.contains(&contract.capability) {
            return Err(RpcError::Contract(
                "handler capability is not advertised by provider".into(),
            ));
        }
        let key = (contract.capability.id.clone(), contract.capability.version);
        if self
            .routes
            .insert(key, (contract, Box::new(handler)))
            .is_some()
        {
            return Err(RpcError::Contract(
                "provider capability registered more than once".into(),
            ));
        }
        Ok(self)
    }

    /// Fail before listening if the provider advertises an interface without
    /// an executable handler.
    pub fn validate_routes(&self) -> Result<(), RpcError> {
        if self.routes.len() != self.provider.capabilities.len() {
            return Err(RpcError::Contract(
                "provider has an unbound capability".into(),
            ));
        }
        Ok(())
    }

    fn route_for(
        &self,
        capability: &CapabilityRef,
    ) -> Result<&(CapabilityContract, Box<dyn RpcHandler>), RpcError> {
        self.routes
            .get(&(capability.id.clone(), capability.version))
            .ok_or_else(|| RpcError::WrongCapability {
                expected: self
                    .provider
                    .capabilities
                    .iter()
                    .map(|cap| format!("{}@{}", cap.id, cap.version))
                    .collect::<Vec<_>>()
                    .join(", "),
                received: format!("{}@{}", capability.id, capability.version),
            })
    }
}

impl RpcHandler for RpcRouter {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        // Legacy single-capability call sites are explicit; production routing
        // uses contract_for() instead.
        if self.routes.len() != 1 {
            return Err(RpcError::Contract(
                "multi-capability router requires a capability selector".into(),
            ));
        }
        Ok(self.routes.values().next().expect("one route").0.clone())
    }

    fn dispatch(&self, operation: &str, arguments: Value) -> Result<Value, RpcError> {
        if self.routes.len() != 1 {
            return Err(RpcError::Contract(
                "multi-capability router requires a capability selector".into(),
            ));
        }
        self.routes
            .values()
            .next()
            .expect("one route")
            .1
            .dispatch(operation, arguments)
    }

    fn provider(&self) -> Option<ProviderDefinition> {
        Some(self.provider.clone())
    }

    fn contract_for(&self, capability: &CapabilityRef) -> Result<CapabilityContract, RpcError> {
        Ok(self.route_for(capability)?.0.clone())
    }

    fn dispatch_for(
        &self,
        capability: &CapabilityRef,
        operation: &str,
        arguments: Value,
    ) -> Result<Value, RpcError> {
        self.route_for(capability)?.1.dispatch(operation, arguments)
    }
}

/// In-process transport for testing and applications without isolation.
pub struct LocalRpc<H> {
    handler: H,
}

impl<H> LocalRpc<H> {
    pub fn new(handler: H) -> Self {
        Self { handler }
    }

    pub fn into_inner(self) -> H {
        self.handler
    }
}

impl<H: RpcHandler> RpcTransport for LocalRpc<H> {
    fn call(
        &self,
        capability: &CapabilityRef,
        operation: &str,
        arguments: Value,
    ) -> Result<Value, RpcError> {
        self.handler.dispatch_for(capability, operation, arguments)
    }
}

/// Decode the serialized domain result without losing its error type.
pub fn decode_result<T: DeserializeOwned, E: DeserializeOwned>(
    value: Value,
) -> Result<T, CallError<E>> {
    let result: Result<T, E> = serde_json::from_value(value)
        .map_err(|error| CallError::Rpc(RpcError::Codec(error.to_string())))?;
    result.map_err(CallError::Domain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_domain_errors_without_treating_them_as_transport_failures() {
        let value = serde_json::to_value(Result::<u64, String>::Err("rejected".into())).unwrap();
        assert!(matches!(
            decode_result::<u64, String>(value),
            Err(CallError::Domain(message)) if message == "rejected"
        ));
        assert!(matches!(
            decode_result::<u64, String>(Value::Null),
            Err(CallError::Rpc(RpcError::Codec(_)))
        ));
    }
}
