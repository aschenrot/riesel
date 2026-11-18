//! Minimal, transport-neutral capability invocation.
//!
//! The Rust capability contract remains the source of truth. Generated
//! dispatchers and clients use this interface without creating a second
//! hand-maintained method catalogue.

use riesel_schema::{CapabilityContract, CapabilityDescriptor, CapabilityRef, ProviderDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::any::Any;
use std::error::Error;
use std::fmt;
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

/// Server-side adapter generated for a single capability trait.
pub trait RpcHandler: Send + Sync {
    fn contract(&self) -> Result<CapabilityContract, RpcError>;
    fn dispatch(&self, operation: &str, arguments: Value) -> Result<Value, RpcError>;

    fn provider(&self) -> Option<ProviderDefinition> {
        None
    }
}

/// A generated RPC binding translates a transport into a typed service
/// without writing one adapter method for every capability operation.
pub trait RpcClientBinding: CapabilityDescriptor {
    type Service: Any + Send + Sync;

    fn bind(transport: crate::TcpRpcTransport) -> Self::Service;

    /// Every binding must return a session-aware service handle. The
    /// contract may use a generated scoped trait proxy, but it must never
    /// silently pass an unguarded provider through to application code.
    /// A breaking compile-time requirement is safer than stale handles.
    fn guard_local(service: Self::Service, session: crate::SessionToken) -> Self::Service;
}

/// Host-supplied provider identity and generated dispatcher, checked once.
pub struct RpcHost<H> {
    provider: ProviderDefinition,
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
        Ok(Self { provider, handler })
    }
}

impl<H: RpcHandler> RpcHandler for RpcHost<H> {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        self.handler.contract()
    }

    fn dispatch(&self, operation: &str, arguments: Value) -> Result<Value, RpcError> {
        self.handler.dispatch(operation, arguments)
    }

    fn provider(&self) -> Option<ProviderDefinition> {
        Some(self.provider.clone())
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
        let actual = self.handler.contract()?.capability;
        if actual != *capability {
            return Err(RpcError::WrongCapability {
                expected: format!("{}@{}", actual.id, actual.version),
                received: format!("{}@{}", capability.id, capability.version),
            });
        }
        self.handler.dispatch(operation, arguments)
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
