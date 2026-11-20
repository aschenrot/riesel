//! Stage 6 migration research fixtures. NOT a production migration implementation.
//! These tests exercise the shipped format-v1 SQLite store and a hypothetical
//! explicit in-transaction transformation to inform specification Issue #41.
//! No public migration API, automatic upgrade, or rollback policy is established.
use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage::{RecordStore, StoreError};
use riesel_storage_sqlite::SqliteStore;
use rusqlite::{Connection, TransactionBehavior, params};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::Path,
    process::Command,
};
use tempfile::TempDir;
use uuid::Uuid;

fn models_v1() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([
        (
            "task".into(),
            ModelDefinition {
                fields: BTreeMap::from([
                    ("title".into(), TypeRef::String),
                    ("completed".into(), TypeRef::Bool),
                ]),
            },
        ),
        (
            "note".into(),
            ModelDefinition {
                fields: BTreeMap::from([
                    ("task_id".into(), TypeRef::Uuid),
                    ("text".into(), TypeRef::String),
                ]),
            },
        ),
    ])
}

fn models_v2() -> BTreeMap<String, ModelDefinition> {
    let mut models = models_v1();
    models.get_mut("task").unwrap().fields.insert("priority".into(), TypeRef::I64);
    models
}

fn fixture() -> (TempDir, Uuid, Uuid, Uuid) {
    let directory = tempfile::tempdir().unwrap();
    let db = directory.path().join("records.db");
    let store = SqliteStore::open(&db, models_v1()).unwrap();
    let one = store.create("task", json!({"title":"one","completed":false})).unwrap();
    let two = store.create("task", json!({"title":"two","completed":true})).unwrap();
    let note = store
        .create("note", json!({"task_id":one.id,"text":"cross-reference"}))
        .unwrap();
    let updated = store
        .update("task", one.id, one.revision, json!({"title":"revised","completed":false}))
        .unwrap();
    assert_eq!(updated.revision, 2);
    drop(store);
    (directory, one.id, two.id, note.id)
}

type Row = (String, Vec<u8>, Vec<u8>, Vec<u8>);
type Snapshot = (i64, Vec<(String, Vec<u8>)>, Vec<Row>);

fn snapshot(path: &Path) -> Snapshot {
    let conn = Connection::open(path).unwrap();
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
    let mut stmt = conn.prepare("SELECT name, definition FROM models ORDER BY name").unwrap();
    let metadata = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    drop(stmt);
    let mut stmt = conn
        .prepare("SELECT model,id,revision,data FROM records ORDER BY model,id")
        .unwrap();
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    (version, metadata, rows)
}

enum TestTransformation {
    AddPriority,
    InvalidPriority,
    StopAfterFirst,
}

/// Deliberately test-only algorithm: model metadata and JSON bytes are
/// changed within one IMMEDIATE transaction. All failures must roll back.
/// This does NOT authorize implicit startup migrations or API exposure.
fn trial_transactional_upgrade(
    path: &Path,
    transformation: TestTransformation,
) -> Result<(), &'static str> {
    let mut conn = Connection::open(path).unwrap();
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
    let version: i64 = tx.pragma_query_value(None, "user_version", |row| row.get(0)).unwrap();
    if version != 1 {
        return Err("unexpected physical format");
    }
    let stored: Vec<u8> = tx
        .query_row("SELECT definition FROM models WHERE name='task'", [], |row| row.get(0))
        .unwrap();
    if stored != serde_json::to_vec(&models_v1()["task"]).unwrap() {
        return Err("unexpected source schema");
    }
    let expected_count: i64 = tx.query_row("SELECT count(*) FROM models", [], |row| row.get(0)).unwrap();
    if expected_count != 2 {
        return Err("unexpected model set");
    }
    let records: Vec<(Vec<u8>, Vec<u8>, Vec<u8>)> = {
        let mut stmt = tx
            .prepare("SELECT id,revision,data FROM records WHERE model='task' ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    for (index, (id, revision, data)) in records.into_iter().enumerate() {
        let mut value: Value = serde_json::from_slice(&data).unwrap();
        let priority = match transformation {
            TestTransformation::InvalidPriority if index == 1 => json!("not an integer"),
            _ => json!(0_i64),
        };
        value
            .as_object_mut()
            .unwrap()
            .insert("priority".into(), priority);
        if models_v2()["task"].validate_record(&value).is_err() {
            return Err("target model validation failed");
        }
        let revision: [u8; 8] = revision.try_into().map_err(|_| "corrupt revision")?;
        let next = u64::from_be_bytes(revision)
            .checked_add(1)
            .ok_or("revision overflow")?;
        let data = serde_json::to_vec(&value).unwrap();
        tx.execute(
            "UPDATE records SET revision=?1,data=?2 WHERE model='task' AND id=?3",
            params![&next.to_be_bytes()[..], data, id],
        )
        .unwrap();
        if matches!(transformation, TestTransformation::StopAfterFirst) && index == 0 {
            return Err("injected failure after one record");
        }
    }
    tx.execute(
        "UPDATE models SET definition=?1 WHERE name='task'",
        params![serde_json::to_vec(&models_v2()["task"]).unwrap()],
    )
    .unwrap();
    tx.commit().unwrap();
    Ok(())
}

