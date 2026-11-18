use super::*;

/// Remove generated deployment config on success and failure.
pub(super) struct TemporaryConfig(pub(super) PathBuf);

impl Drop for TemporaryConfig {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn binary(name: &str) -> PathBuf {
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

/// Enforce a wall-clock limit on executable smoke fixtures. Child processes
/// are reaped on success and killed on timeout, rather than hanging CI.
pub(super) fn wait_for(child: &mut Child, timeout: Duration) -> Result<std::process::ExitStatus, String> {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait().map_err(|error| error.to_string())? {
            Some(status) => return Ok(status),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("smoke child exceeded {timeout:?} deadline"));
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    }
}

pub(super) fn output_with_timeout(
    mut child: Child,
    timeout: Duration,
) -> Result<std::process::Output, String> {
    wait_for(&mut child, timeout)?;
    child.wait_with_output().map_err(|error| error.to_string())
}
