use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage::{RecordStore, StoreError};
use riesel_storage_sqlite::{SnapshotBudget, SqliteMaintenance, SqliteStore};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

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

fn rows(path: &Path) -> Vec<(String, Vec<u8>, Vec<u8>, Vec<u8>)> {
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
    };
    assert!(matches!(
        maintenance.backup_to(&output, bounded),
        Err(StoreError::Failed(_))
    ));
    assert!(!output.exists());
    assert!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .all(|entry| !entry.unwrap().file_name().to_string_lossy().contains("riesel-pending"))
    );
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
