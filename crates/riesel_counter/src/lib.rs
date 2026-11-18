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
}

impl From<riesel_protocol::RpcError> for CounterError {
    fn from(error: riesel_protocol::RpcError) -> Self {
        Self::Transport(error.to_string())
    }
}

#[capability(id = "riesel.counter.operations", version = 1, client_trait)]
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

