//! Two explicitly aliased dependencies share one registered component.
use riesel::{AppResult, app};
use riesel_counter::AsyncCounterHandle;
use riesel_counter_component::AsyncMemoryCounter;

#[app(name = "CounterAliased", components = [AsyncMemoryCounter])]
async fn main(
    #[riesel(alias = "primary")] primary: AsyncCounterHandle,
    #[riesel(alias = "secondary")] secondary: AsyncCounterHandle,
) -> AppResult<()> {
    assert_eq!(primary.add(2).await?, 2);
    assert_eq!(secondary.value().await?, 2);
    println!("Aliases share counter: {}", secondary.value().await?);
    Ok(())
}
