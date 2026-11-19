use std::{
    path::Path,
    process::{Command, Output},
};
use uuid::Uuid;

fn execute(db: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_todo_cli"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .unwrap()
}

fn success(db: &Path, args: &[&str]) -> String {
    let output = execute(db, args);
    assert!(
        output.status.success(),
        "todo_cli {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn record(line: &str) -> (Uuid, u64, &str, &str) {
    let mut fields = line.trim_end().splitn(4, '\t');
    let id = Uuid::parse_str(fields.next().unwrap()).unwrap();
    let revision = fields.next().unwrap().parse().unwrap();
    let state = fields.next().unwrap();
    let title = fields.next().unwrap();
    (id, revision, state, title)
}

#[test]
fn persistent_commands_share_domain_state_across_independent_processes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("todo.db");

    let added = success(&db, &["add", "Build", "Riesel"]);
    let (id, rev, state, title) = record(&added);
    assert_eq!(rev, 1);
    assert_eq!(state, "open");
    assert_eq!(title, "\"Build Riesel\"");
    let id_text = id.to_string();
    assert!(db.exists());

    assert_eq!(record(&success(&db, &["show", &id_text])).1, 1);
    assert_eq!(record(&success(&db, &["list"])).0, id);

    let edited = success(&db, &["edit", &id_text, "1", "Ship", "Riesel"]);
    assert_eq!(record(&edited).1, 2);
    assert_eq!(record(&edited).3, "\"Ship Riesel\"");

    let stale = execute(&db, &["complete", &id_text, "1"]);
    assert_eq!(stale.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&stale.stderr).contains("modified"));
    assert_eq!(record(&success(&db, &["show", &id_text])).1, 2);

    let completed = success(&db, &["complete", &id_text, "2"]);
    assert_eq!(record(&completed).1, 3);
    assert_eq!(record(&completed).2, "done");

    let reopened = success(&db, &["reopen", &id_text, "3"]);
    assert_eq!(record(&reopened).1, 4);
    assert_eq!(record(&reopened).2, "open");

    let removed = success(&db, &["delete", &id_text, "4"]);
    assert_eq!(removed, format!("deleted\t{id}\n"));
    assert_eq!(success(&db, &["list"]), "");
    let missing = execute(&db, &["show", &id_text]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&missing.stderr).contains("task not found"));
}

#[test]
fn rejects_invalid_arguments_without_creating_storage() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("not-created.db");
    let missing_id = Uuid::nil().to_string();
    for args in [
        vec!["add"],
        vec!["add", "  "],
        vec!["show", "not-a-uuid"],
        vec!["edit", "not-a-uuid", "1", "title"],
        vec!["complete", &missing_id, "not-a-revision"],
        vec!["list", "extra"],
        vec!["unexpected"],
    ] {
        let output = execute(&db, &args);
        assert_eq!(output.status.code(), Some(2), "args: {args:?}");
    }
    assert!(!db.exists(), "invalid commands must not create the DB");

    let no_path = Command::new(env!("CARGO_BIN_EXE_todo_cli"))
        .arg("list")
        .output()
        .unwrap();
    assert_eq!(no_path.status.code(), Some(2));
    let help = Command::new(env!("CARGO_BIN_EXE_todo_cli"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
}

#[test]
fn missing_parent_directory_fails_without_ephemeral_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("missing").join("todo.db");
    let output = execute(&db, &["list"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!db.exists());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("storage") || error.contains("SQLite"),
        "{error}"
    );
}
