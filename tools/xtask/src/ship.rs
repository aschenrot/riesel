use crate::{
    command::{TaskResult, git, has_staged_changes, has_unstaged_changes, has_untracked_files},
    validate,
};

pub fn run(message: &str) -> TaskResult {
    if message.trim().is_empty() {
        return Err("Commit message must not be empty".into());
    }

    println!("\n==> Checking the exact staged source snapshot");
    if !has_staged_changes()? {
        return Err("Nothing to commit".into());
    }
    if has_unstaged_changes()? || has_untracked_files()? {
        return Err(
            "Cannot ship with unstaged or untracked changes: validation must match the exact staged source".into()
        );
    }
    git(&["diff", "--cached", "--check"])?;

    println!("\n==> Validating staged source");
    // The working tree and index now agree; Cargo validates exactly what the
    // next commit records instead of an uncommitted mixture.
    validate::run(validate::FormatMode::Check)?;

    git(&["diff", "--cached", "--stat"])?;

    println!("\n==> Committing");
    git(&["commit", "-m", message])?;

    println!("\n==> Pushing");
    git(&["push"])?;

    println!("\nShip completed successfully.");
    Ok(())
}