#[test]
fn format_v1_two_models_reopen_without_schema_mutation() {
    let (dir, first, _, note) = fixture();
    let path = dir.path().join("records.db");
    let original = snapshot(&path);
    assert_eq!(original.0, 1);
    assert_eq!(original.1.len(), 2);
    assert_eq!(original.2.len(), 3);
    assert_eq!(original.2[0].2.len(), 8);
    assert!(matches!(
        SqliteStore::open(&path, models_v2()),
        Err(StoreError::Failed(_))
    ));
    assert_eq!(snapshot(&path), original, "failed open must not modify stored records or model metadata");
    let store = SqliteStore::open(&path, models_v1()).unwrap();
    assert_eq!(store.get("task", first).unwrap().unwrap().revision, 2);
    assert_eq!(store.get("note", note).unwrap().unwrap().data["task_id"], first.to_string());
}

#[test]
fn even_compatible_optional_field_requires_explicit_metadata_change() {
    let (dir, _, _, _) = fixture();
    let db = dir.path().join("records.db");
    let mut optional = models_v1();
    optional.get_mut("task").unwrap().fields.insert(
        "comment".into(),
        TypeRef::Optional { item: Box::new(TypeRef::String) },
    );
    assert!(optional["task"]
        .validate_record(&json!({"title":"old record","completed":false}))
        .is_ok());
    let before = snapshot(&db);
    assert!(SqliteStore::open(&db, optional).is_err());
    assert_eq!(snapshot(&db), before);
}

#[test]
fn candidate_atomic_transform_preserves_identity_and_revisions() {
    let (dir, first, second, note) = fixture();
    let db = dir.path().join("records.db");
    trial_transactional_upgrade(&db, TestTransformation::AddPriority).unwrap();
    let (format, schemas, rows) = snapshot(&db);
    assert_eq!(format, 1, "model evolution does not require changing physical format version");
    assert_eq!(schemas.len(), 2);
    assert_eq!(rows.len(), 3);
    let store = SqliteStore::open(&db, models_v2()).unwrap();
    assert_eq!(store.get("task", first).unwrap().unwrap().revision, 3);
    assert_eq!(store.get("task", second).unwrap().unwrap().revision, 2);
    assert_eq!(store.get("note", note).unwrap().unwrap().revision, 1);
    assert_eq!(store.get("note", note).unwrap().unwrap().data["task_id"], first.to_string());
    for id in [first, second] {
        assert_eq!(store.get("task", id).unwrap().unwrap().data["priority"], 0);
    }
    assert!(matches!(
        store.update(
            "task",
            first,
            2,
            json!({"title":"stale","completed":false,"priority":0})
        ),
        Err(StoreError::RevisionConflict)
    ));
    assert!(SqliteStore::open(&db, models_v1()).is_err(), "downgrade is not silently accepted");
}

