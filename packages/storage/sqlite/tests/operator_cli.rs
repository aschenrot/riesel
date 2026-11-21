//! Explicit operator CLI uses existing typed Rust plans without a new schema.
use riesel_schema::{ModelDefinition, RieselModel, TypeRef};
use riesel_storage::{RecordStore, StoreError};
use riesel_storage_sqlite::{
    EvolutionLimits, ModelEvolution, OperatorOutcome, SnapshotBudget, SqliteStore,
    run_operator_maintenance_cli,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, ffi::OsString, io::Cursor, path::Path, time::Duration};

#[derive(Serialize, Deserialize)]
struct Old {
    title: String,
}
impl RieselModel for Old {
    const MODEL_NAME: &'static str = "task";
    fn model_definition() -> ModelDefinition {
        ModelDefinition {
            fields: BTreeMap::from([("title".into(), TypeRef::String)]),
        }
    }
}
#[derive(Serialize, Deserialize)]
struct New {
    title: String,
    priority: i64,
}
impl RieselModel for New {
    const MODEL_NAME: &'static str = "task";
    fn model_definition() -> ModelDefinition {
        ModelDefinition {
            fields: BTreeMap::from([
                ("title".into(), TypeRef::String),
                ("priority".into(), TypeRef::I64),
            ]),
        }
    }
}
fn models<T: RieselModel>() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([("task".into(), T::model_definition())])
}
fn plan() -> ModelEvolution {
    ModelEvolution::new(models::<Old>(), models::<New>())
        .unwrap()
        .transform::<Old, New, _>(|old| {
            Ok(New {
                title: old.title,
                priority: 0,
            })
        })
        .unwrap()
}
fn limits() -> EvolutionLimits {
    EvolutionLimits {
        snapshot: SnapshotBudget {
            max_records: 12,
            max_json_bytes: 100_000,
            max_snapshot_bytes: 1_048_576,
            max_sidecar_bytes: 1_048_576,
            max_elapsed: Duration::from_secs(30),
        },
        max_elapsed: Duration::from_secs(30),
    }
}
fn fixture(path: &Path) -> uuid::Uuid {
    let store = SqliteStore::open(path, models::<Old>()).unwrap();
    let id = store
        .create("task", json!({"title":"original"}))
        .unwrap()
        .id;
    drop(store);
    id
}
fn command(name: &str, paths: &[&Path]) -> Vec<OsString> {
    std::iter::once(OsString::from(name))
        .chain(paths.iter().map(|p| p.as_os_str().to_owned()))
        .collect()
}

#[test]
fn preview_checks_capacities_and_model_shapes_without_backup_or_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("active.db");
    let archive = dir.path().join("no-preview-side-effect.db");
    let id = fixture(&db);
    let mut output = Vec::new();
    let result = run_operator_maintenance_cli(
        &command("preview", &[&db, &archive]),
        &plan(),
        limits(),
        &mut Cursor::new(Vec::<u8>::new()),
        &mut output,
    )
    .unwrap();
    let OperatorOutcome::Preview(details) = result else {
        panic!("preview must not execute a migration");
    };
    assert_eq!(details.affected_records, 1);
    let text = String::from_utf8(output).unwrap();
    assert!(text.contains("Projected JSON bytes"));
    assert!(text.contains("Observed SQLite WAL/SHM bytes"));
    assert!(text.contains("Available on database volume"));
    assert!(text.contains("Model task:"));
    assert!(text.contains("Operator must stop older"));
    assert!(!archive.exists());
    let unchanged = SqliteStore::open(&db, models::<Old>()).unwrap();
    assert_eq!(unchanged.get("task", id).unwrap().unwrap().revision, 1);
}

