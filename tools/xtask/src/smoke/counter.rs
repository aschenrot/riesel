use super::*;

pub(super) fn verify_counter_provider() -> TaskResult {
    let local_status = Command::new(binary("riesel_counter_example"))
        .current_dir(workspace_root())
        .status()
        .map_err(|error| format!("cannot run local Counter example: {error}"))?;
    if !local_status.success() {
        return Err("local Counter capability integration failed".into());
    }

    let path = env::temp_dir().join(format!("riesel-counter-smoke-{}.json", std::process::id()));
    let config = serde_json::json!({
        "services": {
            "counter": {
                "transport": "managed",
                "program": binary("counter_service")
            }
        }
    });
    fs::write(
        &path,
        serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("cannot write Counter deployment: {error}"))?;
    let temporary = TemporaryConfig(path);
    let managed_status = Command::new(binary("counter_client"))
        .arg("--deployment")
        .arg(&temporary.0)
        .current_dir(workspace_root())
        .status()
        .map_err(|error| format!("cannot run managed Counter client: {error}"))?;
    if !managed_status.success() {
        return Err("managed Counter capability integration failed".into());
    }
    Ok(())
}

pub(super) fn verify_managed_session_recovery() -> TaskResult {
    let marker = env::temp_dir().join(format!("riesel-recovery-marker-{}", std::process::id()));
    if marker.exists() {
        return Err("recovery fixture marker already exists".into());
    }
    let _marker_cleanup = TemporaryConfig(marker.clone());
    let path = env::temp_dir().join(format!("riesel-recovery-smoke-{}.json", std::process::id()));
    let config = serde_json::json!({
        "services": {
            "counter": {
                "transport": "managed",
                "program": binary("counter_crash_fixture"),
                "args": [marker],
                "startup_timeout_ms": 10_000,
                "shutdown_timeout_ms": 2_000
            }
        },
        "recovery": {
            "mode": "restart_application",
            "max_restarts": 1,
            "initial_backoff_ms": 25,
            "max_backoff_ms": 100
        }
    });
    fs::write(
        &path,
        serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("cannot write recovery deployment: {error}"))?;
    let deployment = TemporaryConfig(path);
    let client = Command::new(binary("counter_recovery_client"))
        .arg("--deployment")
        .arg(&deployment.0)
        .current_dir(workspace_root())
        .output()
        .map_err(|error| format!("cannot launch recovering Counter application: {error}"))?;

    if !client.status.success()
        || !String::from_utf8_lossy(&client.stdout).contains("RIESEL_RECOVERY_OK")
    {
        return Err(format!(
            "managed Counter session failed to recover: status {}, stdout {}, stderr {}",
            client.status,
            String::from_utf8_lossy(&client.stdout),
            String::from_utf8_lossy(&client.stderr)
        ));
    }
    if !marker.exists() {
        return Err("managed Counter crash was not injected".into());
    }
    Ok(())
}
