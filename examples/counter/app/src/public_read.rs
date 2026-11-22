//! Explicit, optional non-sensitive Counter read facade using the actual
//! SDK-selected provider (not another owned counter or a copied resolver).
use riesel::{Capability, RieselType, capability};
use riesel_counter::{Counter, CounterContract};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType, Error)]
pub enum CounterT1ReadError {
    #[error("Counter observation is unavailable: {0}")]
    Unavailable(String),
}

/// The application must deliberately choose to publish this query-only facet.
/// The original CounterContract has a write command; T1 does not publish it.
#[capability(id = "riesel.counter.public_value", version = 1)]
pub trait CounterT1Read {
    /// Observe the current value from the application's selected provider.
    #[query]
    async fn snapshot(&self) -> Result<u64, CounterT1ReadError>;
}

pub struct CounterT1ReadFacade {
    counter: Capability<CounterContract>,
}

impl CounterT1ReadFacade {
    pub fn new(counter: Capability<CounterContract>) -> Self {
        Self { counter }
    }
}

impl CounterT1Read for CounterT1ReadFacade {
    async fn snapshot(&self) -> Result<u64, CounterT1ReadError> {
        self.counter
            .value()
            .map_err(|error| CounterT1ReadError::Unavailable(error.to_string()))
    }
}