#[test]
fn missing_or_wrong_operator_consent_rejects_without_backup() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("active.db");
    let archive = dir.path().join("never-published.db");
    let id = fixture(&db);
    let wrong_backup = format!(
        "MIGRATE {} BACKUP {}\n",
        db.canonicalize().unwrap().display(),
        dir.path().join("wrong-archive.db").display()
    );
    for confirmation in [
        String::new(),
        "MIGRATE different-database\n".to_owned(),
        wrong_backup,
    ] {
        let mut output = Vec::new();
        let result = run_operator_maintenance_cli(
            &command("execute", &[&db, &archive]),
            &plan(),
            limits(),
            &mut Cursor::new(confirmation.as_bytes()),
            &mut output,
        );
        assert!(result.is_err());
        assert!(!archive.exists());
    }
    let old = SqliteStore::open(&db, models::<Old>()).unwrap();
    assert_eq!(old.get("task", id).unwrap().unwrap().revision, 1);
    drop(old);
    assert!(SqliteStore::open(&db, models::<New>()).is_err());
}

#[test]
fn consented_evolution_and_separate_restore_keep_both_recovery_images() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("active.db");
    let archive = dir.path().join("source-backup.db");
    let retained = dir.path().join("pre-restore.db");
    let id = fixture(&db);

    let consent = format!(
        "MIGRATE {} BACKUP {}\n",
        db.canonicalize().unwrap().display(),
        archive.display()
    );
    let mut output = Vec::new();
    let outcome = run_operator_maintenance_cli(
        &command("execute", &[&db, &archive]),
        &plan(),
        limits(),
        &mut Cursor::new(consent.as_bytes()),
        &mut output,
    )
    .unwrap();
    let OperatorOutcome::Executed(receipt) = outcome else {
        panic!("expected an explicit migration receipt");
    };
    assert_eq!(receipt.retained_backup.path, archive);
    assert!(SqliteStore::open(&db, models::<Old>()).is_err());
    let new = SqliteStore::open(&db, models::<New>()).unwrap();
    assert_eq!(new.get("task", id).unwrap().unwrap().revision, 2);
    drop(new);

    // Restore also requires its OWN explicit approval. A failed approval
    // never overwrites or retains anything.
    let mut rejected = Vec::new();
    assert!(
        run_operator_maintenance_cli(
            &command("restore", &[&db, &archive, &retained]),
            &plan(),
            limits(),
            &mut Cursor::new(Vec::<u8>::new()),
            &mut rejected,
        )
        .is_err()
    );
    assert!(!retained.exists());
    let wrong_retention = format!(
        "RESTORE {} FROM {} RETAIN {} DISCARD POST-SNAPSHOT WRITES\n",
        db.canonicalize().unwrap().display(),
        archive.display(),
        dir.path().join("wrong-retention.db").display()
    );
    assert!(
        run_operator_maintenance_cli(
            &command("restore", &[&db, &archive, &retained]),
            &plan(),
            limits(),
            &mut Cursor::new(wrong_retention.as_bytes()),
            &mut rejected,
        )
        .is_err()
    );
    assert!(!retained.exists());
    drop(SqliteStore::open(&db, models::<New>()).unwrap());

    let confirmation = format!(
        "RESTORE {} FROM {} RETAIN {} DISCARD POST-SNAPSHOT WRITES\n",
        db.canonicalize().unwrap().display(),
        archive.display(),
        retained.display()
    );
    let mut output = Vec::new();
    let result = run_operator_maintenance_cli(
        &command("restore", &[&db, &archive, &retained]),
        &plan(),
        limits(),
        &mut Cursor::new(confirmation.as_bytes()),
        &mut output,
    )
    .unwrap();
    let OperatorOutcome::Restored(restored) = result else {
        panic!("expected a separate restore receipt");
    };
    assert_eq!(restored.restored_records, 1);
    assert_eq!(restored.retained_pre_restore.path, retained);
    let old = SqliteStore::open(&db, models::<Old>()).unwrap();
    assert_eq!(
        old.get("task", id).unwrap().unwrap().data["title"],
        "original"
    );
    drop(old);
    assert!(SqliteStore::open(&db, models::<New>()).is_err());
    let newer = SqliteStore::open(&retained, models::<New>()).unwrap();
    assert_eq!(newer.get("task", id).unwrap().unwrap().revision, 2);
}

