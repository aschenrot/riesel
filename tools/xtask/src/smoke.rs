use crate::command::{TaskResult, cargo, workspace_root};
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use riesel::schema::CapabilityContract;
use riesel::{
    App, CapabilityConnector, ConnectedProvider, ConnectorError, ConnectorRegistry,
    DeploymentConfig, RpcConnector, RuntimeError, ServiceEndpoint,
};
use riesel_storage::RecordStoreContract;

mod authoring;
mod counter;
mod fixtures;
mod inspection;
mod storage;

use authoring::verify_documented_entrypoints;
use counter::{
    verify_counter_provider, verify_managed_session_recovery, verify_shared_managed_component,
};
use fixtures::{TemporaryConfig, binary, output_with_timeout, wait_for};
use inspection::verify_deployment_preflight;
use storage::{verify_graceful_storage_exit, verify_multi_service_rollback};

pub fn run() -> TaskResult {
    println!("\n==> Building Storage, Counter and application binaries");
    cargo(&[
        "build",
        "--locked",
        "-p",
        "riesel_storage_component",
        "--bin",
        "storage_service",
    ])?;
    cargo(&["build", "--locked", "-p", "todo", "--bins"])?;
    cargo(&["build", "--locked", "-p", "riesel_hello_example", "--bins"])?;
    cargo(&[
        "build",
        "--locked",
        "-p",
        "riesel_counter_component",
        "--bin",
        "counter_service",
    ])?;
    cargo(&[
        "build",
        "--locked",
        "-p",
        "riesel_counter_example",
        "--bins",
    ])?;

    println!("\n==> Verifying documented Hello and Todo authoring entrypoints");
    verify_documented_entrypoints()?;
    println!("\nHello and Todo authoring examples passed.");

    // No pre-bind/drop reservation race and no fixed port in deployment.
    let path = env::temp_dir().join(format!("riesel-managed-smoke-{}.json", std::process::id()));
    let deployment = serde_json::json!({
        "services": {
            "storage": {
                "transport": "managed",
                "program": binary("storage_service"),
                "startup_timeout_ms": 10_000
            }
        }
    });
    fs::write(
        &path,
        serde_json::to_vec_pretty(&deployment).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("cannot write managed deployment: {error}"))?;
    let temporary = TemporaryConfig(path);

    println!("\n==> Running two concurrent Todo instances with OS-assigned Storage ports");
    let start_client = || {
        Command::new(binary("todo"))
            .arg("--deployment")
            .arg(&temporary.0)
            .current_dir(workspace_root())
            .spawn()
    };

    let mut first = start_client().map_err(|error| error.to_string())?;
    let mut second = match start_client() {
        Ok(child) => child,
        Err(error) => {
            let _ = first.kill();
            let _ = first.wait();
            return Err(format!("cannot launch second managed Todo: {error}"));
        }
    };
    let first_result = wait_for(&mut first, Duration::from_secs(25));
    let second_result = wait_for(&mut second, Duration::from_secs(25));

    if !first_result?.success() || !second_result?.success() {
        return Err("concurrent OS-assigned Storage smoke test failed".into());
    }
    println!("\nConcurrent managed Storage smoke test passed.");

    println!("\n==> Inspecting a deployment without running its configured service");
    verify_deployment_preflight()?;
    println!("\nSide-effect-free deployment inspection passed.");

    println!("\n==> Verifying multi-service startup rollback");
    verify_multi_service_rollback()?;
    println!("\nMulti-service rollback smoke test passed.");

    println!("\n==> Verifying cooperative Storage shutdown");
    verify_graceful_storage_exit()?;
    println!("\nCooperative Storage shutdown smoke test passed.");

    println!("\n==> Verifying independent Counter capability locally and as managed RPC");
    verify_counter_provider()?;
    println!("\nCounter local and managed capability integration passed.");

    println!("\n==> Verifying two capabilities sharing one managed component instance");
    verify_shared_managed_component()?;
    println!("\nShared managed component state passed.");

    println!("\n==> Verifying application recovery from a crashed managed Counter");
    verify_managed_session_recovery()?;
    println!("\nManaged Counter recovery smoke test passed.");
    Ok(())
}
