#![cfg(feature = "async-tcp")]
//! End-to-end parity: one declarative native async component provides two
//! capabilities locally and in a manager-owned child. No manual RPC router.

use riesel::{Application, AsyncCapability, DeploymentConfig, ServiceEndpoint};
use riesel_counter::{
    AsyncCounterContract, AsyncCounterHandle, AsyncCounterObserverContract, CounterError,
};
use riesel_counter_component::AsyncMemoryCounter;
use riesel_protocol::{CallError, RpcError};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn exercise(deployment: Option<DeploymentConfig>) {
    let captured: Arc<Mutex<Option<AsyncCounterHandle>>> = Arc::new(Mutex::new(None));
    let escaped = Arc::clone(&captured);
    let prepared = Application::new("NativeAsyncParity")
        .async_handler_with_aliases(
            move |counter: AsyncCapability<AsyncCounterContract>,
                  observer: AsyncCapability<AsyncCounterObserverContract>| {
                let escaped = Arc::clone(&escaped);
                async move {
                    assert_eq!(counter.value().await?, 0);
                    assert_eq!(observer.observe().await?, 0);
                    assert_eq!(counter.add(12).await?, 12);
                    assert_eq!(observer.observe().await?, 12);
                    assert_eq!(counter.label("count").await?, "count: 12");
                    assert!(matches!(
                        counter.add(0).await,
                        Err(CallError::Domain(CounterError::ZeroIncrement))
                    ));
                    assert_eq!(observer.observe().await?, 12);
                    *escaped.lock().unwrap() = Some((*counter).clone());
                    Ok::<_, CallError<CounterError>>(())
                }
            },
            ["counter", "observer"],
        )
        .unwrap()
        .component::<AsyncMemoryCounter>()
        .unwrap();

    let prepared = match deployment {
        Some(config) => prepared.deployment(config),
        None => prepared,
    };
    prepared.run().unwrap();

    // Cloned handles cannot escape shutdown in either execution mode.
    let stale = captured.lock().unwrap().take().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    assert!(matches!(
        runtime.block_on(stale.value()),
        Err(CallError::Rpc(RpcError::SessionExpired))
    ));
}

#[test]
fn generated_native_async_component_runs_locally_with_shared_state() {
    exercise(None);
}

#[test]
fn generated_native_async_component_runs_under_existing_manager() {
    let executable = std::path::PathBuf::from(env!("CARGO_BIN_EXE_async_counter_service"));
    let target = |instance: &str| ServiceEndpoint::Managed {
        instance: Some(instance.into()),
        program: executable.clone(),
        args: Vec::new(),
        startup_timeout_ms: 10_000,
        shutdown_timeout_ms: 2_000,
    };
    let config = DeploymentConfig::new(BTreeMap::from([
        ("counter".into(), target("shared-native-async")),
        ("observer".into(), target("shared-native-async")),
    ]));
    exercise(Some(config));
}
