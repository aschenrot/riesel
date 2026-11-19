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
fn failing_background_task_cancels_entrypoint_without_replaying_it_on_recovery() {
    use riesel::{AppRunError, AsyncTasks, ConnectorError, SdkError};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let executions = Arc::new(AtomicUsize::new(0));
    let invoked = Arc::clone(&executions);
    let mut deployment = DeploymentConfig::new(BTreeMap::new());
    deployment.recovery = riesel_app::RecoveryPolicy::RestartApplication {
        max_restarts: 3,
        initial_backoff_ms: 10,
        max_backoff_ms: 40,
    };
    let error = Application::new("AsyncBackgroundFailure")
        .async_handler(move |tasks: AsyncTasks| {
            invoked.fetch_add(1, Ordering::SeqCst);
            async move {
                tasks.spawn("fatal", async {
                    Err(std::io::Error::other("injected background failure").into())
                })?;
                std::future::pending::<Result<(), ConnectorError>>().await
            }
        })
        .unwrap()
        .deployment(deployment)
        .run()
        .unwrap_err();

    assert_eq!(executions.load(Ordering::SeqCst), 1);
    assert!(matches!(
        error,
        SdkError::Execution(AppRunError::TaskFailure { failure, .. })
            if failure.to_string().contains("fatal")
                && failure.to_string().contains("injected background failure")
    ));
}

#[test]
fn native_async_managed_crash_cancels_tasks_and_restarts_fresh_session() {
    use riesel::{AsyncTasks, ConnectorError};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    let marker = std::env::temp_dir().join(format!(
        "riesel-async-recovery-marker-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(marker.clone());

    let program = std::path::PathBuf::from(env!("CARGO_BIN_EXE_async_counter_crash_fixture"));
    let endpoint = ServiceEndpoint::Managed {
        instance: Some("async_shared".into()),
        program,
        args: vec![marker.to_string_lossy().into_owned()],
        startup_timeout_ms: 10_000,
        shutdown_timeout_ms: 2_000,
    };
    let mut deployment = DeploymentConfig::new(BTreeMap::from([
        ("counter".into(), endpoint.clone()),
        ("observer".into(), endpoint),
    ]));
    deployment.recovery = riesel_app::RecoveryPolicy::RestartApplication {
        max_restarts: 1,
        initial_backoff_ms: 25,
        max_backoff_ms: 100,
    };

    let attempts = Arc::new(AtomicUsize::new(0));
    let previous = Arc::new(Mutex::new(None::<AsyncCounterHandle>));
    let cancelled_task_dropped = Arc::new(AtomicBool::new(false));
    let for_attempts = Arc::clone(&attempts);
    let for_previous = Arc::clone(&previous);
    let for_dropped = Arc::clone(&cancelled_task_dropped);

    Application::new("NativeAsyncRecovery")
        .async_handler_with_aliases(
            move |counter: AsyncCapability<AsyncCounterContract>,
                  observer: AsyncCapability<AsyncCounterObserverContract>,
                  tasks: AsyncTasks| {
                let attempt = for_attempts.fetch_add(1, Ordering::SeqCst);
                let previous = Arc::clone(&for_previous);
                let dropped = Arc::clone(&for_dropped);
                async move {
                    if attempt == 0 {
                        assert_eq!(counter.value().await?, 0);
                        assert_eq!(observer.observe().await?, 0);
                        *previous.lock().unwrap() = Some((*counter).clone());
                        tasks.spawn("in-flight", async move {
                            let _guard = DropProbe(dropped);
                            std::future::pending::<()>().await;
                            Ok(())
                        })?;
                        tokio::task::yield_now().await;
                        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
                        return Err(std::io::Error::other(
                            "the native async child did not cancel its session",
                        )
                        .into());
                    }
                    assert_eq!(attempt, 1, "whole-session recovery must be bounded");
                    assert!(dropped.load(Ordering::Acquire));
                    let stale = previous.lock().unwrap().take().unwrap();
                    assert!(matches!(
                        stale.value().await,
                        Err(CallError::Rpc(RpcError::SessionExpired))
                    ));
                    assert_eq!(counter.value().await?, 0);
                    assert_eq!(counter.add(9).await?, 9);
                    assert_eq!(observer.observe().await?, 9);

                    // A new session owns a fresh task scope with the same
                    // task name allowed to be used after recovery.
                    let (sent, received) = tokio::sync::oneshot::channel();
                    tasks.spawn("in-flight", async move {
                        let _ = sent.send(());
                        Ok(())
                    })?;
                    received.await.unwrap();
                    Ok::<(), ConnectorError>(())
                }
            },
            ["counter", "observer", "tasks"],
        )
        .unwrap()
        .component::<AsyncMemoryCounter>()
        .unwrap()
        .deployment(deployment)
        .run()
        .unwrap();

    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert!(marker.exists(), "the child must actually have crashed once");
    assert!(cancelled_task_dropped.load(Ordering::Acquire));
}

fn exercise_blocking_boundary(deployment: Option<DeploymentConfig>) {
    use riesel::{AsyncTasks, ConnectorError};

    let prepared = Application::new("BlockingTaskBoundary")
        .async_handler_with_aliases(
            |counter: AsyncCapability<AsyncCounterContract>, tasks: AsyncTasks| async move {
                let (sender, receiver) = tokio::sync::oneshot::channel::<u64>();
                tasks.spawn_blocking("blocking-computation", move |cancel| {
                    if cancel.is_cancelled() {
                        return Err(std::io::Error::other("cancelled before execution").into());
                    }
                    let _ = sender.send(6);
                    Ok(())
                })?;
                // This await continues on the application's Tokio executor
                // while the blocking worker runs in its bounded thread pool.
                let increment = receiver.await?;
                assert_eq!(counter.add(increment).await?, 6);
                Ok::<(), ConnectorError>(())
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
}

#[test]
fn blocking_work_can_feed_linked_native_async_component() {
    exercise_blocking_boundary(None);
}

#[test]
fn blocking_work_can_feed_managed_native_async_component() {
    let deployment = DeploymentConfig::new(BTreeMap::from([(
        "counter".into(),
        ServiceEndpoint::Managed {
            instance: None,
            program: std::path::PathBuf::from(env!("CARGO_BIN_EXE_async_counter_service")),
            args: Vec::new(),
            startup_timeout_ms: 10_000,
            shutdown_timeout_ms: 2_000,
        },
    )]));
    exercise_blocking_boundary(Some(deployment));
}
