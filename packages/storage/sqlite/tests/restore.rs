use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage::{RecordStore, StoreError};
use riesel_storage_sqlite::{RestoreAuthorization, SnapshotBudget, SqliteMaintenance, SqliteStore};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

fn models_v1() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([(
        "task".into(),
        ModelDefinition {
            fields: BTreeMap::from([("title".into(), TypeRef::String)]),
        },
    )])
}

fn models_v2() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([(
        "task".into(),
        ModelDefinition {
            fields: BTreeMap::from([
                ("title".into(), TypeRef::String),
                ("completed".into(), TypeRef::Bool),
            ]),
        },
    )])
}

fn budget() -> SnapshotBudget {
    SnapshotBudget {
        max_records: 10,
        max_json_bytes: 65_536,
        max_snapshot_bytes: 1_048_576,
    }
}

fn fixture(path: &Path) -> uuid::Uuid {
    let store = SqliteStore::open(path, models_v1()).unwrap();
    let first = store.create("task", json!({"title":"old"})).unwrap();
    store.create("task", json!({"title":"other"})).unwrap();
    first.id
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

fn snapshot(path: &Path, out: &Path) {
    let maintenance = SqliteMaintenance::open_existing(path, models_v1(), budget()).unwrap();
    maintenance.backup_to(out, budget()).unwrap();
}

#[test]
fn deliberate_restore_discards_later_writes_and_retains_current_state() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("records.db");
    let original_id = fixture(&db);
    let original = rows(&db);
    let saved = dir.path().join("historical.db");
    snapshot(&db, &saved);

    let store = SqliteStore::open(&db, models_v1()).unwrap();
    store.create("task", json!({"title":"newer"})).unwrap();
    let first = store.get("task", original_id).unwrap().unwrap();
    store
        .update(
            "task",
            original_id,
            first.revision,
            json!({"title":"changed"}),
        )
        .unwrap();
    drop(store);
    let newer = rows(&db);
    assert_ne!(original, newer);

    let maintenance = SqliteMaintenance::open_existing(&db, models_v1(), budget()).unwrap();
    let retained = dir.path().join("pre-restore.db");
    let result = maintenance
        .restore_from_backup(
            &saved,
            models_v1(),
            &retained,
            budget(),
            RestoreAuthorization::DiscardPostSnapshotWrites,
        )
        .unwrap();
    assert_eq!(result.retained_pre_restore.path, retained);
    assert_eq!(result.restored_records, 2);
    assert_eq!(rows(&db), original);
    assert_eq!(rows(&retained), newer);
    assert_eq!(rows(&saved), original);

    let reopened = SqliteStore::open(&db, models_v1()).unwrap();
    assert_eq!(
        reopened.get("task", original_id).unwrap().unwrap().revision,
        1
    );
}

#[test]
fn corrupt_or_incompatible_snapshot_does_not_modify_current_database() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("records.db");
    let id = fixture(&db);
    let historical = dir.path().join("historical.db");
    snapshot(&db, &historical);

    let conn = Connection::open(&historical).unwrap();
    conn.execute(
        "UPDATE records SET data=?1 WHERE model='task' AND id=?2",
        rusqlite::params![b"not JSON", id.as_bytes()],
    )
    .unwrap();
    drop(conn);
    let original = rows(&db);
    let retained = dir.path().join("should-not-exist.db");
    let maintenance = SqliteMaintenance::open_existing(&db, models_v1(), budget()).unwrap();
    assert!(matches!(
        maintenance.restore_from_backup(
            &historical,
            models_v1(),
            &retained,
            budget(),
            RestoreAuthorization::DiscardPostSnapshotWrites,
        ),
        Err(StoreError::Failed(_))
    ));
    assert_eq!(rows(&db), original);
    assert!(!retained.exists());
}

