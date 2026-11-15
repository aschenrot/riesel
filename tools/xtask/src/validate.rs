use crate::command::{TaskResult, cargo};

pub enum FormatMode {
    Fix,
    Check,
}

pub fn run(mode: FormatMode) -> TaskResult {
    println!("\n==> Formatting");

    match mode {
        FormatMode::Fix => cargo(&["fmt", "--all"])?,
        FormatMode::Check => cargo(&["fmt", "--all", "--", "--check"])?,
    }

    let checks: &[(&str, &[&str])] = &[
        ("Check", &["check", "--workspace", "--all-targets"]),
        (
            "Clippy",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        ("Tests", &["test", "--workspace"]),
        ("Documentation", &["doc", "--workspace", "--no-deps"]),
    ];

    for &(name, args) in checks {
        println!("\n==> {name}");
        cargo(args)?;
    }

    println!("\nAll validation checks passed.");
    Ok(())
}