#[test]
fn unsupported_args_owner_conflict_and_impossible_reserve_fail_closed() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("active.db");
    let archive = dir.path().join("never-published.db");
    fixture(&db);
    let mut output = Vec::new();
    assert!(
        run_operator_maintenance_cli(
            &command("run", &[&db, &archive]),
            &plan(),
            limits(),
            &mut Cursor::new(Vec::<u8>::new()),
            &mut output,
        )
        .is_err()
    );

    let live = SqliteStore::open(&db, models::<Old>()).unwrap();
    assert!(
        run_operator_maintenance_cli(
            &command("preview", &[&db, &archive]),
            &plan(),
            limits(),
            &mut Cursor::new(Vec::<u8>::new()),
            &mut output,
        )
        .is_err()
    );
    drop(live);

    let impossible = EvolutionLimits {
        snapshot: SnapshotBudget {
            max_snapshot_bytes: u64::MAX,
            ..limits().snapshot
        },
        ..limits()
    };
    assert!(
        run_operator_maintenance_cli(
            &command("preview", &[&db, &archive]),
            &plan(),
            impossible,
            &mut Cursor::new(Vec::<u8>::new()),
            &mut output,
        )
        .is_err()
    );
    assert!(!archive.exists());
    drop(SqliteStore::open(&db, models::<Old>()).unwrap());
}

struct EditSourceWhenConsentIsRead {
    database: std::path::PathBuf,
    input: Cursor<Vec<u8>>,
    edited: bool,
}

impl std::io::Read for EditSourceWhenConsentIsRead {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.input, out)
    }
}

impl std::io::BufRead for EditSourceWhenConsentIsRead {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if !self.edited {
            let writer =
                rusqlite::Connection::open(&self.database).map_err(std::io::Error::other)?;
            writer
                .execute(
                    "UPDATE records SET data=?1 WHERE model='task'",
                    rusqlite::params![serde_json::to_vec(&json!({"title":"modified"})).unwrap()],
                )
                .map_err(std::io::Error::other)?;
            self.edited = true;
        }
        std::io::BufRead::fill_buf(&mut self.input)
    }

    fn consume(&mut self, amount: usize) {
        std::io::BufRead::consume(&mut self.input, amount);
    }
}

#[test]
fn correct_operator_consent_does_not_override_direct_source_drift_during_prompt() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("active.db");
    let archive = dir.path().join("newly-verified-source.db");
    let id = fixture(&db);
    let consent = format!(
        "MIGRATE {} BACKUP {}\n",
        db.canonicalize().unwrap().display(),
        archive.display()
    );
    let mut input = EditSourceWhenConsentIsRead {
        database: db.clone(),
        input: Cursor::new(consent.into_bytes()),
        edited: false,
    };
    let mut output = Vec::new();
    let result = run_operator_maintenance_cli(
        &command("execute", &[&db, &archive]),
        &plan(),
        limits(),
        &mut input,
        &mut output,
    );
    let error = result.unwrap_err();
    assert!(input.edited);
    assert!(format!("{error}").contains("source drift"), "{error}");
    assert!(
        archive.exists(),
        "a verified post-drift preimage is retained"
    );
    let old = SqliteStore::open(&db, models::<Old>()).unwrap();
    let source = old.get("task", id).unwrap().unwrap();
    assert_eq!(source.revision, 1);
    assert_eq!(source.data["title"], "modified");
    drop(old);
    assert!(SqliteStore::open(&db, models::<New>()).is_err());
    let saved = SqliteStore::open(&archive, models::<Old>()).unwrap();
    assert_eq!(saved.get("task", id).unwrap().unwrap().data, source.data);
}

#[test]
fn operator_confirmation_is_bounded_and_requires_a_complete_line() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let archive = dir.path().join("no-archive.db");
    let id = fixture(&db);
    let exact = format!(
        "MIGRATE {} BACKUP {}",
        db.canonicalize().unwrap().display(),
        archive.display()
    );
    for answer in [
        format!("{exact}{}", " ".repeat(4_096)).into_bytes(),
        exact.into_bytes(),
        vec![0xff, b'\n'],
    ] {
        let mut output = Vec::new();
        let result = run_operator_maintenance_cli(
            &command("execute", &[&db, &archive]),
            &plan(),
            limits(),
            &mut Cursor::new(answer),
            &mut output,
        );
        assert!(result.is_err(), "malformed confirmation was accepted");
        assert!(!archive.exists());
        let old = SqliteStore::open(&db, models::<Old>()).unwrap();
        assert_eq!(old.get("task", id).unwrap().unwrap().revision, 1);
    }
    assert!(SqliteStore::open(&db, models::<New>()).is_err());
}

