//! A high-level application without a statically linked component.
/// Its capability is available only through explicitly configured deployment.
use riesel::{AppResult, app};
use riesel_counter::AsyncCounterHandle;

#[app(name = "CounterRemote")]
async fn main(counter: AsyncCounterHandle) -> AppResult<()> {
    assert_eq!(counter.value().await?, 0);
    assert_eq!(counter.add(3).await?, 3);
    println!("Remote macro: {}", counter.value().await?);
    Ok(())
}
