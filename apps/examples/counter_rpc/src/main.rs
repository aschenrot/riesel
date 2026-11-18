use riesel::Application;
use riesel_counter::{Counter, CounterContract};
use riesel_counter_component::MemoryCounterFactory;

fn main() -> riesel::Result<()> {
    Application::new("Counter")
        .rpc::<CounterContract>("counter")?
        .provider(MemoryCounterFactory)?
        .run(|ctx| {
            let counter = ctx.capability::<CounterContract>("counter")?;
            assert_eq!(counter.value()?, 0);
            assert_eq!(counter.add(3)?, 3);
            assert_eq!(counter.label("count")?, "count: 3");
            Ok(())
        })
}
