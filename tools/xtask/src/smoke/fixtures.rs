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