#[test]
fn occupied_backup_destination_and_corrupt_source_refuse_before_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let archive = dir.path().join("preexisting.db");
    let id = fixture(&db);
    std::fs::write(&archive, b"must not overwrite").unwrap();
    let original_archive = std::fs::read(&archive).unwrap();
    for operation in ["preview", "execute"] {
        let mut output = Vec::new();
        let result = run_operator_maintenance_cli(
            &command(operation, &[&db, &archive]),
            &plan(),
            limits(),
            &mut Cursor::new(Vec::<u8>::new()),
            &mut output,
        );
        assert!(result.is_err(), "occupied archive unexpectedly accepted");
        assert_eq!(std::fs::read(&archive).unwrap(), original_archive);
    }
    let old = SqliteStore::open(&db, models::<Old>()).unwrap();
    assert_eq!(old.get("task", id).unwrap().unwrap().revision, 1);
    drop(old);

    let corrupt = dir.path().join("corrupt.db");
    fixture(&corrupt);
    let direct = rusqlite::Connection::open(&corrupt).unwrap();
    direct
        .execute(
            "UPDATE records SET data=?1 WHERE model='task'",
            rusqlite::params![b"not JSON"],
        )
        .unwrap();
    drop(direct);
    let unpublished = dir.path().join("unpublished.db");
    for operation in ["preview", "execute"] {
        let mut output = Vec::new();
        assert!(
            run_operator_maintenance_cli(
                &command(operation, &[&corrupt, &unpublished]),
                &plan(),
                limits(),
                &mut Cursor::new(Vec::<u8>::new()),
                &mut output,
            )
            .is_err()
        );
        assert!(!unpublished.exists());
    }
    let direct = rusqlite::Connection::open(&corrupt).unwrap();
    let raw: Vec<u8> = direct
        .query_row("SELECT data FROM records WHERE model='task'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(raw, b"not JSON");
}

#[test]
fn failure_during_confirmed_execution_preserves_old_schema_and_recovery_image() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let archive = dir.path().join("retained-old.db");
    let id = fixture(&db);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let failing = ModelEvolution::new(models::<Old>(), models::<New>())
        .unwrap()
        .transform::<Old, New, _>(move |old| {
            if observed.fetch_add(1, Ordering::SeqCst) > 0 {
                return Err(StoreError::Failed(
                    "injected confirmed-execution transform failure".into(),
                ));
            }
            Ok(New {
                title: old.title,
                priority: 0,
            })
        })
        .unwrap();
    let confirmation = format!(
        "MIGRATE {} BACKUP {}\n",
        db.canonicalize().unwrap().display(),
        archive.display()
    );
    let mut output = Vec::new();
    let err = run_operator_maintenance_cli(
        &command("execute", &[&db, &archive]),
        &failing,
        limits(),
        &mut Cursor::new(confirmation.as_bytes()),
        &mut output,
    )
    .unwrap_err();
    assert!(format!("{err}").contains("injected confirmed-execution"));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(archive.exists());
    let old = SqliteStore::open(&db, models::<Old>()).unwrap();
    assert_eq!(old.get("task", id).unwrap().unwrap().revision, 1);
    drop(old);
    assert!(SqliteStore::open(&db, models::<New>()).is_err());
    let snapshot = SqliteStore::open(&archive, models::<Old>()).unwrap();
    assert_eq!(snapshot.get("task", id).unwrap().unwrap().revision, 1);
}

