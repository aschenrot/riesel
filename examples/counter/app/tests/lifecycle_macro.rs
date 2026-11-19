use std::process::Command;

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_counter_macro_lifecycle"))
        .args(args)
        .output()
        .expect("lifecycle macro binary must launch")
}

fn assert_lifecycle(output: std::process::Output) {
    assert!(
        output.status.success(),
        "lifecycle app failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let hooks: Vec<_> = text
        .lines()
        .filter(|line| line.starts_with("LIFECYCLE_"))
        .collect();
    assert_eq!(
        hooks,
        ["LIFECYCLE_START", "LIFECYCLE_ENTRY", "LIFECYCLE_STOP"]
    );
}

#[test]
fn high_level_module_uses_identical_hooks_for_local_and_managed_providers() {
    assert_lifecycle(run(&[]));

    let deployment = serde_json::json!({
        "services": {
            "counter": {
                "transport": "managed",
                "program": env!("CARGO_BIN_EXE_counter_macro_managed_service"),
                "startup_timeout_ms": 10_000,
                "shutdown_timeout_ms": 2_000
            }
        }
    });
    let path = std::env::temp_dir().join(format!(
        "riesel-lifecycle-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, deployment.to_string()).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    assert_lifecycle(run(&["--deployment", path.to_str().unwrap()]));

    let inspection = run(&["--print-definition"]);
    assert!(inspection.status.success());
    let definition: serde_json::Value = serde_json::from_slice(&inspection.stdout).unwrap();
    assert_eq!(definition["requires"].as_array().unwrap().len(), 1);
    assert_eq!(definition["requires"][0]["name"], "counter");
}