#[test]
fn occupied_pre_restore_destination_fails_before_discarding_writes() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("records.db");
    fixture(&db);
    let historical = dir.path().join("historical.db");
    snapshot(&db, &historical);
    let before = rows(&db);
    let retained = dir.path().join("occupied.db");
    std::fs::write(&retained, b"do not overwrite").unwrap();

    let maintenance = SqliteMaintenance::open_existing(&db, models_v1(), budget()).unwrap();
    assert!(matches!(
        maintenance.restore_from_backup(
            &historical,
            models_v1(),
            &retained,
            budget(),
            RestoreAuthorization::DiscardPostSnapshotWrites,
        ),
        Err(StoreError::Failed(_))
    ));
    assert_eq!(rows(&db), before);
    assert_eq!(std::fs::read(&retained).unwrap(), b"do not overwrite");
}

#[test]
fn restore_can_downgrade_model_metadata_only_after_explicit_authorization() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("records.db");
    fixture(&db);
    let old_rows = rows(&db);
    let archive = dir.path().join("historical.db");
    snapshot(&db, &archive);

    // Deliberately test-only simulation of a completed schema change:
    // the shipping provider must not offer implicit startup migrations.
    let mut conn = Connection::open(&db).unwrap();
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    for (model, id, revision, data) in &old_rows {
        let mut record: serde_json::Value = serde_json::from_slice(data).unwrap();
        record["completed"] = json!(false);
        let revision = u64::from_be_bytes(revision.clone().try_into().unwrap());
        tx.execute(
            "UPDATE records SET revision=?1,data=?2 WHERE model=?3 AND id=?4",
            rusqlite::params![
                &(revision + 1).to_be_bytes()[..],
                serde_json::to_vec(&record).unwrap(),
                model,
                id,
            ],
        )
        .unwrap();
    }
    tx.execute(
        "UPDATE models SET definition=?1 WHERE name='task'",
        rusqlite::params![serde_json::to_vec(&models_v2()["task"]).unwrap()],
    )
    .unwrap();
    tx.commit().unwrap();
    drop(conn);
    assert!(SqliteStore::open(&db, models_v1()).is_err());
    drop(SqliteStore::open(&db, models_v2()).unwrap());

    let retained = dir.path().join("evolved-retained.db");
    let maintenance = SqliteMaintenance::open_existing(&db, models_v2(), budget()).unwrap();
    let receipt = maintenance
        .restore_from_backup(
            &archive,
            models_v1(),
            &retained,
            budget(),
            RestoreAuthorization::DiscardPostSnapshotWrites,
        )
        .unwrap();
    assert_eq!(receipt.restored_records, 2);
    assert_eq!(rows(&db), old_rows);
    assert_ne!(rows(&retained), old_rows);
    drop(SqliteStore::open(&db, models_v1()).unwrap());
    assert!(SqliteStore::open(&db, models_v2()).is_err());
}

#[test]
fn active_provider_cannot_run_offline_restore() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("records.db");
    let store = SqliteStore::open(&db, models_v1()).unwrap();
    let first = store.create("task", json!({"title":"active"})).unwrap();
    assert!(SqliteMaintenance::open_existing(&db, models_v1(), budget()).is_err());
    assert_eq!(store.get("task", first.id).unwrap().unwrap().revision, 1);
}

#[test]
fn incomplete_native_backup_copy_does_not_publish_a_partial_restore() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("records.db");
    fixture(&db);
    let archive = dir.path().join("older.db");
    snapshot(&db, &archive);
    let store = SqliteStore::open(&db, models_v1()).unwrap();
    store.create("task", json!({"title":"newer"})).unwrap();
    drop(store);
    let before = rows(&db);

    // Test the underlying SQLite backup API contract with a single page step
    // and then drop the incomplete handle. Not a production fault injection.
    let source = Connection::open_with_flags(&archive, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut destination = Connection::open(&db).unwrap();
    let interrupted = rusqlite::backup::Backup::new(&source, &mut destination).unwrap();
    assert_eq!(
        interrupted.step(1).unwrap(),
        rusqlite::backup::StepResult::More
    );
    drop(interrupted);
    drop(destination);
    drop(source);
    assert_eq!(rows(&db), before);
}
