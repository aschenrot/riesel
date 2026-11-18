//! Reference Counter implementation. This component provides a versioned
//! Counter capability; the contract is defined independently in riesel_counter.

mod server;

pub use server::run_counter_server;

use riesel_app::{
    ConnectorError, LocalComponentFactory, LocalProviderFactory, LocalProviderInstance,
};
use riesel_counter::{
    Counter, CounterContract, CounterError, CounterObserver, CounterObserverContract,
};
use riesel_schema::ProviderDefinition;
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct MemoryCounter {
    value: Mutex<u64>,
}

impl Counter for MemoryCounter {
    fn value(&self) -> Result<u64, CounterError> {
        Ok(*self
            .value
            .lock()
            .map_err(|_| CounterError::Internal("counter lock poisoned".into()))?)
    }

    fn add(&self, amount: u64) -> Result<u64, CounterError> {
        if amount == 0 {
            return Err(CounterError::ZeroIncrement);
        }
        let mut value = self
            .value
            .lock()
            .map_err(|_| CounterError::Internal("counter lock poisoned".into()))?;
        *value = value.checked_add(amount).ok_or(CounterError::Overflow)?;
        Ok(*value)
    }

    fn label(&self, prefix: &str) -> Result<String, CounterError> {
        Ok(format!("{prefix}: {}", self.value()?))
    }
}

impl CounterObserver for MemoryCounter {
    fn observe(&self) -> Result<u64, CounterError> {
        Counter::value(self)
    }
}

pub fn memory_counter_provider() -> Result<impl LocalProviderFactory, ConnectorError> {
    LocalComponentFactory::new(
        "riesel.counter.memory",
        |definition: &ProviderDefinition| {
            let state = Arc::new(MemoryCounter::default());
            LocalProviderInstance::new(definition.clone())
                .bind::<CounterContract>(Arc::clone(&state) as Arc<dyn Counter + Send + Sync>)?
                .bind::<CounterObserverContract>(state as Arc<dyn CounterObserver + Send + Sync>)
        },
    )
    .exposes::<CounterContract>()?
    .exposes::<CounterObserverContract>()
}

/// One authoritative provider definition, produced by the same registration
/// used for local instances. Managed hosting cannot drift from local metadata.
pub fn memory_provider_definition() -> Result<ProviderDefinition, ConnectorError> {
    memory_counter_provider()?.definition()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_provider_matches_independent_contract() {
        let provider = memory_provider_definition().unwrap();
        provider.validate().unwrap();
        let contract = CounterContract::definition().unwrap();
        assert_eq!(
            provider.capabilities,
            vec![
                contract.capability,
                CounterObserverContract::definition().unwrap().capability
            ]
        );
    }

    #[test]
    fn counter_provider_preserves_domain_errors_and_state() {
        let counter = MemoryCounter::default();
        assert_eq!(counter.value().unwrap(), 0);
        assert_eq!(counter.add(7).unwrap(), 7);
        assert_eq!(counter.label("count").unwrap(), "count: 7");
        assert_eq!(counter.observe().unwrap(), 7);
        let shared = Arc::new(MemoryCounter::default());
        assert_eq!(riesel_counter::Counter::add(&shared, 2).unwrap(), 2);
        assert_eq!(counter.add(0), Err(CounterError::ZeroIncrement));
    }
}
