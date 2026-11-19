use riesel::{Application, ApplicationModule, AsyncCapability, RuntimeError, SdkError};
use riesel_counter::{AsyncCounterContract, AsyncCounterObserverContract, CounterError};
use riesel_counter_component::AsyncMemoryCounter;
use riesel_protocol::CallError;
use std::process::Command;

struct ProviderModule;
impl ApplicationModule for ProviderModule {
    fn register(application: Application) -> riesel::Result<Application> {
        application.component::<AsyncMemoryCounter>()
    }
}

struct DuplicateProviderModule;
impl ApplicationModule for DuplicateProviderModule {
    fn register(application: Application) -> riesel::Result<Application> {
        application.component::<AsyncMemoryCounter>()
    }
}

struct ObserverDependsOnCounter;
impl ApplicationModule for ObserverDependsOnCounter {
    fn register(application: Application) -> riesel::Result<Application> {
        application.depends_on("observer", ["counter"])
    }
}

#[test]
fn modules_use_one_dependency_graph_independent_of_registration_order() {
    let app = Application::new("Modules")
        .async_rpc::<AsyncCounterContract>("counter")
        .unwrap()
        .async_rpc::<AsyncCounterObserverContract>("observer")
        .unwrap()
        .module::<ObserverDependsOnCounter>()
        .unwrap()
        .module::<ProviderModule>()
        .unwrap();

    let ordered: Vec<_> = app
        .definition()
        .startup_order()
        .unwrap()
        .into_iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(ordered, ["counter", "observer"]);

    // The one existing resolver preflights the module-provided component.
    let prepared = app
        .async_handler_with_aliases(
            |counter: AsyncCapability<AsyncCounterContract>,
             observer: AsyncCapability<AsyncCounterObserverContract>| async move {
                assert_eq!(counter.add(4).await?, 4);
                assert_eq!(observer.observe().await?, 4);
                Ok::<(), CallError<CounterError>>(())
            },
            ["counter", "observer"],
        )
        .unwrap();
    prepared.run().unwrap();
}

#[test]
fn duplicate_module_and_duplicate_provider_fail_before_startup() {
    let app = Application::new("Duplicate")
        .module::<ProviderModule>()
        .unwrap();
    assert!(matches!(
        app.module::<ProviderModule>(),
        Err(SdkError::DuplicateModule(_))
    ));
    let app = Application::new("ConflictingProvider")
        .module::<ProviderModule>()
        .unwrap();
    assert!(matches!(
        app.module::<DuplicateProviderModule>(),
        Err(SdkError::Runtime(RuntimeError::DuplicateLocalProvider(_)))
    ));
}

#[test]
fn generated_application_can_compose_modules_without_another_runtime() {
    let bin = env!("CARGO_BIN_EXE_counter_macro_module");
    let result = Command::new(bin).output().unwrap();
    assert!(
        result.status.success(),
        "macro module failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("Module counter: 5"));
    let output = Command::new(bin)
        .arg("--print-definition")
        .output()
        .unwrap();
    assert!(output.status.success());
    let schema: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(schema["requires"].as_array().unwrap().len(), 2);
    let observer = schema["requires"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "observer")
        .unwrap();
    assert_eq!(observer["depends_on"][0], "counter");
}
