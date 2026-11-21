use std::{env, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Intentionally executes before Riesel processes any application CLI flag.
    // This simulates ordinary user-authored setup, not provider startup.
    if let Some(path) = env::var_os("RIESEL_PROBE_SETUP_MARKER") {
        fs::write(PathBuf::from(path), b"setup executed")?;
    }

    riesel::Application::new("IndependentOperationsProbe").run_from_env(|_| {
        // If the definition is only inspected, this user handler must not run.
        if let Some(path) = env::var_os("RIESEL_PROBE_HANDLER_MARKER") {
            fs::write(PathBuf::from(path), b"handler executed")?;
        }
        Ok(())
    })?;
    Ok(())
}
