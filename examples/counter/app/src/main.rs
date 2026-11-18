use riesel::Application;
use riesel_counter_component::memory_counter_provider;
use riesel_counter_example::run_demo;

fn main() -> riesel::Result<()> {
    Application::new("Counter")
        .handler_with_aliases(run_demo, ["counter"])?
        .provider(
            memory_counter_provider()
                .map_err(|source| riesel::RuntimeError::ProviderFactory { source })?,
        )?
        .run_from_env()
}