#[test]
fn bad_transformation_or_interruption_rolls_back_all_rows_and_metadata() {
    for mode in [TestTransformation::InvalidPriority, TestTransformation::StopAfterFirst] {
        let (dir, first, _, _) = fixture();
        let db = dir.path().join("records.db");
        let before = snapshot(&db);
        assert!(trial_transactional_upgrade(&db, mode).is_err());
        assert_eq!(snapshot(&db), before);
        assert!(SqliteStore::open(&db, models_v2()).is_err());
        let old = SqliteStore::open(&db, models_v1()).unwrap();
        assert_eq!(old.get("task", first).unwrap().unwrap().revision, 2);
    }
}

#[test]
fn overflowing_full_width_revision_aborts_candidate_upgrade() {
    let (dir, first, _, _) = fixture();
    let db = dir.path().join("records.db");
    let conn = Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE records SET revision=?1 WHERE model='task' AND id=?2",
        params![&u64::MAX.to_be_bytes()[..], &first.as_bytes()[..]],
    )
    .unwrap();
    drop(conn);
    let before = snapshot(&db);
    assert_eq!(trial_transactional_upgrade(&db, TestTransformation::AddPriority), Err("revision overflow"));
    assert_eq!(snapshot(&db), before);
    let old = SqliteStore::open(&db, models_v1()).unwrap();
    assert_eq!(old.get("task", first).unwrap().unwrap().revision, u64::MAX);
}

#[test]
fn uncommitted_process_exit_rolls_back_both_records_and_model_metadata() {
    if let Some(path) = std::env::var_os("RIESEL_MIGRATION_RESEARCH_CHILD_DB") {
        let marker = std::env::var_os("RIESEL_MIGRATION_RESEARCH_MARKER").unwrap();
        let mut conn = Connection::open(path).unwrap();
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate).unwrap();
        tx.execute(
            "UPDATE records SET data=?1 WHERE model='task'",
            params![serde_json::to_vec(&json!({"title":"uncommitted","completed":false,"priority":0})).unwrap()],
        )
        .unwrap();
        tx.execute(
            "UPDATE models SET definition=?1 WHERE name='task'",
            params![serde_json::to_vec(&models_v2()["task"]).unwrap()],
        )
        .unwrap();
        // Marker proves both SQL statements ran; neither was committed.
        std::fs::write(marker, b"updated records and model inside uncommitted transaction").unwrap();
        std::process::exit(137);
    }
    let (dir, _, _, _) = fixture();
    let db = dir.path().join("records.db");
    let marker = dir.path().join("child.marker");
    let before = snapshot(&db);
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "uncommitted_process_exit_rolls_back_both_records_and_model_metadata"])
        .env("RIESEL_MIGRATION_RESEARCH_CHILD_DB", &db)
        .env("RIESEL_MIGRATION_RESEARCH_MARKER", &marker)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(137), "child failed to reach forced exit: {}", String::from_utf8_lossy(&output.stderr));
    assert!(marker.is_file(), "child did not run both SQL writes");
    assert_eq!(snapshot(&db), before);
    assert!(SqliteStore::open(&db, models_v1()).is_ok());
}

#[test]
fn consistent_sqlite_vacuum_into_snapshot_reopens_while_original_store_is_live() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("origin.db");
    let backup = dir.path().join("snapshot.db");
    let store = SqliteStore::open(&original, models_v1()).unwrap();
    let record = store.create("task", json!({"title":"in live WAL","completed":false})).unwrap();
    let conn = Connection::open(&original).unwrap();
    conn.execute("VACUUM INTO ?1", params![backup.to_string_lossy().as_ref()]).unwrap();
    let copy = SqliteStore::open(&backup, models_v1()).unwrap();
    assert_eq!(copy.get("task", record.id).unwrap().unwrap().data["title"], "in live WAL");
    assert_eq!(snapshot(&backup).0, 1);
    // This demonstrates a consistent snapshot, not a restore/swap procedure
    // or a power-loss/directory-sync guarantee.
}
