#![cfg(feature = "async-tcp")]
//! Stage 5.5: real catalog-selected native async Counter providers, from
//! local state, a Riesel-owned child and an operator-owned loopback process.
use riesel::{
    AppRunError, Application, AsyncCapability, CatalogError, ProviderCatalog, RuntimeError,
    ProviderSelectionConfig, SdkError, ServiceEndpoint,
};
use riesel_app::{LocalProviderFactory, PlannedTarget};
use riesel_counter::{AsyncCounterContract, AsyncCounterObserverContract, CounterError};
use riesel_counter_component::{
    AsyncMemoryCounter, BlockingAsyncCounter, memory_async_counter_provider,
};
use riesel_protocol::CallError;
use riesel_schema::ProviderDefinition;
use std::io::{BufRead, BufReader, Write};
use std::net::SocketAddr;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

fn published() -> ProviderDefinition {
    memory_async_counter_provider()
        .unwrap()
        .definition()
        .unwrap()
}

fn managed(program: impl Into<std::path::PathBuf>) -> ServiceEndpoint {
    ServiceEndpoint::Managed {
        instance: None,
        program: program.into(),
        args: Vec::new(),
        startup_timeout_ms: 10_000,
        shutdown_timeout_ms: 2_000,
    }
}

fn run_counter_catalog(
    catalog: ProviderCatalog,
    chosen: Option<&str>,
    with_linked: bool,
) -> riesel::Result<()> {
    let app = Application::new("CounterCatalog").async_handler_with_aliases(
        |counter: AsyncCapability<AsyncCounterContract>,
         observer: AsyncCapability<AsyncCounterObserverContract>| async move {
            assert_eq!(counter.value().await?, 0);
            assert_eq!(counter.add(5).await?, 5);
            assert_eq!(observer.observe().await?, 5);
            Ok::<_, CallError<CounterError>>(())
        },
        ["counter", "observer"],
    )?;
    let app = if with_linked {
        app.component::<AsyncMemoryCounter>()?
    } else {
        app
    };
    let app = app.catalog(catalog);
    let app = match chosen {
        Some(choice) => app
            .select_provider("counter", choice)?
            .select_provider("observer", choice)?,
        None => app,
    };
    let plan = app.catalog_plan()?;
    assert_eq!(plan.services.len(), 2);
    match chosen {
        Some("managed") => {
            assert!(plan.services.iter().all(|service| {
                matches!(
                    &service.target,
                    PlannedTarget::Managed { instance: Some(instance), .. }
                        if instance == "managed"
                )
            }));
        }
        Some("external") => {
            assert!(
                plan.services
                    .iter()
                    .all(|service| matches!(service.target, PlannedTarget::Tcp { .. }))
            );
        }
        None => {
            assert!(
                plan.services
                    .iter()
                    .all(|service| matches!(service.target, PlannedTarget::Local { .. }))
            );
        }
        _ => {}
    }
    app.run()
}

#[test]
fn catalog_uses_only_explicit_linked_components_by_default() {
    // An unusable managed executable is never started because its placement
    // has not been selected. Both contracts share linked component state.
    let mut catalog = ProviderCatalog::new();
    catalog
        .register(
            "managed",
            published(),
            managed("deliberately_missing_catalog_service"),
        )
        .unwrap();
    run_counter_catalog(catalog, None, true).unwrap();
}

#[test]
fn one_named_managed_choice_reuses_one_real_child_for_two_capabilities() {
    let mut catalog = ProviderCatalog::new();
    catalog
        .register(
            "managed",
            published(),
            managed(env!("CARGO_BIN_EXE_async_counter_service")),
        )
        .unwrap();
    // The same published implementation is linked AND available as a
    // managed placement. Only the explicit choice selects remote execution.
    run_counter_catalog(catalog, Some("managed"), true).unwrap();
}

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

