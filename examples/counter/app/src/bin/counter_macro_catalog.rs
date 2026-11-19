//! Stage 5.6: the same high-level application can be locally linked or
//! select one approved managed placement through a selection-only manifest.
use riesel::{AppResult, ProviderCatalog, ServiceEndpoint, app};
use riesel_counter::AsyncCounterHandle;
use riesel_counter_component::AsyncMemoryCounter;

fn approved_counter_providers() -> riesel::Result<ProviderCatalog> {
    let mut catalog = ProviderCatalog::new();
    let mut service = std::env::current_exe()?;
    service.set_file_name(format!(
        "counter_macro_managed_service{}",
        std::env::consts::EXE_SUFFIX
    ));
    catalog.register_component::<AsyncMemoryCounter>(
        "managed",
        ServiceEndpoint::Managed {
            instance: None,
            program: service,
            args: Vec::new(),
            startup_timeout_ms: 10_000,
            shutdown_timeout_ms: 2_000,
        },
    )?;
    Ok(catalog)
}

#[app(
    name = "CounterCatalogMacro",
    components = [AsyncMemoryCounter],
    catalog = approved_counter_providers
)]
async fn main(counter: AsyncCounterHandle) -> AppResult<()> {
    assert_eq!(counter.add(3).await?, 3);
    println!("Catalog counter: {}", counter.value().await?);
    Ok(())
}
