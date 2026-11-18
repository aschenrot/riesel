use crate::command::{TaskResult, cargo, workspace_root};
use std::env;
use std::fs;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// Clean up temporary deployment config on both success and failure.
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

    // A temporary port avoids hardcoded test resources. The runtime performs
    // its own pre-spawn bind check and validates a real service handshake.
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("cannot reserve loopback port: {e}"))?;
    let address: SocketAddr = listener.local_addr().map_err(|e| e.to_string())?;
    drop(listener);

    let path = env::temp_dir().join(format!(
        "riesel-smoke-{}-{}.json",
        std::process::id(),
        address.port()
    ));
    let deployment = serde_json::json!({
        "services": {
            "storage": {
                "transport": "managed_tcp",
                "address": address.to_string(),
                "program": binary("storage_service"),
                "args": [address.to_string()],
                "startup_timeout_ms": 10_000
            }
        }
    });
    fs::write(
        &path,
        serde_json::to_vec_pretty(&deployment).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("cannot write deployment file: {e}"))?;
    let temporary = TemporaryConfig(path);

    println!("\n==> Running Todo with runtime-managed Storage at {address}");
    let status = Command::new(binary("todo_client"))
        .arg("--deployment")
        .arg(&temporary.0)
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("cannot start Todo: {e}"))?;
    if !status.success() {
        return Err(format!("Todo smoke test failed: {status}"));
    }

    // Once Todo exits, the runtime must have stopped the child.
    if TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok() {
        return Err("Storage listener remains reachable after application exit".into());
    }
    println!("\n==> Running two Todo instances with OS-assigned service ports");
    let dynamic_path =
        env::temp_dir().join(format!("riesel-dynamic-smoke-{}.json", std::process::id()));
    let dynamic_config = serde_json::json!({
        "services": {
            "storage": {
                "transport": "managed_tcp_dynamic",
                "program": binary("storage_service"),
                "startup_timeout_ms": 10_000
            }
        }
    });
    fs::write(
        &dynamic_path,
        serde_json::to_vec_pretty(&dynamic_config).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("cannot write dynamic deployment: {error}"))?;
    let dynamic = TemporaryConfig(dynamic_path);

    let start_client = || {
        Command::new(binary("todo_client"))
            .arg("--deployment")
            .arg(&dynamic.0)
            .current_dir(workspace_root())
            .spawn()
    };

    let mut first = start_client().map_err(|error| error.to_string())?;
    let mut second = match start_client() {
        Ok(child) => child,
        Err(error) => {
            let _ = first.kill();
            let _ = first.wait();
            return Err(format!("cannot launch second dynamic client: {error}"));
        }
    };
    let first_result = first.wait().map_err(|error| error.to_string());
    let second_result = second.wait().map_err(|error| error.to_string());

    if !first_result?.success() || !second_result?.success() {
        return Err("concurrent OS-assigned Storage smoke test failed".into());
    }
    println!("\nFixed and dynamic remote Todo smoke tests passed.");
    Ok(())
}
