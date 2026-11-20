//! Explicit operator CLI uses existing typed Rust plans without a new schema.
use riesel_schema::{ModelDefinition, RieselModel, TypeRef};
use riesel_storage::RecordStore;
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
    for confirmation in ["", "MIGRATE different-database\n"] {
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

    let consent = format!("MIGRATE {}\n", db.canonicalize().unwrap().display());
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
    drop(SqliteStore::open(&db, models::<New>()).unwrap());

    let confirmation = format!(
        "RESTORE {} DISCARD POST-SNAPSHOT WRITES\n",
        db.canonicalize().unwrap().display()
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
            let writer = rusqlite::Connection::open(&self.database)
                .map_err(std::io::Error::other)?;
            writer
                .execute(
                    "UPDATE records SET data=?1 WHERE model='task'",
                    rusqlite::params![
                        serde_json::to_vec(&json!({"title":"modified"})).unwrap()
                    ],
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
    let consent = format!("MIGRATE {}\n", db.canonicalize().unwrap().display());
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
    assert!(archive.exists(), "a verified post-drift preimage is retained");
    let old = SqliteStore::open(&db, models::<Old>()).unwrap();
    let source = old.get("task", id).unwrap().unwrap();
    assert_eq!(source.revision, 1);
    assert_eq!(source.data["title"], "modified");
    drop(old);
    assert!(SqliteStore::open(&db, models::<New>()).is_err());
    let saved = SqliteStore::open(&archive, models::<Old>()).unwrap();
    assert_eq!(saved.get("task", id).unwrap().unwrap().data, source.data);
}
