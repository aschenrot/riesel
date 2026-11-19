use super::*;

/// Exercise the documented minimal entrypoints, rather than relying solely
/// on their successful compilation in the workspace test matrix.
pub(super) fn verify_documented_entrypoints() -> TaskResult {
    for (name, expected) in [
        ("riesel_hello_example", "Hello from Riesel"),
        ("schema_hello", "Application: Riesel"),
        ("todo_macro", "Todo + Riesel Rust SDK: passed"),
    ] {
        let output = output_with_timeout(
            Command::new(binary(name))
                .current_dir(workspace_root())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|error| format!("cannot launch documented example {name}: {error}"))?,
            Duration::from_secs(15),
        )?;
        if !output.status.success() || !String::from_utf8_lossy(&output.stdout).contains(expected) {
            return Err(format!(
                "documented example {name} failed: status {}, stdout {}, stderr {}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    let definition = output_with_timeout(
        Command::new(binary("todo_macro"))
            .arg("--print-definition")
            .current_dir(workspace_root())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("cannot inspect documented Todo macro: {error}"))?,
        Duration::from_secs(15),
    )?;
    if !definition.status.success() {
        return Err(format!(
            "Todo macro definition inspection failed: {}",
            String::from_utf8_lossy(&definition.stderr)
        ));
    }
    let schema: serde_json::Value = serde_json::from_slice(&definition.stdout)
        .map_err(|error| format!("Todo macro definition is invalid JSON: {error}"))?;
    if schema["app"]["name"] != "TodoMacro"
        || schema["requires"]
            .as_array()
            .is_none_or(|requires| requires.len() != 1)
    {
        return Err(format!(
            "Todo macro definition disagrees with authoring contract: {schema}"
        ));
    }
    Ok(())
}
