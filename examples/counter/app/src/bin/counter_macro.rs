//! Minimal Stage 5.6 high-level entrypoint: generated handle and one local
//! component. The same published contract can be hosted in managed mode via
//! an explicit deployment config without changing this source code.
use riesel::{AppResult, app};
use riesel_counter::AsyncCounterHandle;
use riesel_counter_component::AsyncMemoryCounter;

#[app(name = "CounterMacro", components = [AsyncMemoryCounter])]
async fn main(counter: AsyncCounterHandle) -> AppResult<()> {
    assert_eq!(counter.value().await?, 0);
    assert_eq!(counter.add(3).await?, 3);
    assert_eq!(counter.label("count").await?, "count: 3");
    println!("Counter macro: {}", counter.value().await?);
    Ok(())
}
