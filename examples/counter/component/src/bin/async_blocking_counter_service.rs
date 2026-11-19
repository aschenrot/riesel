//! Native async component with host-owned bounded blocking workers.
use riesel_counter_component::blocking_async_counter_provider;
use riesel_protocol::run_component_from_env;

fn main() -> std::io::Result<()> {
    run_component_from_env(|address| {
        blocking_async_counter_provider()
            .map_err(std::io::Error::other)?
            .serve(address)
    })
}
