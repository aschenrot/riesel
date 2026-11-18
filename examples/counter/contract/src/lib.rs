//! Versioned Counter capability contract. The implementation lives in
//! components/counter and can be replaced without modifying callers.

use riesel_macros::{RieselType, capability};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize, RieselType)]
pub enum CounterError {
    #[error("increment must be greater than zero")]
    ZeroIncrement,
    #[error("counter would overflow")]
    Overflow,
    #[error("counter transport failed: {0}")]
    Transport(String),
    #[error("counter internal failure: {0}")]
    Internal(String),
}

impl From<riesel_protocol::RpcError> for CounterError {
    fn from(error: riesel_protocol::RpcError) -> Self {
        Self::Transport(error.to_string())
    }
}

#[capability(id = "riesel.counter.operations", version = 2, client_trait)]
pub trait Counter {
    /// Return the current count without modifying it.
    #[query]
    fn value(&self) -> Result<u64, CounterError>;

    /// Increment the counter with overflow protection.
    #[command]
    fn add(&self, amount: u64) -> Result<u64, CounterError>;

    /// Return a string label built from the current count.
    #[query]
    fn label(&self, prefix: &str) -> Result<String, CounterError>;
}

/// Independent read-only view over the same Counter instance. A managed
/// component can expose this alongside Counter without spawning another
/// process or creating a second copy of state.
#[capability(id = "riesel.counter.observer", version = 1, client_trait)]
pub trait CounterObserver {
    /// Observe the current count without changing it.
    #[query]
    fn observe(&self) -> Result<u64, CounterError>;
}

#[cfg(test)]
#[allow(dead_code)]
mod private_identity_tests {
    use super::*;

    #[capability]
    pub trait PrivateCounter {
        /// Read the counter.
        #[query]
        fn value(&self) -> Result<u64, CounterError>;
    }

    struct PrivateImplementation;

    impl PrivateCounter for PrivateImplementation {
        fn value(&self) -> Result<u64, CounterError> {
            Ok(7)
        }
    }

    #[test]
    fn private_declaration_needs_no_id_and_supports_generated_dispatch() {
        use riesel_protocol::RpcHandler;

        let contract = PrivateCounterContract::definition().unwrap();
        assert_eq!(contract.capability.version, 1);
        assert_eq!(
            contract.capability.id,
            riesel_schema::CapabilityRef::private_rust_id(module_path!(), "PrivateCounter")
        );
        let handler = PrivateCounterRpcDispatcher::new(PrivateImplementation);
        assert_eq!(handler.contract().unwrap().capability, contract.capability);
        assert!(handler.dispatch("value", serde_json::json!({})).is_ok());
    }
}
