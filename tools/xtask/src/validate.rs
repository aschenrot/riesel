use crate::command::{TaskResult, cargo, git, workspace_root};
use std::fs;
use std::path::Path;

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

    println!("\n==> Local Markdown links");
    check_markdown_links()?;

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
            "Todo local HTTP API (opt-in)",
            &["test", "--locked", "-p", "todo", "--features", "http"],
        ),
        (
            "Graph runner with the existing async session feature",
            &[
                "test",
                "--locked",
                "-p",
                "riesel",
                "--features",
                "async-tcp",
                "--test",
                "execution_runner",
            ],
        ),
        (
            "Structured graph cleanup before provider teardown",
            &[
                "test",
                "--locked",
                "-p",
                "riesel_app",
                "--features",
                "async-tcp",
                "--lib",
                "structured_background_graph_drains_before_local_provider_teardown",
            ],
        ),
        (
            "Named background graph failure propagation",
            &[
                "test",
                "--locked",
                "-p",
                "riesel_app",
                "--features",
                "async-tcp",
                "--lib",
                "background_graph_failure_retains_named_task_error_and_report",
            ],
        ),
        (
            "Foreign graph task scope is rejected before admission",
            &[
                "test",
                "--locked",
                "-p",
                "riesel_app",
                "--features",
                "async-tcp",
                "--lib",
                "graph_from_another_app_session_cannot_enter_our_task_scope",
            ],
        ),
        (
            "Revoked session retains original graph failure attribution",
            &[
                "test",
                "--locked",
                "-p",
                "riesel_app",
                "--features",
                "async-tcp",
                "--lib",
                "revoked_session_does_not_invent_secondary_background_graph_failure",
            ],
        ),
        (
            "Real application-session graph/domain ownership regression",
            &[
                "test",
                "--locked",
                "-p",
                "riesel_app",
                "--features",
                "async-tcp",
                "--lib",
                "session_owned_graph_owns_thread_confined_domain",
            ],
        ),
        (
            "Runnable session-owned headless execution graph",
            &[
                "run",
                "--locked",
                "-p",
                "riesel",
                "--features",
                "async-tcp",
                "--example",
                "execution_graph",
            ],
        ),
        (
            "Explicit fixed-tick domain authority comparison",
            &[
                "test",
                "--locked",
                "-p",
                "riesel",
                "--features",
                "async-tcp",
                "--test",
                "execution_domains",
            ],
        ),
        (
            "Minimal SDK without standard capability packages",
            &["test", "--locked", "-p", "riesel", "--no-default-features"],
        ),
        (
            "Typed Records SDK without a development provider",
            &[
                "test",
                "--locked",
                "-p",
                "riesel",
                "--no-default-features",
                "--features",
                "records",
            ],
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

/// Check local link destinations in the existing reference documentation.
/// This validates paths, not remote URLs or in-page heading fragments.
fn check_markdown_links() -> TaskResult {
    let root = workspace_root();
    for relative in ["README.md", "docs", "examples"] {
        check_markdown_path(&root.join(relative))?;
    }
    Ok(())
}

fn check_markdown_path(path: &Path) -> TaskResult {
    if path.is_dir() {
        for entry in fs::read_dir(path).map_err(|error| error.to_string())? {
            check_markdown_path(&entry.map_err(|error| error.to_string())?.path())?;
        }
        return Ok(());
    }
    if path.extension().is_none_or(|extension| extension != "md") {
        return Ok(());
    }

    let content = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let mut fenced = false;
    for (index, line) in content.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for tail in line.split("](").skip(1) {
            let raw = tail.split(')').next().unwrap_or("");
            let destination = raw.split_whitespace().next().unwrap_or("");
            let relative = destination.split('#').next().unwrap_or("");
            if relative.is_empty()
                || relative.starts_with('/')
                || relative.starts_with("https://")
                || relative.starts_with("http://")
                || relative.starts_with("mailto:")
            {
                continue;
            }
            let resolved = path.parent().unwrap_or(Path::new(".")).join(relative);
            if !resolved.exists() {
                return Err(format!(
                    "{}:{}: missing Markdown link destination: {}",
                    path.display(),
                    index + 1,
                    relative
                ));
            }
        }
    }
    Ok(())
}
