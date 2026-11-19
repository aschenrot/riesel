//! Fault-injection fixture: the host must join in-flight blocking work
//! after aborting an RPC response on cooperative stdin shutdown.
use riesel_app::{ComponentDefinition, DeclarativeComponent, ProviderBlocking};
use riesel_counter::{AsyncCounter, AsyncCounterContract, CounterError};
use riesel_macros::component;
use riesel_protocol::run_component_from_env;
use std::path::PathBuf;
use std::time::Duration;

#[component(
    id = "riesel.counter.async.shutdown_fixture",
    provides = [AsyncCounterContract],
    factory_with_blocking = build_fixture
)]
struct ShutdownFixture {
    blocking: ProviderBlocking,
    marker: PathBuf,
}

fn build_fixture(blocking: ProviderBlocking) -> ShutdownFixture {
    ShutdownFixture {
        blocking,
        marker: std::env::var_os("RIESEL_TEST_BLOCKING_MARKER")
            .map(PathBuf::from)
            .expect("integration fixture requires a marker path"),
    }
}

impl AsyncCounter for ShutdownFixture {
    async fn value(&self) -> Result<u64, CounterError> {
        Ok(0)
    }

    async fn add(&self, _: u64) -> Result<u64, CounterError> {
        let marker = self.marker.clone();
        self.blocking
            .run("inflight", move |shutdown| {
                std::fs::write(marker.with_extension("started"), b"started")
                    .map_err(|error| CounterError::Internal(error.to_string()))?;
                let cancelled = shutdown.wait_cancelled_timeout(Duration::from_secs(4));
                std::fs::write(marker.with_extension("joined"), b"joined")
                    .map_err(|error| CounterError::Internal(error.to_string()))?;
                if cancelled {
                    Err(CounterError::Internal("provider shutting down".into()))
                } else {
                    Ok(1)
                }
            })
            .await
            .map_err(|error| CounterError::Internal(error.to_string()))?
    }

    async fn label(&self, prefix: &str) -> Result<String, CounterError> {
        Ok(prefix.to_owned())
    }
}

fn main() -> std::io::Result<()> {
    run_component_from_env(|address| {
        let component: ComponentDefinition<ShutdownFixture> =
            ShutdownFixture::component_definition().map_err(std::io::Error::other)?;
        component.serve(address)
    })
}
