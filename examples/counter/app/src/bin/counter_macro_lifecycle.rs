//! The same module lifecycle operates with linked or managed Counter providers.
use riesel::{AppResult, Application, ApplicationModule, AsyncCapability, app};
use riesel_counter::{AsyncCounterContract, AsyncCounterHandle, CounterError};
use riesel_counter_component::AsyncMemoryCounter;
use riesel_protocol::CallError;

struct CounterLifecycle;

impl ApplicationModule for CounterLifecycle {
    fn register(app: Application) -> riesel::Result<Application> {
        app.on_start_with_aliases(
            "counter_lifecycle",
            |counter: AsyncCapability<AsyncCounterContract>| async move {
                assert_eq!(counter.add(1).await?, 1);
                println!("LIFECYCLE_START");
                Ok::<(), CallError<CounterError>>(())
            },
            ["counter"],
        )?
        .on_stop_with_aliases(
            "counter_lifecycle",
            |counter: AsyncCapability<AsyncCounterContract>| async move {
                assert_eq!(counter.value().await?, 1);
                println!("LIFECYCLE_STOP");
                Ok::<(), CallError<CounterError>>(())
            },
            ["counter"],
        )
    }
}

#[app(
    name = "CounterLifecycle",
    modules = [CounterLifecycle],
    components = [AsyncMemoryCounter]
)]
async fn main(#[riesel(alias = "counter")] counter: AsyncCounterHandle) -> AppResult<()> {
    assert_eq!(counter.value().await?, 1);
    println!("LIFECYCLE_ENTRY");
    Ok(())
}
