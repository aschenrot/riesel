use riesel::Application;
use riesel_counter::CounterContract;
use riesel_counter_example::run_demo;

fn main() -> riesel::Result<()> {
    Application::new("Counter")
        .rpc::<CounterContract>("counter")?
        .run_from_env(run_demo)
}
