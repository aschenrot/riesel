//! Modules compose existing requirements and components. Their
//! registration order is not the startup order of capability providers.
use riesel::{AppResult, Application, ApplicationModule, app};
use riesel_counter::{AsyncCounterHandle, AsyncCounterObserverHandle};
use riesel_counter_component::AsyncMemoryCounter;

struct ProviderModule;

impl ApplicationModule for ProviderModule {
    fn register(application: Application) -> riesel::Result<Application> {
        application.component::<AsyncMemoryCounter>()
    }
}

struct DependenciesModule;

impl ApplicationModule for DependenciesModule {
    fn register(application: Application) -> riesel::Result<Application> {
        application.depends_on("observer", ["counter"])
    }
}

#[app(
    name = "CounterModules",
    modules = [DependenciesModule, ProviderModule]
)]
async fn main(
    #[riesel(alias = "counter")] counter: AsyncCounterHandle,
    #[riesel(alias = "observer")] observer: AsyncCounterObserverHandle,
) -> AppResult<()> {
    assert_eq!(counter.add(5).await?, 5);
    assert_eq!(observer.observe().await?, 5);
    println!("Module counter: {}", observer.observe().await?);
    Ok(())
}
