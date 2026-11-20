use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage::{RecordStore, StoreError};
use riesel_storage_sqlite::{RestoreAuthorization, SnapshotBudget, SqliteMaintenance, SqliteStore};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, time::Duration};

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
        max_sidecar_bytes: 1_048_576,
        max_elapsed: Duration::from_secs(30),
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
    drop(SqliteStore::open(&db, models_v1()).unwrap());
}

#[test]
fn native_restore_with_exhausted_sqlite_page_budget_rolls_back_completely() {
    let dir = tempfile::tempdir().unwrap();
    let archived = dir.path().join("large-archive.db");
    let source_store = SqliteStore::open(&archived, models_v1()).unwrap();
    for _ in 0..48 {
        source_store
            .create("task", json!({"title":"x".repeat(8_000)}))
            .unwrap();
    }
    drop(source_store);
    let original_archive = rows(&archived);

    let db = dir.path().join("records.db");
    let kept_id = fixture(&db);
    let before = rows(&db);
    let mut destination = Connection::open(&db).unwrap();
    let existing_pages: i64 = destination
        .pragma_query_value(None, "page_count", |row| row.get(0))
        .unwrap();
    let cap = existing_pages + 4;
    let applied: i64 = destination
        .query_row(&format!("PRAGMA max_page_count={cap}"), [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(applied, cap);
    let source = Connection::open(&archived).unwrap();
    let source_pages: i64 = source
        .pragma_query_value(None, "page_count", |row| row.get(0))
        .unwrap();
    assert!(source_pages > cap);
    drop(source);

    // SQLite's own page limit injects SQLITE_FULL during the same native
    // backup/restore API used by maintenance. This is not physical ENOSPC.
    let error = destination
        .restore(rusqlite::MAIN_DB, &archived, rusqlite::backup::NO_PROGRESS)
        .unwrap_err();
    assert!(
        matches!(
            &error,
            rusqlite::Error::SqliteFailure(details, _)
                if details.code == rusqlite::ErrorCode::DiskFull
        ),
        "{error:?}"
    );
    drop(destination);
    assert_eq!(rows(&db), before);
    assert_eq!(rows(&archived), original_archive);
    let reopened = SqliteStore::open(&db, models_v1()).unwrap();
    assert_eq!(reopened.get("task", kept_id).unwrap().unwrap().revision, 1);
}

#[test]
fn physically_invalid_archived_snapshot_fails_before_target_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("records.db");
    fixture(&db);
    let archive = dir.path().join("historical.db");
    snapshot(&db, &archive);
    let original = rows(&db);

    // A damaged archive, not a simulation of a physical disk failure.
    std::fs::write(&archive, b"not a valid SQLite database").unwrap();
    let retained = dir.path().join("retained.db");
    let maintenance = SqliteMaintenance::open_existing(&db, models_v1(), budget()).unwrap();
    assert!(matches!(
        maintenance.restore_from_backup(
            &archive,
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
fn restore_rejects_invalid_budget_before_copying_any_pages() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("records.db");
    fixture(&db);
    let archive = dir.path().join("historical.db");
    snapshot(&db, &archive);
    let before = rows(&db);
    let retained = dir.path().join("budget-rejected.db");

    let maintenance = SqliteMaintenance::open_existing(&db, models_v1(), budget()).unwrap();
    let restrictive = SnapshotBudget {
        max_records: 1,
        max_json_bytes: 65_536,
        max_snapshot_bytes: 1_048_576,
        max_sidecar_bytes: 1_048_576,
        max_elapsed: Duration::from_secs(30),
    };
    assert!(matches!(
        maintenance.restore_from_backup(
            &archive,
            models_v1(),
            &retained,
            restrictive,
            RestoreAuthorization::DiscardPostSnapshotWrites,
        ),
        Err(StoreError::Failed(_))
    ));
    assert_eq!(rows(&db), before);
    assert!(!retained.exists());
}

#[test]
fn impossible_restore_capacity_refuses_before_retention_or_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("records.db");
    fixture(&db);
    let historical = dir.path().join("historical.db");
    snapshot(&db, &historical);
    let before = rows(&db);
    let retained = dir.path().join("should-not-be-created.db");
    let maintenance = SqliteMaintenance::open_existing(&db, models_v1(), budget()).unwrap();
    let impossible = SnapshotBudget {
        max_snapshot_bytes: u64::MAX,
        ..budget()
    };
    assert!(matches!(
        maintenance.restore_from_backup(
            &historical,
            models_v1(),
            &retained,
            impossible,
            RestoreAuthorization::DiscardPostSnapshotWrites,
        ),
        Err(StoreError::Failed(_))
    ));
    assert_eq!(rows(&db), before);
    assert!(!retained.exists());
}

#[test]
fn expired_restore_deadline_preserves_original_without_retention() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("records.db");
    fixture(&db);
    let archived = dir.path().join("snapshot.db");
    snapshot(&db, &archived);
    let before = rows(&db);
    let retained = dir.path().join("retained.db");
    let maintenance = SqliteMaintenance::open_existing(&db, models_v1(), budget()).unwrap();
    let expired = SnapshotBudget {
        max_elapsed: Duration::from_nanos(1),
        ..budget()
    };
    assert!(
        maintenance
            .restore_from_backup(
                &archived,
                models_v1(),
                &retained,
                expired,
                RestoreAuthorization::DiscardPostSnapshotWrites,
            )
            .is_err()
    );
    assert!(!retained.exists());
    assert_eq!(rows(&db), before);
    drop(SqliteStore::open(&db, models_v1()).unwrap());
}
