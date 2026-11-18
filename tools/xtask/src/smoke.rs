use crate::command::{TaskResult, cargo, workspace_root};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
    Ok(())
}
