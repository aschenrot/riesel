//! Counter example: separate capability contract and provider component,
//! generated RPC and both local/managed Riesel execution paths.
#[cfg(feature = "t1-public-read")]
pub mod public_read;
pub use riesel_counter::{
    Counter, CounterContract, CounterError, CounterRpcClient, CounterRpcDispatcher,
};
pub use riesel_counter_component::MemoryCounter;

/// Shared example work for local and runtime-managed deployments.
pub fn run_demo(counter: riesel::Capability<CounterContract>) -> Result<(), CounterError> {
    assert_eq!(counter.value()?, 0);
    assert_eq!(counter.add(3)?, 3);
    assert_eq!(counter.label("count")?, "count: 3");
    Ok(())
}

#[cfg(test)]
mod tests;
