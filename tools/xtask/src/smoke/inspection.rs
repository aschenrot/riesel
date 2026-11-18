use super::*;

pub(super) fn verify_deployment_preflight() -> TaskResult {
    // A nonexistent service executable proves that the CLI does not attempt
    // to start the service while inspecting a valid configuration.
    let missing = workspace_root().join("riesel-preflight-must-not-execute");
    if missing.exists() {
        return Err("preflight test executable unexpectedly exists".into());
    }
    let path = env::temp_dir().join(format!("riesel-plan-smoke-{}.json", std::process::id()));
    let config = serde_json::json!({
        "services": {
            "storage": {
                "transport": "managed",
                "program": missing
            }
        }
    });
    fs::write(
        &path,
        serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("cannot write preflight configuration: {error}"))?;
    let temporary = TemporaryConfig(path);
    let inspected = Command::new(binary("todo_client"))
        .arg("--inspect-deployment")
        .arg(&temporary.0)
        .current_dir(workspace_root())
        .output()
        .map_err(|error| format!("cannot run Todo deployment inspection: {error}"))?;
    if !inspected.status.success() {
        return Err(format!(
            "deployment inspection failed: {}",
            String::from_utf8_lossy(&inspected.stderr)
        ));
    }
    let plan: serde_json::Value = serde_json::from_slice(&inspected.stdout)
        .map_err(|error| format!("invalid generated deployment plan: {error}"))?;
    if plan["services"][0]["requirement"] != "storage"
        || plan["services"][0]["target"]["transport"] != "managed"
        || plan["services"][0]["target"]["program"] != config["services"]["storage"]["program"]
    {
        return Err(format!(
            "deployment plan did not preserve selected target: {plan}"
        ));
    }
    Ok(())
}
