use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage::{RecordStore, StoreError};
use riesel_storage_sqlite::{SnapshotBudget, SqliteMaintenance, SqliteStore};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, time::Duration};

fn models() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([(
        "task".into(),
        ModelDefinition {
            fields: BTreeMap::from([("title".into(), TypeRef::String)]),
        },
    )])
}

fn budget() -> SnapshotBudget {
    SnapshotBudget {
        max_records: 8,
        max_json_bytes: 65_536,
        max_snapshot_bytes: 1_048_576,
    max_elapsed: Duration::from_secs(30),
    }
}

fn fixture(path: &Path) -> uuid::Uuid {
    let store = SqliteStore::open(path, models()).unwrap();
    let one = store.create("task", json!({"title":"keep"})).unwrap();
    store
        .update("task", one.id, one.revision, json!({"title":"revised"}))
        .unwrap();
    store.create("task", json!({"title":"second"})).unwrap();
    one.id
}

type StoredRow = (String, Vec<u8>, Vec<u8>, Vec<u8>);

fn rows(path: &Path) -> Vec<StoredRow> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut stmt = conn
        .prepare("SELECT model,id,revision,data FROM records ORDER BY model,id")
        .unwrap();
    stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

#[test]
fn verifies_independent_snapshot_and_reopens_historical_records() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let id = fixture(&db);
    let before = rows(&db);

    let maintenance = SqliteMaintenance::open_existing(&db, models(), budget()).unwrap();
    let backup = dir.path().join("backup.db");
    let receipt = maintenance.backup_to(&backup, budget()).unwrap();
    assert_eq!(receipt.path, backup);
    assert_eq!(receipt.records, 2);
    assert!(receipt.json_bytes > 0);
    assert!(receipt.snapshot_bytes > 0);
    assert_eq!(rows(&backup), before);
    assert_eq!(rows(&db), before);
    drop(maintenance);

    let restored = SqliteStore::open(&backup, models()).unwrap();
    assert_eq!(restored.get("task", id).unwrap().unwrap().revision, 2);
}

#[test]
fn refuses_backup_while_provider_is_active_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let store = SqliteStore::open(&db, models()).unwrap();
    let first = store.create("task", json!({"title":"active"})).unwrap();
    let backup = dir.path().join("forbidden.db");
    assert!(matches!(
        SqliteMaintenance::open_existing(&db, models(), budget()),
        Err(StoreError::Failed(_))
    ));
    assert!(!backup.exists());
    assert_eq!(store.get("task", first.id).unwrap().unwrap().revision, 1);
}

#[test]
fn occupied_destination_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let before = rows(&db);
    let destination = dir.path().join("already-exists.db");
    std::fs::write(&destination, b"do not replace").unwrap();

    let maintenance = SqliteMaintenance::open_existing(&db, models(), budget()).unwrap();
    assert!(matches!(
        maintenance.backup_to(&destination, budget()),
        Err(StoreError::Failed(_))
    ));
    assert_eq!(std::fs::read(destination).unwrap(), b"do not replace");
    assert_eq!(rows(&db), before);
}

#[test]
fn resource_ceiling_rejects_without_publishing_incomplete_backup() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let maintenance = SqliteMaintenance::open_existing(&db, models(), budget()).unwrap();
    let output = dir.path().join("over-budget.db");

    let bounded = SnapshotBudget {
        max_records: 8,
        max_json_bytes: 65_536,
        max_snapshot_bytes: 1,
    max_elapsed: Duration::from_secs(30),
    };
    assert!(matches!(
        maintenance.backup_to(&output, bounded),
        Err(StoreError::Failed(_))
    ));
    assert!(!output.exists());
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("riesel-pending")
    }));
}

#[test]
fn incompatible_models_and_corrupt_records_fail_without_a_backup() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let id = fixture(&db);
    let incompatible = BTreeMap::from([(
        "task".into(),
        ModelDefinition {
            fields: BTreeMap::from([("other".into(), TypeRef::String)]),
        },
    )]);
    assert!(matches!(
        SqliteMaintenance::open_existing(&db, incompatible, budget()),
        Err(StoreError::Failed(_))
    ));

    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE records SET data=?1 WHERE model='task' AND id=?2",
        rusqlite::params![b"corrupt JSON", id.as_bytes()],
    )
    .unwrap();
    drop(conn);
    assert!(matches!(
        SqliteMaintenance::open_existing(&db, models(), budget()),
        Err(StoreError::Failed(_))
    ));
    assert!(!dir.path().join("unverified.db").exists());
}

