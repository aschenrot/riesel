//! Managed native async Counter and Observer provider.
//! The generated component owns registration, readiness and shutdown.
use riesel_counter_component::memory_async_counter_provider;
use riesel_protocol::run_component_from_env;

fn main() -> std::io::Result<()> {
    run_component_from_env(|address| {
        memory_async_counter_provider()
            .map_err(std::io::Error::other)?
            .serve(address)
    })
}
