use super::*;

pub(super) fn verify_counter_provider() -> TaskResult {
    let mut local = Command::new(binary("riesel_counter_example"))
        .current_dir(workspace_root())
        .spawn()
        .map_err(|error| format!("cannot run local Counter example: {error}"))?;
    let local_status = wait_for(&mut local, Duration::from_secs(15))?;
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
    let mut managed = Command::new(binary("counter_client"))
        .arg("--deployment")
        .arg(&temporary.0)
        .current_dir(workspace_root())
        .spawn()
        .map_err(|error| format!("cannot run managed Counter client: {error}"))?;
    let managed_status = wait_for(&mut managed, Duration::from_secs(15))?;
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
    let client = output_with_timeout(
        Command::new(binary("counter_recovery_client"))
            .arg("--deployment")
            .arg(&deployment.0)
            .current_dir(workspace_root())
            .spawn()
            .map_err(|error| format!("cannot launch recovering Counter application: {error}"))?,
        Duration::from_secs(25),
    )?;

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

pub(super) fn verify_shared_managed_component() -> TaskResult {
    let path = env::temp_dir().join(format!(
        "riesel-shared-component-smoke-{}.json",
        std::process::id()
    ));
    let managed = serde_json::json!({
        "transport": "managed",
        "instance": "counter_shared",
        "program": binary("counter_service")
    });
    let config = serde_json::json!({
        "services": {
            "counter": managed,
            "observer": managed
        }
    });
    fs::write(
        &path,
        serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("cannot write shared component deployment: {error}"))?;
    let deployment = TemporaryConfig(path);
    let output = output_with_timeout(
        Command::new(binary("counter_dual_client"))
            .arg("--deployment")
            .arg(&deployment.0)
            .current_dir(workspace_root())
            .spawn()
            .map_err(|error| format!("cannot run shared component client: {error}"))?,
        Duration::from_secs(20),
    )?;
    if !output.status.success()
        || !String::from_utf8_lossy(&output.stdout).contains("RIESEL_SHARED_COMPONENT_OK")
    {
        return Err(format!(
            "managed provider was not shared: status {}, stdout {}, stderr {}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(())
}
