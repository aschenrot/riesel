use crate::command::{TaskResult, cargo, git};

pub enum FormatMode {
    Fix,
    Check,
}

pub fn run(mode: FormatMode) -> TaskResult {
    let check_lockfile = matches!(mode, FormatMode::Check);
    println!("\n==> Formatting");

    match mode {
        FormatMode::Fix => cargo(&["fmt", "--all"])?,
        FormatMode::Check => cargo(&["fmt", "--all", "--", "--check"])?,
    }

    let checks: &[(&str, &[&str])] = &[
        (
            "Check",
            &["check", "--locked", "--workspace", "--all-targets"],
        ),
        (
            "Clippy",
            &[
                "clippy",
                "--locked",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("Tests", &["test", "--locked", "--workspace"]),
        (
            "Minimal SDK without development providers",
            &["test", "--locked", "-p", "riesel", "--no-default-features"],
        ),
        (
            "Documentation",
            &["doc", "--locked", "--workspace", "--no-deps"],
        ),
    ];

    for &(name, args) in checks {
        println!("\n==> {name}");
        cargo(args)?;
    }

    if check_lockfile {
        println!("\n==> Checking committed dependency lockfile");
        git(&["diff", "--exit-code", "--", "Cargo.lock"])?;
    }

    println!("\nAll validation checks passed.");
    Ok(())
}
