//! Two capabilities bound to one explicitly named managed component
//! must read and mutate one shared Counter state.
use riesel::{Application, Capability};
use riesel_counter::{Counter, CounterContract, CounterError, CounterObserver, CounterObserverContract};

fn verify_shared_state(
    counter: Capability<CounterContract>,
    observer: Capability<CounterObserverContract>,
) -> Result<(), CounterError> {
    assert_eq!(observer.observe()?, 0);
    assert_eq!(counter.add(17)?, 17);
    assert_eq!(observer.observe()?, 17);
    println!("RIESEL_SHARED_COMPONENT_OK");
    Ok(())
}

fn main() -> riesel::Result<()> {
    Application::new("DualManaged")
        .handler_with_aliases(verify_shared_state, ["counter", "observer"])?
        .run_from_env()
}