#[test]
fn operator_restore_rejects_corrupt_backup_and_occupied_target_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let archive = dir.path().join("old.db");
    let retention = dir.path().join("not-created.db");
    let id = fixture(&db);
    let confirmation = format!(
        "MIGRATE {} BACKUP {}\n",
        db.canonicalize().unwrap().display(),
        archive.display()
    );
    run_operator_maintenance_cli(
        &command("execute", &[&db, &archive]),
        &plan(),
        limits(),
        &mut Cursor::new(confirmation.as_bytes()),
        &mut Vec::new(),
    )
    .unwrap();

    let live = SqliteStore::open(&db, models::<New>()).unwrap();
    let mut output = Vec::new();
    assert!(
        run_operator_maintenance_cli(
            &command("restore", &[&db, &archive, &retention]),
            &plan(),
            limits(),
            &mut Cursor::new(Vec::<u8>::new()),
            &mut output,
        )
        .is_err()
    );
    assert!(!retention.exists());
    assert_eq!(live.get("task", id).unwrap().unwrap().revision, 2);
    drop(live);

    let direct = rusqlite::Connection::open(&archive).unwrap();
    direct
        .execute(
            "UPDATE records SET data=?1 WHERE model='task'",
            rusqlite::params![b"not JSON"],
        )
        .unwrap();
    drop(direct);
    let mut output = Vec::new();
    let err = run_operator_maintenance_cli(
        &command("restore", &[&db, &archive, &retention]),
        &plan(),
        limits(),
        &mut Cursor::new(Vec::<u8>::new()),
        &mut output,
    )
    .unwrap_err();
    assert!(format!("{err}").contains("SQLite"), "{err}");
    assert!(!retention.exists());
    let evolved = SqliteStore::open(&db, models::<New>()).unwrap();
    assert_eq!(evolved.get("task", id).unwrap().unwrap().revision, 2);
    drop(evolved);
    assert!(SqliteStore::open(&db, models::<Old>()).is_err());
}

#[test]
fn projected_json_overflow_is_refused_before_operator_approval() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let archive = dir.path().join("not-created.db");
    let id = fixture(&db);
    let direct = rusqlite::Connection::open(&db).unwrap();
    let old_json_bytes: i64 = direct
        .query_row(
            "SELECT length(data) FROM records WHERE model='task'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    drop(direct);
    let strict = EvolutionLimits {
        snapshot: SnapshotBudget {
            max_json_bytes: u64::try_from(old_json_bytes).unwrap(),
            ..limits().snapshot
        },
        ..limits()
    };
    let mut output = Vec::new();
    let result = run_operator_maintenance_cli(
        &command("execute", &[&db, &archive]),
        &plan(),
        strict,
        &mut Cursor::new(Vec::<u8>::new()),
        &mut output,
    );
    let error = result.unwrap_err();
    assert!(format!("{error}").contains("projected JSON"), "{error}");
    assert!(!archive.exists());
    let old = SqliteStore::open(&db, models::<Old>()).unwrap();
    assert_eq!(old.get("task", id).unwrap().unwrap().revision, 1);
}

#[derive(Default)]
struct FailOnCommitReceipt {
    output: Vec<u8>,
    failed: bool,
}

