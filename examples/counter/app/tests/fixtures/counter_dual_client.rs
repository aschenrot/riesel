//! Two capabilities bound to one explicitly named managed component
//! must read and mutate one shared Counter state.
use riesel::Application;
use riesel_counter::{CounterContract, CounterObserverContract};

fn main() -> riesel::Result<()> {
    Application::new("DualManaged")
        .rpc::<CounterContract>("counter")?
        .rpc::<CounterObserverContract>("observer")?
        .run_from_env(|ctx| {
            let counter = ctx.capability::<CounterContract>("counter")?;
            let observer = ctx.capability::<CounterObserverContract>("observer")?;
            assert_eq!(observer.observe()?, 0);
            assert_eq!(counter.add(17)?, 17);
            assert_eq!(observer.observe()?, 17);
            println!("RIESEL_SHARED_COMPONENT_OK");
            Ok(())
        })
}