#[test]
fn oversized_raw_sqlite_blob_is_rejected_before_snapshot_publication() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let id = fixture(&db);
    let conn = Connection::open(&db).unwrap();
    let raw = serde_json::to_vec(&json!({"title":"x".repeat(70_000)})).unwrap();
    conn.execute(
        "UPDATE records SET data=?1 WHERE model='task' AND id=?2",
        rusqlite::params![raw, id.as_bytes()],
    )
    .unwrap();
    drop(conn);

    let destination = dir.path().join("oversized.db");
    let large_budget = SnapshotBudget {
        max_records: 8,
        max_json_bytes: 200_000,
        max_snapshot_bytes: 1_048_576,
    max_elapsed: Duration::from_secs(30),
    };
    assert!(matches!(
        SqliteMaintenance::open_existing(&db, models(), large_budget),
        Err(StoreError::Failed(_))
    ));
    assert!(!destination.exists());
}

#[test]
fn snapshot_captures_committed_wal_rows_while_an_older_reader_is_pinned() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let store = SqliteStore::open(&db, models()).unwrap();
    store.create("task", json!({"title":"earlier"})).unwrap();

    // A separate reader retains an older SQLite view across a committed write.
    let pinned = Connection::open(&db).unwrap();
    pinned.execute_batch("BEGIN").unwrap();
    let old_count: i64 = pinned
        .query_row("SELECT count(*) FROM records", [], |row| row.get(0))
        .unwrap();
    assert_eq!(old_count, 1);
    store.create("task", json!({"title":"newer"})).unwrap();
    let expected = rows(&db);
    assert_eq!(expected.len(), 2);
    drop(store);

    let maintenance = SqliteMaintenance::open_existing(&db, models(), budget()).unwrap();
    let output = dir.path().join("wal-consistent.db");
    let verified = maintenance.backup_to(&output, budget()).unwrap();
    assert_eq!(verified.records, 2);
    assert_eq!(rows(&output), expected);
    assert_eq!(rows(&db), expected);
    pinned.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn missing_or_overbudget_source_does_not_create_a_backup_database() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.db");
    assert!(matches!(
        SqliteMaintenance::open_existing(&missing, models(), budget()),
        Err(StoreError::Failed(_))
    ));
    assert!(
        !missing.exists(),
        "maintenance may not initialize missing data"
    );

    let db = dir.path().join("source.db");
    fixture(&db);
    let before = rows(&db);
    let too_small = SnapshotBudget {
        max_records: 1,
        max_json_bytes: 65_536,
        max_snapshot_bytes: 1_048_576,
    max_elapsed: Duration::from_secs(30),
    };
    assert!(matches!(
        SqliteMaintenance::open_existing(&db, models(), too_small),
        Err(StoreError::Failed(_))
    ));
    assert_eq!(rows(&db), before);
    assert!(!dir.path().join("not-created.db").exists());
}

#[test]
fn impossible_capacity_reserve_refuses_backup_before_creating_a_pending_file() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let before = rows(&db);
    let maintenance = SqliteMaintenance::open_existing(&db, models(), budget()).unwrap();
    let backup = dir.path().join("insufficient-capacity.db");
    let impossible = SnapshotBudget {
        max_snapshot_bytes: u64::MAX,
        ..budget()
    };
    assert!(matches!(
        maintenance.backup_to(&backup, impossible),
        Err(StoreError::Failed(_))
    ));
    assert!(!backup.exists());
    assert_eq!(rows(&db), before);
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("riesel-pending")
    }));
}

#[cfg(unix)]
#[test]
fn readonly_destination_directory_fails_before_creating_a_partial_backup() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let before = rows(&db);
    let readonly = dir.path().join("readonly");
    std::fs::create_dir(&readonly).unwrap();
    std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o500)).unwrap();

    let maintenance = SqliteMaintenance::open_existing(&db, models(), budget()).unwrap();
    let backup = readonly.join("not-created.db");
    let result = maintenance.backup_to(&backup, budget());

    std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(result, Err(StoreError::Failed(_))));
    assert!(!backup.exists());
    assert_eq!(rows(&db), before);
    assert!(std::fs::read_dir(readonly).unwrap().next().is_none());
}

#[test]
fn expired_backup_deadline_rejects_publication_and_preserves_source() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let before = rows(&db);
    let maintenance = SqliteMaintenance::open_existing(&db, models(), budget()).unwrap();
    let destination = dir.path().join("expired.db");
    let expired = SnapshotBudget {
        max_elapsed: Duration::from_nanos(1),
        ..budget()
    };
    assert!(maintenance.backup_to(&destination, expired).is_err());
    assert!(!destination.exists());
    assert_eq!(rows(&db), before);
}

#[test]
fn zero_elapsed_budget_rejects_maintenance_open() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let invalid = SnapshotBudget {
        max_elapsed: Duration::ZERO,
        ..budget()
    };
    assert!(SqliteMaintenance::open_existing(&db, models(), invalid).is_err());
    drop(SqliteStore::open(&db, models()).unwrap());
}
