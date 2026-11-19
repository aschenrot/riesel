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
        ("counter".into(), target("shared")),
        ("observer".into(), target("shared")),
    ]));
    exercise(Some(config));
}

struct DropProbe(Arc<std::sync::atomic::AtomicBool>);

impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}

fn exercise_scoped_tasks(deployment: Option<DeploymentConfig>) {
    use riesel::{AsyncTasks, ConnectorError};

    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = Arc::clone(&dropped);
    let prepared = Application::new("AsyncTaskParity")
        .async_handler_with_aliases(
            move |counter: AsyncCapability<AsyncCounterContract>, tasks: AsyncTasks| {
                let flag = Arc::clone(&flag);
                async move {
                    let (sent, received) = tokio::sync::oneshot::channel::<u64>();
                    tasks.spawn("increment", async move {
                        let updated = counter.add(4).await?;
                        let _ = sent.send(updated);
                        Ok(())
                    })?;
                    assert_eq!(received.await.unwrap(), 4);

                    // Pending background work is dropped and joined before the
                    // owner tears down its local or managed provider.
                    let drop_probe = DropProbe(flag);
                    tasks.spawn("pending", async move {
                        let _guard = drop_probe;
                        std::future::pending::<()>().await;
                        Ok(())
                    })?;
                    tokio::task::yield_now().await;
                    Ok::<(), ConnectorError>(())
                }
            },
            ["counter", "tasks"],
        )
        .unwrap()
        .component::<AsyncMemoryCounter>()
        .unwrap();
    match deployment {
        Some(config) => prepared.deployment(config).run().unwrap(),
        None => prepared.run().unwrap(),
    }
    assert!(dropped.load(std::sync::atomic::Ordering::Acquire));
}

#[test]
fn typed_async_background_tasks_share_local_component_and_join_on_return() {
    exercise_scoped_tasks(None);
}

#[test]
fn typed_async_background_tasks_share_managed_component_and_join_on_return() {
    let config = DeploymentConfig::new(BTreeMap::from([(
        "counter".into(),
        ServiceEndpoint::Managed {
            instance: None,
            program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_async_counter_service")),
            args: Vec::new(),
            startup_timeout_ms: 10_000,
            shutdown_timeout_ms: 2_000,
        },
    )]));
    exercise_scoped_tasks(Some(config));
}

#[test]
fn failing_background_task_cancels_pending_async_entrypoint_and_is_reported() {
    use riesel::{AppRunError, AsyncTasks, ConnectorError, SdkError};

    let error = Application::new("AsyncBackgroundFailure")
        .async_handler(|tasks: AsyncTasks| async move {
            tasks.spawn("fatal", async {
                Err(std::io::Error::other("injected background failure").into())
            })?;
            std::future::pending::<Result<(), ConnectorError>>().await
        })
        .unwrap()
        .run()
        .unwrap_err();

    assert!(matches!(
        error,
        SdkError::Execution(AppRunError::TaskFailure { failure, .. })
            if failure.to_string().contains("fatal")
                && failure.to_string().contains("injected background failure")
    ));
}
