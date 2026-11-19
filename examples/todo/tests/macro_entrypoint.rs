use std::process::Command;

#[test]
fn macro_records_app_runs_without_silent_storage_fallback() {
    let binary = env!("CARGO_BIN_EXE_todo_macro");
    let output = Command::new(binary)
        .output()
        .expect("todo macro binary should start");
    assert!(
        output.status.success(),
        "todo macro failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Todo + Riesel Rust SDK: passed"));
    let output = Command::new(binary)
        .arg("--print-definition")
        .output()
        .unwrap();
    assert!(output.status.success());
    let definition: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(definition["app"]["name"], "TodoMacro");
    assert_eq!(definition["requires"].as_array().unwrap().len(), 1);
    assert!(definition["model_bindings"]["task"].is_array());
}