impl OwnedChild {
    fn start() -> (Self, SocketAddr) {
        let mut child = OwnedChild(
            Command::new(env!("CARGO_BIN_EXE_async_counter_service"))
                .env("RIESEL_BIND_ADDRESS", "127.0.0.1:0")
                .env("RIESEL_CONTROL_STDIN", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap(),
        );
        let stdout = child.0.stdout.take().unwrap();
        let (tx, rx) = mpsc::sync_channel(1);
        let reader = thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
            let _ = tx.send(result);
        });
        let line = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("external service did not announce readiness")
            .expect("could not read service readiness");
        reader.join().unwrap();
        let payload: serde_json::Value = serde_json::from_str(
            line.strip_prefix("RIESEL_READY ")
                .expect("missing readiness prefix"),
        )
        .unwrap();
        let address: SocketAddr = payload["address"].as_str().unwrap().parse().unwrap();
        (child, address)
    }

    fn stop(&mut self) {
        self.0
            .stdin
            .take()
            .unwrap()
            .write_all(b"RIESEL_SHUTDOWN\n")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(exit) = self.0.try_wait().unwrap() {
                assert!(exit.success(), "external service exited with error");
                break;
            }
            assert!(Instant::now() < deadline, "external service failed to stop");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn catalog_selects_operator_owned_loopback_service_without_stopping_it() {
    let (mut child, address) = OwnedChild::start();
    let mut catalog = ProviderCatalog::new();
    catalog
        .register("external", published(), ServiceEndpoint::Tcp { address })
        .unwrap();
    run_counter_catalog(catalog, Some("external"), false).unwrap();
    assert!(
        child.0.try_wait().unwrap().is_none(),
        "Riesel must not stop an operator-owned provider"
    );
    child.stop();
}

#[test]
fn catalog_rejects_remote_provider_metadata_substitution() {
    let (mut child, address) = OwnedChild::start();
    let mut wrong = published();
    wrong.id = "riesel.counter.unexpected".into();
    let mut catalog = ProviderCatalog::new();
    catalog
        .register("external", wrong, ServiceEndpoint::Tcp { address })
        .unwrap();
    let result = run_counter_catalog(catalog, Some("external"), false);
    assert!(matches!(
        result,
        Err(SdkError::Execution(AppRunError::Startup(
            RuntimeError::ProviderIdentityMismatch { requirement, .. }
        ))) if requirement == "counter"
    ));
    // Metadata mismatch is detected without taking ownership of a service
    // belonging to the operator. This is not peer authentication.
    assert!(child.0.try_wait().unwrap().is_none());
    child.stop();
}

#[test]
fn operator_selection_manifest_runs_the_shared_managed_provider() {
    let selections = ProviderSelectionConfig::from_json(
        r#"{"services":{"counter":"managed","observer":"managed"}}"#,
    )
    .unwrap();
    let mut catalog = ProviderCatalog::new();
    catalog
        .register(
            "managed",
            published(),
            managed(env!("CARGO_BIN_EXE_async_counter_service")),
        )
        .unwrap();
    let app = Application::new("CounterManifest")
        .async_handler_with_aliases(
            |counter: AsyncCapability<AsyncCounterContract>,
             observer: AsyncCapability<AsyncCounterObserverContract>| async move {
                assert_eq!(counter.add(8).await?, 8);
                assert_eq!(observer.observe().await?, 8);
                Ok::<_, CallError<CounterError>>(())
            },
            ["counter", "observer"],
        )
        .unwrap()
        .catalog(catalog)
        .provider_selections(selections)
        .unwrap();
    let plan = app.catalog_plan().unwrap();
    assert_eq!(plan.services.len(), 2);
    assert!(plan.services.iter().all(|service| {
        service.expected_provider.as_ref().is_some_and(|provider| {
            provider.id == "riesel.counter.async.memory"
        })
    }));
    app.run().unwrap();
}

#[test]
fn ambiguous_linked_choices_require_selection_before_any_factory_runs() {
    let prepared = Application::new("AmbiguousCatalog")
        .async_handler_with_aliases(
            |counter: AsyncCapability<AsyncCounterContract>| async move {
                assert_eq!(counter.value().await?, 0);
                Ok::<_, CallError<CounterError>>(())
            },
            ["counter"],
        )
        .unwrap()
        .component::<AsyncMemoryCounter>()
        .unwrap()
        .component::<BlockingAsyncCounter>()
        .unwrap()
        .catalog(ProviderCatalog::new());
    assert!(matches!(
        prepared.catalog_plan(),
        Err(SdkError::Catalog(CatalogError::AmbiguousLocal {
            requirement, choices
        })) if requirement == "counter" && choices.len() == 2
    ));
    let selected = prepared
        .select_provider("counter", "riesel.counter.async.memory")
        .unwrap();
    assert!(matches!(
        selected.catalog_plan().unwrap().services[0].target,
        PlannedTarget::Local { .. }
    ));
    selected.run().unwrap();
}
