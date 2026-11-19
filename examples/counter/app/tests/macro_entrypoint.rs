//! The generated high-level entrypoint uses the same authoritative
//! schema and provider resolver as the explicit typed-handler builder.
use riesel::{Application, NativeHandleParam};
use riesel_counter::{AsyncCounterContract, CounterError};
use riesel_counter_component::AsyncMemoryCounter;
use riesel_protocol::CallError;
use std::process::Command;

fn output(args: &[&str]) -> std::process::Output {
    let output = Command::new(env!("CARGO_BIN_EXE_counter_macro"))
        .args(args)
        .output()
        .expect("counter macro binary did not launch");
    assert!(
        output.status.success(),
        "counter macro failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn macro_and_builder_have_equivalent_definition_and_run_linked_counter() {
    let result = output(&[]);
    assert!(String::from_utf8_lossy(&result.stdout).contains("Counter macro: 3"));

    let defined: serde_json::Value =
        serde_json::from_slice(&output(&["--print-definition"]).stdout).unwrap();
    let builder = Application::new("CounterMacro")
        .async_handler(
            |_counter: NativeHandleParam<riesel_counter::AsyncCounterHandle>| async move {
                Ok::<(), CallError<CounterError>>(())
            },
        )
        .unwrap()
        .component::<AsyncMemoryCounter>()
        .unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&builder.definition_json().unwrap()).unwrap();
    assert_eq!(defined, expected);
    assert_eq!(defined["requires"].as_array().unwrap().len(), 1);
}

#[test]
fn identical_macro_binary_works_with_managed_provider_without_code_changes() {
    let alias = AsyncCounterContract::definition()
        .unwrap()
        .capability
        .default_requirement_alias();
    let mut services = serde_json::Map::new();
    services.insert(
        alias,
        serde_json::json!({
            "transport": "managed",
            "program": env!("CARGO_BIN_EXE_counter_macro_managed_service"),
            "startup_timeout_ms": 10_000,
            "shutdown_timeout_ms": 2_000
        }),
    );
    let config = serde_json::json!({ "services": services });
    let path = std::env::temp_dir().join(format!(
        "riesel-counter-macro-{}-{}.json",
        std::process::id(),
        line!()
    ));
    std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());

    let inspected = output(&[
        "--inspect-deployment",
        path.to_str().expect("UTF-8 test path"),
    ]);
    let plan: serde_json::Value = serde_json::from_slice(&inspected.stdout).unwrap();
    assert_eq!(plan["services"].as_array().unwrap().len(), 1);
    let explicit = riesel::DeploymentConfig::from_json(&config.to_string()).unwrap();
    let builder = Application::new("CounterMacro")
        .async_handler(
            |_counter: NativeHandleParam<riesel_counter::AsyncCounterHandle>| async move {
                Ok::<(), CallError<CounterError>>(())
            },
        )
        .unwrap()
        .component::<AsyncMemoryCounter>()
        .unwrap();
    let expected_plan = serde_json::to_value(builder.deployment_plan(&explicit).unwrap()).unwrap();
    assert_eq!(plan, expected_plan);

    let managed = output(&["--deployment", path.to_str().expect("UTF-8 test path")]);
    assert!(String::from_utf8_lossy(&managed.stdout).contains("Counter macro: 3"));
}

#[test]
fn macro_supports_two_explicit_aliases_with_one_local_provider_instance() {
    let output = Command::new(env!("CARGO_BIN_EXE_counter_macro_alias"))
        .output()
        .expect("aliased counter example must launch");
    assert!(
        output.status.success(),
        "aliased macro failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Aliases share counter: 2"));

    let output = Command::new(env!("CARGO_BIN_EXE_counter_macro_alias"))
        .arg("--print-definition")
        .output()
        .unwrap();
    assert!(output.status.success());
    let schema: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut aliases: Vec<String> = schema["requires"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_owned())
        .collect();
    aliases.sort();
    assert_eq!(aliases, ["primary", "secondary"]);
}
