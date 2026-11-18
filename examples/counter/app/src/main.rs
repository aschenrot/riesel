use riesel::Application;
use riesel_counter_component::MemoryCounter;
use riesel_counter_example::run_demo;

fn main() -> riesel::Result<()> {
    Application::new("Counter")
        .handler_with_aliases(run_demo, ["counter"])?
        .component::<MemoryCounter>()?
        .run_from_env()
}
