use crate::{
    command::{TaskResult, git, has_staged_changes},
    validate,
};

pub fn run(message: &str) -> TaskResult {
    if message.trim().is_empty() {
        return Err("Commit message must not be empty".into());
    }

    println!("\n==> Validating");
    validate::run(validate::FormatMode::Fix)?;

    println!("\n==> Staging");
    git(&["add", "."])?;

    println!("\n==> Checking staged changes");
    git(&["diff", "--cached", "--check"])?;

    if !has_staged_changes()? {
        return Err("Nothing to commit".into());
    }

    git(&["diff", "--cached", "--stat"])?;

    println!("\n==> Committing");
    git(&["commit", "-m", message])?;

    println!("\n==> Pushing");
    git(&["push"])?;

    println!("\nShip completed successfully.");
    Ok(())
}
