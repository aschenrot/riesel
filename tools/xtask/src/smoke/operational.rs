use super::*;
use std::path::Path;

struct ProbeDirectory(PathBuf);

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_fixture(binary: &Path, setup: &Path, handler: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    let child = Command::new(binary)
        .args(args)
        .env("RIESEL_PROBE_SETUP_MARKER", setup)
        .env("RIESEL_PROBE_HANDLER_MARKER", handler)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot launch independent Riesel app: {error}"))?;
    output_with_timeout(child, Duration::from_secs(10))
}

fn check_exit(output: &std::process::Output, case: &str) -> TaskResult {
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "{case} failed: status {}, stdout {}, stderr {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
}

/// Research #75: executing an independent application's introspection command
/// runs its setup code, even though SDK preflight does not start providers.
pub(super) fn verify_independent_inspection() -> TaskResult {
    let root = workspace_root();
    let manifest = root.join("research/operational-inspection/Cargo.toml");
    let probe = binary("riesel-operational-inspection-probe");
    let target_dir = probe
        .parent()
        .and_then(|path| path.parent())
        .ok_or_else(|| "cannot determine Cargo research target directory".to_owned())?;

    // Build a Cargo workspace that is separate from the Riesel repository
    // workspace. Offline resolution uses the dependencies fetched by validate.
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(&cargo)
        .arg("build")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", target_dir)
        .current_dir(&root)
        .status()
        .map_err(|error| format!("cannot build independent app: {error}"))?;
    if !status.success() {
        return Err(format!("independent Cargo workspace build failed: {status}"));
    }

    let scratch = env::temp_dir().join(format!(
        "riesel-operational-inspection-{}",
        std::process::id()
    ));
    fs::create_dir(&scratch)
        .map_err(|error| format!("cannot create research marker directory: {error}"))?;
    let _cleanup = ProbeDirectory(scratch.clone());
    let setup = scratch.join("app-setup.marker");
    let handler = scratch.join("app-handler.marker");

    let definition = run_fixture(&probe, &setup, &handler, &["--print-definition"])?;
    check_exit(&definition, "external executable definition inspection")?;
    let schema: serde_json::Value = serde_json::from_slice(&definition.stdout)
        .map_err(|error| format!("invalid independent app definition: {error}"))?;
    if schema["app"]["name"] != "IndependentOperationsProbe" {
        return Err(format!("unexpected inspected app schema: {schema}"));
    }
    if !setup.exists() || handler.exists() {
        return Err(
            "executable inspection must execute app setup but not the handler".to_owned(),
        );
    }

    // Reading saved metadata has no execution side effect, but its provenance
    // is not authenticated by this fixture.
    let cached = scratch.join("cached-definition.json");
    fs::write(&cached, &definition.stdout)
        .map_err(|error| format!("cannot cache inspected definition: {error}"))?;
    fs::remove_file(&setup).map_err(|error| error.to_string())?;
    let bytes = fs::read(&cached).map_err(|error| error.to_string())?;
    if bytes != definition.stdout || setup.exists() || handler.exists() {
        return Err("passive cached-definition read unexpectedly ran app code".into());
    }

    let help = run_fixture(&probe, &setup, &handler, &["--help"])?;
    check_exit(&help, "independent app help")?;
    if !setup.exists()
        || handler.exists()
        || !String::from_utf8_lossy(&help.stdout).contains("Usage:")
    {
        return Err("app help did not preserve setup/handler distinction".into());
    }

    fs::remove_file(&setup).map_err(|error| error.to_string())?;
    let bad_args = run_fixture(&probe, &setup, &handler, &["--unsupported-infrastructure-flag"])?;
    if bad_args.status.success() || !setup.exists() || handler.exists() {
        return Err("invalid app flags did not preserve setup/handler distinction".into());
    }

    fs::remove_file(&setup).map_err(|error| error.to_string())?;
    let normal = run_fixture(&probe, &setup, &handler, &[])?;
    check_exit(&normal, "normal independent app run")?;
    if !setup.exists() || !handler.exists() {
        return Err("normal independent app did not execute setup and handler".into());
    }
    Ok(())
}
