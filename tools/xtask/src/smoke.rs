use crate::command::{TaskResult, cargo, workspace_root};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::net::{SocketAddr, TcpStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use riesel::schema::CapabilityContract;
use riesel::storage::RecordStoreContract;
use riesel::{
    App, CapabilityConnector, ConnectedProvider, ConnectorError, ConnectorRegistry,
    DeploymentConfig, RpcConnector, RuntimeError, ServiceEndpoint,
};

/// Remove generated deployment config on success and failure.
struct TemporaryConfig(PathBuf);

impl Drop for TemporaryConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn binary(name: &str) -> PathBuf {
    let root = workspace_root();
    let target = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
        .unwrap_or_else(|| root.join("target"));

    target
        .join("debug")
        .join(format!("{name}{}", env::consts::EXE_SUFFIX))
}

pub fn run() -> TaskResult {
    println!("\n==> Building Storage and Todo binaries");
    cargo(&[
        "build",
        "-p",
        "riesel_storage_component",
        "--bin",
        "storage_service",
    ])?;
    cargo(&["build", "-p", "todo", "--bin", "todo_client"])?;

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
        Command::new(binary("todo_client"))
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
    let first_result = first.wait().map_err(|error| error.to_string());
    let second_result = second.wait().map_err(|error| error.to_string());

    if !first_result?.success() || !second_result?.success() {
        return Err("concurrent OS-assigned Storage smoke test failed".into());
    }
    println!("\nConcurrent managed Storage smoke test passed.");

    println!("\n==> Verifying multi-service startup rollback");
    verify_multi_service_rollback()?;
    println!("\nMulti-service rollback smoke test passed.");
    Ok(())
}

struct ObservedConnector {
    rpc: RpcConnector<RecordStoreContract>,
    connected: Arc<Mutex<Vec<SocketAddr>>>,
}

impl CapabilityConnector for ObservedConnector {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        self.rpc.contract()
    }

    fn connect(&self, endpoint: &ServiceEndpoint) -> Result<ConnectedProvider, ConnectorError> {
        let connected = self.rpc.connect(endpoint)?;
        if let Some(address) = endpoint.address() {
            self.connected.lock().unwrap().push(address);
        }
        Ok(connected)
    }
}

/// A real first managed Storage process becomes ready; the next dependent
/// process cannot spawn. All previously started children must be reaped.
fn verify_multi_service_rollback() -> TaskResult {
    let app = App::from_schema(
        r#"{
            "version":1,
            "app":{"name":"RollbackProbe"},
            "models":{},
            "requires":[
                {"name":"second","capability":{"id":"riesel.storage.records","version":1},
                 "depends_on":["first"]},
                {"name":"first","capability":{"id":"riesel.storage.records","version":1}}
            ]
        }"#,
    ).map_err(|error| error.to_string())?;
    let missing_program = workspace_root().join("riesel-nonexistent-service-for-rollback");
    if missing_program.exists() {
        return Err("rollback test executable unexpectedly exists".into());
    }
    let deployment = serde_json::json!({
        "services": {
            "first": {"transport":"managed","program":binary("storage_service")},
            "second": {"transport":"managed","program":missing_program}
        }
    });
    let config = DeploymentConfig::from_json(&deployment.to_string())
        .map_err(|error| error.to_string())?;
    let connected = Arc::new(Mutex::new(Vec::new()));
    let mut connectors = ConnectorRegistry::new();
    connectors.register(ObservedConnector {
        rpc: RpcConnector::new(),
        connected: Arc::clone(&connected),
    }).map_err(|error| error.to_string())?;

    match app.start(&config, &connectors) {
        Err(RuntimeError::ServiceStart { requirement, .. }) if requirement == "second" => {}
        Err(error) => return Err(format!("unexpected rollback failure: {error}")),
        Ok(mut running) => {
            running.shutdown().map_err(|error| error.to_string())?;
            return Err("expected failure to spawn second managed service".into());
        }
    }

    let addresses = connected.lock().map_err(|error| error.to_string())?;
    let [address] = addresses.as_slice() else {
        return Err(format!("expected one verified live service, found {}", addresses.len()));
    };
    if TcpStream::connect_timeout(address, Duration::from_millis(250)).is_ok() {
        return Err(format!("first managed service survived rollback at {address}"));
    }
    Ok(())
}
