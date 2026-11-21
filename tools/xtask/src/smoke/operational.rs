use super::*;
use std::path::Path;

struct ProbeDirectory(PathBuf);

impl Drop for ProbeDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_fixture(
    binary: &Path,
    working_dir: &Path,
    setup: &Path,
    handler: &Path,
    args: &[&str],
) -> Result<std::process::Output, String> {
    let child = Command::new(binary)
        .args(args)
        .current_dir(working_dir)
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

/// Research #75: an external executable's inspection command executes its
/// application setup code even when SDK preflight does not start providers.
pub(super) fn verify_independent_inspection() -> TaskResult {
    let root = workspace_root();
    let fixture = root.join("research/operational-inspection");
    let probe = binary("riesel-operational-inspection-probe");
    let target_dir = probe
        .parent()
        .and_then(|path| path.parent())
        .ok_or_else(|| "cannot determine Cargo research target directory".to_owned())?;
    let scratch = env::temp_dir().join(format!(
        "riesel-operational-inspection-{}",
        std::process::id()
    ));
    fs::create_dir(&scratch)
        .map_err(|error| format!("cannot create external Cargo workspace: {error}"))?;
    let _cleanup = ProbeDirectory(scratch.clone());

    // Cargo configuration searches ancestor directories. Merely adding an
    // inner [workspace] does NOT prove isolation from the repository aliases.
    // Materialize this fixture outside the repository's directory tree.
    fs::create_dir(scratch.join("src"))
        .map_err(|error| format!("cannot create independent app source: {error}"))?;
    fs::copy(fixture.join("src/main.rs"), scratch.join("src/main.rs"))
        .map_err(|error| format!("cannot copy independent app source: {error}"))?;
    let sdk = root
        .join("crates/riesel")
        .canonicalize()
        .map_err(|error| format!("cannot locate Riesel SDK: {error}"))?;
    let sdk = sdk
        .to_str()
        .ok_or_else(|| "research fixture requires a UTF-8 SDK checkout path".to_owned())?;
    let dependency = serde_json::to_string(sdk).map_err(|error| error.to_string())?;
    let template = fs::read_to_string(fixture.join("Cargo.toml"))
        .map_err(|error| format!("cannot read independent Cargo manifest: {error}"))?;
    const PLACEHOLDER: &str = "\"../../crates/riesel\"";
    if !template.contains(PLACEHOLDER) {
        return Err("independent app manifest is missing its SDK path".into());
    }
    let manifest = scratch.join("Cargo.toml");
    fs::write(&manifest, template.replace(PLACEHOLDER, &dependency))
        .map_err(|error| format!("cannot write independent Cargo manifest: {error}"))?;

    // Offline dependencies have been fetched by the preceding workspace
    // validation. The independent app does not inherit the repository's
    // Cargo aliases, workspace membership or lockfile.
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(&cargo)
        .arg("build")
        .arg("--offline")
        .arg("--manifest-path")
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", target_dir)
        .current_dir(&scratch)
        .status()
        .map_err(|error| format!("cannot build independent app: {error}"))?;
    if !status.success() {
        return Err(format!(
            "independent Cargo workspace build failed: {status}"
        ));
    }

    let setup = scratch.join("app-setup.marker");
    let handler = scratch.join("app-handler.marker");
    let definition = run_fixture(
        &probe,
        &scratch,
        &setup,
        &handler,
        &["--print-definition"],
    )?;
    check_exit(&definition, "external executable definition inspection")?;
    let schema: serde_json::Value = serde_json::from_slice(&definition.stdout)
        .map_err(|error| format!("invalid independent app definition: {error}"))?;
    if schema["app"]["name"] != "IndependentOperationsProbe" {
        return Err(format!("unexpected inspected app schema: {schema}"));
    }
    if !setup.exists() || handler.exists() {
        return Err("executable inspection must execute app setup but not the handler".to_owned());
    }

    // Reading saved metadata is passive, not proof of its provenance.
    let cached = scratch.join("cached-definition.json");
    fs::write(&cached, &definition.stdout)
        .map_err(|error| format!("cannot cache inspected definition: {error}"))?;
    fs::remove_file(&setup).map_err(|error| error.to_string())?;
    let bytes = fs::read(&cached).map_err(|error| error.to_string())?;
    if bytes != definition.stdout || setup.exists() || handler.exists() {
        return Err("passive cached-definition read unexpectedly ran app code".into());
    }

    let help = run_fixture(&probe, &scratch, &setup, &handler, &["--help"])?;
    check_exit(&help, "independent app help")?;
    if !setup.exists()
        || handler.exists()
        || !String::from_utf8_lossy(&help.stdout).contains("Usage:")
    {
        return Err("app help did not preserve setup/handler distinction".into());
    }

    fs::remove_file(&setup).map_err(|error| error.to_string())?;
    let bad_args = run_fixture(
        &probe,
        &scratch,
        &setup,
        &handler,
        &["--unsupported-infrastructure-flag"],
    )?;
    if bad_args.status.success() || !setup.exists() || handler.exists() {
        return Err("invalid app flags did not preserve setup/handler distinction".into());
    }

    fs::remove_file(&setup).map_err(|error| error.to_string())?;
    let normal = run_fixture(&probe, &scratch, &setup, &handler, &[])?;
    check_exit(&normal, "normal independent app run")?;
    if !setup.exists() || !handler.exists() {
        return Err("normal independent app did not execute setup and handler".into());
    }
    Ok(())
}
