//! Managed fixture for the same native async Counter component.
use riesel_counter_component::memory_async_counter_provider;

fn main() -> std::io::Result<()> {
    riesel_protocol::run_component_from_env(|address| {
        memory_async_counter_provider()
            .map_err(std::io::Error::other)?
            .serve(address)
    })
}
