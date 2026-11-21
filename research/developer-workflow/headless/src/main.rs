//! Independent headless application used solely for developer-workflow research.
//! The application-defined stdin stop command is NOT a Riesel stop protocol.
use std::{env, fs, io::{self, Write}, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(path) = env::var_os("DEV68_SETUP_MARKER") {
        fs::write(PathBuf::from(path), b"setup")?;
    }

    riesel::Application::new("DeveloperWorkflowHeadlessProbe").run_from_env(|_| {
        if let Some(path) = env::var_os("DEV68_HANDLER_MARKER") {
            fs::write(PathBuf::from(path), b"handler").expect("write research marker");
        }
        println!("DEV68_HEADLESS_READY");
        io::stdout().flush().expect("flush headless readiness");

        // A representative app-specific shutdown mechanism: one line of stdin.
        // A generic launcher cannot assume other applications implement this.
        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("read stop input");
        assert_eq!(input.trim(), "stop", "unrecognized headless stop");
        Ok(())
    })?;
    Ok(())
}
