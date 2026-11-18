use std::{
    env,
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

pub type TaskResult = Result<(), String>;

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("xtask must be inside tools/xtask")
        .to_path_buf()
}

fn execute(program: impl AsRef<OsStr>, args: &[&str]) -> Result<ExitStatus, String> {
    Command::new(program)
        .args(args)
        .current_dir(workspace_root())
        .status()
        .map_err(|error| error.to_string())
}

fn checked(program: impl AsRef<OsStr>, args: &[&str]) -> TaskResult {
    let status = execute(program, args)?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("Command failed: {status}"))
    }
}

pub fn cargo(args: &[&str]) -> TaskResult {
    let executable = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));

    checked(executable, args)
}

pub fn git(args: &[&str]) -> TaskResult {
    checked("git", args)
}

pub fn has_staged_changes() -> Result<bool, String> {
    let status = execute("git", &["diff", "--cached", "--quiet"])?;

    match status.code() {
        Some(0) => Ok(false),
        Some(1) => Ok(true),
        _ => Err(format!("Could not inspect staged changes: {status}")),
    }
}
