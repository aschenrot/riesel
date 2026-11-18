use crate::command::{TaskResult, cargo, workspace_root};
use riesel_manager::{ProcessKey, ProcessRegistry, ProcessStatus};
use std::env;
use std::fs;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

/// Ensure the temporary deployment configuration is deleted on all exits.
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
        .map(|path| if path.is_absolute() { path } else { root.join(path) })
        .unwrap_or_else(|| root.join("target"));

    target
        .join("debug")
        .join(format!("{name}{}", env::consts::EXE_SUFFIX))
}

pub fn run() -> TaskResult {
    println!("\n==> Building Storage and Todo binaries");
    cargo(&["build", "-p", "riesel_storage_component", "--bin", "storage_service"])?;
    cargo(&["build", "-p", "todo", "--bin", "todo_client"])?;

    // Reserve a free port, then hand it to the service process.
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("cannot reserve loopback port: {e}"))?;
    let address: SocketAddr = listener.local_addr().map_err(|e| e.to_string())?;
    drop(listener);

    let path = env::temp_dir().join(format!(
        "riesel-smoke-{}-{}.json",
        std::process::id(),
        address.port()
    ));
    let deployment = format!(
        r#"{{"services":{{"storage":{{"transport":"tcp","address":"{address}"}}}}}}"#
    );
    fs::write(&path, deployment).map_err(|e| format!("cannot write deployment file: {e}"))?;
    let temporary = TemporaryConfig(path);

    println!("\n==> Starting managed Storage at {address}");
    let mut processes = ProcessRegistry::new();
    let key = ProcessKey::new(1, Some("storage".into()));
    processes
        .spawn(
            key.clone(),
            Command::new(binary("storage_service"))
                .arg(address.to_string())
                .current_dir(workspace_root()),
        )
        .map_err(|e| format!("cannot start Storage: {e}"))?;

    // A bounded readiness check ensures CI does not depend on arbitrary sleeps.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match processes.status(&key).map_err(|e| e.to_string())? {
            ProcessStatus::Running => {}
            ProcessStatus::Exited(status) => {
                return Err(format!("Storage exited before becoming ready: {status}"));
            }
        }
        if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            return Err(format!("Storage did not listen on {address} within 10 seconds"));
        }
        thread::sleep(Duration::from_millis(50));
    }

    println!("\n==> Running Todo against configured remote Storage");
    let status = Command::new(binary("todo_client"))
        .arg(&temporary.0)
        .current_dir(workspace_root())
        .status()
        .map_err(|e| format!("cannot start Todo: {e}"))?;
    if !status.success() {
        return Err(format!("Todo smoke test failed: {status}"));
    }

    println!("\n==> Stopping managed Storage");
    processes.stop_all().map_err(|e| e.to_string())?;
    println!("\nRemote Todo smoke test passed.");
    Ok(())
}