impl std::io::Write for FailOnCommitReceipt {
    fn write(&mut self, text: &[u8]) -> std::io::Result<usize> {
        if text
            .windows(b"Migration committed".len())
            .any(|s| s == b"Migration committed")
        {
            self.failed = true;
            return Err(std::io::Error::other(
                "injected output failure after commit",
            ));
        }
        self.output.extend_from_slice(text);
        Ok(text.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn output_failure_after_commit_is_not_misreported_as_a_safe_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let archive = dir.path().join("retained-old.db");
    let id = fixture(&db);
    let confirmation = format!(
        "MIGRATE {} BACKUP {}\n",
        db.canonicalize().unwrap().display(),
        archive.display()
    );
    let mut output = FailOnCommitReceipt::default();
    let err = run_operator_maintenance_cli(
        &command("execute", &[&db, &archive]),
        &plan(),
        limits(),
        &mut Cursor::new(confirmation.as_bytes()),
        &mut output,
    )
    .unwrap_err();
    assert!(
        output.failed,
        "simulated post-commit failure was not triggered"
    );
    assert!(
        format!("{err}").contains("injected output failure after commit"),
        "{err}"
    );
    // The return value is Err, but the transaction already committed.
    // An operator must inspect the new schema rather than retry or restore.
    let current = SqliteStore::open(&db, models::<New>()).unwrap();
    assert_eq!(current.get("task", id).unwrap().unwrap().revision, 2);
    drop(current);
    assert!(SqliteStore::open(&db, models::<Old>()).is_err());
    let old = SqliteStore::open(&archive, models::<Old>()).unwrap();
    assert_eq!(old.get("task", id).unwrap().unwrap().revision, 1);
}

struct ActionDuringConsent {
    answer: Cursor<Vec<u8>>,
    action: Option<Box<dyn FnMut() -> std::io::Result<()>>>,
}

impl std::io::Read for ActionDuringConsent {
    fn read(&mut self, destination: &mut [u8]) -> std::io::Result<usize> {
        std::io::Read::read(&mut self.answer, destination)
    }
}

impl std::io::BufRead for ActionDuringConsent {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        if let Some(mut action) = self.action.take() {
            action()?;
        }
        std::io::BufRead::fill_buf(&mut self.answer)
    }

    fn consume(&mut self, count: usize) {
        std::io::BufRead::consume(&mut self.answer, count);
    }
}

#[test]
fn direct_source_schema_drift_during_approval_refuses_before_backup_or_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let archive = dir.path().join("not-published.db");
    fixture(&db);
    let before: Vec<u8> = rusqlite::Connection::open(&db)
        .unwrap()
        .query_row("SELECT data FROM records WHERE model='task'", [], |row| {
            row.get(0)
        })
        .unwrap();
    let confirmation = format!(
        "MIGRATE {} BACKUP {}\n",
        db.canonicalize().unwrap().display(),
        archive.display()
    );
    let path = db.clone();
    let current_model_bytes = serde_json::to_vec(&New::model_definition()).unwrap();
    let mut input = ActionDuringConsent {
        answer: Cursor::new(confirmation.into_bytes()),
        action: Some(Box::new(move || {
            let conn = rusqlite::Connection::open(&path).map_err(std::io::Error::other)?;
            conn.execute(
                "UPDATE models SET definition=?1 WHERE name='task'",
                rusqlite::params![current_model_bytes],
            )
            .map_err(std::io::Error::other)?;
            Ok(())
        })),
    };
    let error = run_operator_maintenance_cli(
        &command("execute", &[&db, &archive]),
        &plan(),
        limits(),
        &mut input,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(
        format!("{error}").contains("registration") || format!("{error}").contains("model"),
        "{error}"
    );
    assert!(
        !archive.exists(),
        "schema drift must reject before publication"
    );
    let connection = rusqlite::Connection::open(&db).unwrap();
    let after: Vec<u8> = connection
        .query_row("SELECT data FROM records WHERE model='task'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(after, before);
    assert!(SqliteStore::open(&db, models::<Old>()).is_err());
}

#[test]
fn backup_destination_occupied_during_confirmation_remains_intact() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let archive = dir.path().join("created-during-confirmation.db");
    let id = fixture(&db);
    let confirmation = format!(
        "MIGRATE {} BACKUP {}\n",
        db.canonicalize().unwrap().display(),
        archive.display()
    );
    let backup_path = archive.clone();
    let mut input = ActionDuringConsent {
        answer: Cursor::new(confirmation.into_bytes()),
        action: Some(Box::new(move || {
            std::fs::write(&backup_path, b"existing backup must not be overwritten")
        })),
    };
    let error = run_operator_maintenance_cli(
        &command("execute", &[&db, &archive]),
        &plan(),
        limits(),
        &mut input,
        &mut Vec::new(),
    )
    .unwrap_err();
    assert!(format!("{error}").contains("already exists"), "{error}");
    assert_eq!(
        std::fs::read(&archive).unwrap(),
        b"existing backup must not be overwritten"
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|item| {
        !item
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".riesel-pending-")
    }));
    let old = SqliteStore::open(&db, models::<Old>()).unwrap();
    assert_eq!(old.get("task", id).unwrap().unwrap().revision, 1);
    drop(old);
    assert!(SqliteStore::open(&db, models::<New>()).is_err());
}
