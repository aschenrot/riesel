//! Reference Counter implementation. This component provides a versioned
//! Counter capability; the contract is defined independently in riesel_counter.

mod server;

pub use server::run_counter_server;

use riesel_app::{ConnectedProvider, ConnectorError, LocalProviderFactory};
use riesel_counter::{Counter, CounterContract, CounterError};
use riesel_schema::{CapabilityContract, ContractError, ProviderDefinition};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct MemoryCounter {
    value: Mutex<u64>,
}

impl Counter for MemoryCounter {
    fn value(&self) -> Result<u64, CounterError> {
        Ok(*self.value.lock().expect("counter mutex poisoned"))
    }

    fn add(&self, amount: u64) -> Result<u64, CounterError> {
        if amount == 0 {
            return Err(CounterError::ZeroIncrement);
        }
        let mut value = self.value.lock().expect("counter mutex poisoned");
        *value = value.checked_add(amount).ok_or(CounterError::Overflow)?;
        Ok(*value)
    }

    fn label(&self, prefix: &str) -> Result<String, CounterError> {
        Ok(format!("{prefix}: {}", self.value()?))
    }
}

pub fn memory_provider_definition() -> Result<ProviderDefinition, ContractError> {
    Ok(ProviderDefinition {
        id: "riesel.counter.memory".into(),
        capabilities: vec![CounterContract::definition()?.capability],
    })
}

pub struct MemoryCounterFactory;

impl LocalProviderFactory for MemoryCounterFactory {
    fn definition(&self) -> Result<ProviderDefinition, ConnectorError> {
        Ok(memory_provider_definition()?)
    }

    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        Ok(CounterContract::definition()?)
    }

    fn create(&self) -> Result<ConnectedProvider, ConnectorError> {
        Ok(ConnectedProvider {
            definition: memory_provider_definition()?,
            contract: CounterContract::definition()?,
            service: Box::new(Arc::new(MemoryCounter::default()) as Arc<dyn Counter + Send + Sync>),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_provider_matches_independent_contract() {
        let provider = memory_provider_definition().unwrap();
        provider.validate().unwrap();
        let contract = CounterContract::definition().unwrap();
        assert_eq!(provider.capabilities, vec![contract.capability]);
    }

    #[test]
    fn counter_provider_preserves_domain_errors_and_state() {
        let counter = MemoryCounter::default();
        assert_eq!(counter.value().unwrap(), 0);
        assert_eq!(counter.add(7).unwrap(), 7);
        assert_eq!(counter.label("count").unwrap(), "count: 7");
        assert_eq!(counter.add(0), Err(CounterError::ZeroIncrement));
    }
}
