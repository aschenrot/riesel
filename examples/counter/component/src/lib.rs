//! Reference Counter implementation. This component provides a versioned
//! Counter capability; the contract is defined independently in riesel_counter.
//!
//! A declared component must actually implement all provided contracts:
//!
//! ```compile_fail
//! use riesel_macros::component;
//! use riesel_counter::CounterContract;
//!
//! #[component(provides = [CounterContract])]
//! #[derive(Default)]
//! struct MissingCounterImplementation;
//! ```

mod server;

pub use server::run_counter_server;

use riesel_app::{ComponentDefinition, ConnectorError, DeclarativeComponent, LocalProviderFactory};
use riesel_counter::{
    Counter, CounterContract, CounterError, CounterObserver, CounterObserverContract,
};
use riesel_macros::component;
use riesel_schema::ProviderDefinition;
use std::sync::Mutex;

#[component(
    id = "riesel.counter.memory",
    provides = [CounterContract, CounterObserverContract]
)]
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

pub fn memory_counter_provider() -> Result<ComponentDefinition<MemoryCounter>, ConnectorError> {
    MemoryCounter::component_definition()
}

/// One authoritative provider definition, produced by the same registration
/// used for local instances. Managed hosting cannot drift from local metadata.
pub fn memory_provider_definition() -> Result<ProviderDefinition, ConnectorError> {
    memory_counter_provider()?.definition()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // A non-Default component may provide an explicit constructor and
    // receive a deterministic private ID without a handwritten factory.
    #[component(provides = [CounterContract], factory = build_private_counter)]
    struct PrivateCounter;

    fn build_private_counter() -> PrivateCounter {
        PrivateCounter
    }

    impl Counter for PrivateCounter {
        fn value(&self) -> Result<u64, CounterError> {
            Ok(0)
        }

        fn add(&self, amount: u64) -> Result<u64, CounterError> {
            Ok(amount)
        }

        fn label(&self, prefix: &str) -> Result<String, CounterError> {
            Ok(format!("{prefix}: 0"))
        }
    }

    #[test]
    fn private_component_uses_generated_identity_and_custom_constructor() {
        let definition = PrivateCounter::component_definition().unwrap();
        let provider = definition.definition().unwrap();
        provider.validate().unwrap();
        assert_eq!(
            provider.id,
            riesel_schema::CapabilityRef::private_rust_id(module_path!(), "PrivateCounter")
        );
        assert_eq!(provider.capabilities, vec![CounterContract::definition().unwrap().capability]);
        assert_eq!(PrivateCounter.value().unwrap(), 0);
    }

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
