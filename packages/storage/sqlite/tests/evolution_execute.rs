//! Integration tests of explicit offline execution, independent backup and rollback.
use riesel_schema::{ModelDefinition, RieselModel, TypeRef};
use riesel_storage::{RecordStore, StoreError};
use riesel_storage_sqlite::{
    EvolutionAuthorization, EvolutionLimits, ModelEvolution, RestoreAuthorization, SnapshotBudget,
    SqliteMaintenance, SqliteStore,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, time::Duration};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct HistoricalTask {
    title: String,
    completed: bool,
}

impl RieselModel for HistoricalTask {
    const MODEL_NAME: &'static str = "task";
    fn model_definition() -> ModelDefinition {
        ModelDefinition {
            fields: BTreeMap::from([
                ("title".into(), TypeRef::String),
                ("completed".into(), TypeRef::Bool),
            ]),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CurrentTask {
    title: String,
    completed: bool,
    priority: i64,
}

impl RieselModel for CurrentTask {
    const MODEL_NAME: &'static str = "task";
    fn model_definition() -> ModelDefinition {
        let mut fields = HistoricalTask::model_definition().fields;
        fields.insert("priority".into(), TypeRef::I64);
        ModelDefinition { fields }
    }
}

#[derive(Serialize, Deserialize)]
struct MetadataTask {
    title: String,
    completed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

impl RieselModel for MetadataTask {
    const MODEL_NAME: &'static str = "task";
    fn model_definition() -> ModelDefinition {
        let mut fields = HistoricalTask::model_definition().fields;
        fields.insert(
            "note".into(),
            TypeRef::Optional {
                item: Box::new(TypeRef::String),
            },
        );
        ModelDefinition { fields }
    }
}

fn source_models() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([
        ("task".into(), HistoricalTask::model_definition()),
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

fn target_models<T: RieselModel>() -> BTreeMap<String, ModelDefinition> {
    let mut models = source_models();
    models.insert("task".into(), T::model_definition());
    models
}

fn budget() -> SnapshotBudget {
    SnapshotBudget {
        max_records: 20,
        max_json_bytes: 300_000,
        max_snapshot_bytes: 1_048_576,
        max_sidecar_bytes: 1_048_576,
        max_elapsed: Duration::from_secs(30),
    }
}
fn limits() -> EvolutionLimits {
    EvolutionLimits {
        snapshot: budget(),
        max_elapsed: Duration::from_secs(30),
    }
}

fn plan() -> ModelEvolution {
    ModelEvolution::new(source_models(), target_models::<CurrentTask>())
        .unwrap()
        .transform::<HistoricalTask, CurrentTask, _>(|old| {
            Ok(CurrentTask {
                title: old.title,
                completed: old.completed,
                priority: 0,
            })
        })
        .unwrap()
}

type StoredRow = (String, Vec<u8>, Vec<u8>, Vec<u8>);
fn rows(path: &Path) -> Vec<StoredRow> {
    let db = Connection::open(path).unwrap();
    let mut stmt = db
        .prepare("SELECT model,id,revision,data FROM records ORDER BY model,id")
        .unwrap();
    stmt.query_map([], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    })
    .unwrap()
    .map(Result::unwrap)
    .collect()
}

fn fixture(path: &Path) -> (Uuid, Uuid) {
    let store = SqliteStore::open(path, source_models()).unwrap();
    let task = store
        .create("task", json!({"title":"original","completed":false}))
        .unwrap();
    let note = store
        .create("note", json!({"task_id":task.id,"text":"reference"}))
        .unwrap();
    drop(store);
    (task.id, note.id)
}

#[test]
fn explicit_typed_execute_preserves_identity_links_and_retains_verified_backup() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let (task_id, note_id) = fixture(&db);
    let original = rows(&db);
    let saved = dir.path().join("pre-migration.db");
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    let preview = maintenance.preview_evolution(&plan(), limits()).unwrap();
    assert_eq!(preview.records, 2);
    assert_eq!(preview.affected_records, 1);

    let receipt = maintenance
        .execute_evolution(
            &plan(),
            &saved,
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap();
    assert_eq!(receipt.records, 2);
    assert_eq!(receipt.affected_records, 1);
    assert_eq!(receipt.retained_backup.path, saved);
    assert_eq!(rows(&saved), original);
    assert!(SqliteStore::open(&db, source_models()).is_err());
    let current = SqliteStore::open(&db, target_models::<CurrentTask>()).unwrap();
    let transformed = current.get("task", task_id).unwrap().unwrap();
    assert_eq!(transformed.revision, 2);
    assert_eq!(transformed.data["priority"], 0);
    let unchanged = current.get("note", note_id).unwrap().unwrap();
    assert_eq!(unchanged.revision, 1);
    assert_eq!(unchanged.data["task_id"], task_id.to_string());
    drop(current);
    let after = rows(&db);
    assert_eq!(
        after
            .iter()
            .filter(|row| row.0 == "note")
            .collect::<Vec<_>>(),
        original
            .iter()
            .filter(|row| row.0 == "note")
            .collect::<Vec<_>>()
    );

    // Recovery is a separate, deliberately authorized offline action.
    let maintenance =
        SqliteMaintenance::open_existing(&db, target_models::<CurrentTask>(), budget()).unwrap();
    let before_restore = dir.path().join("pre-restore.db");
    maintenance
        .restore_from_backup(
            &saved,
            source_models(),
            &before_restore,
            budget(),
            RestoreAuthorization::DiscardPostSnapshotWrites,
        )
        .unwrap();
    assert_eq!(rows(&db), original);
    assert_eq!(rows(&before_restore), after);
    drop(SqliteStore::open(&db, source_models()).unwrap());
}

#[test]
fn metadata_only_transition_still_increments_revision_without_rewriting_json() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let (id, _) = fixture(&db);
    let original = rows(&db);
    let changed = ModelEvolution::new(source_models(), target_models::<MetadataTask>())
        .unwrap()
        .transform::<HistoricalTask, MetadataTask, _>(|old| {
            Ok(MetadataTask {
                title: old.title,
                completed: old.completed,
                note: None,
            })
        })
        .unwrap();
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    maintenance
        .execute_evolution(
            &changed,
            dir.path().join("pre.db"),
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap();
    let current = SqliteStore::open(&db, target_models::<MetadataTask>()).unwrap();
    let result = current.get("task", id).unwrap().unwrap();
    assert_eq!(result.revision, 2);
    assert_eq!(result.data, json!({"title":"original","completed":false}));
    let after = rows(&db);
    assert_eq!(
        original.iter().find(|r| r.0 == "task").unwrap().3,
        after.iter().find(|r| r.0 == "task").unwrap().3,
    );
}

#[test]
fn failing_transform_retains_backup_without_committing_any_change() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let before = rows(&db);
    let rejected = ModelEvolution::new(source_models(), target_models::<CurrentTask>())
        .unwrap()
        .transform::<HistoricalTask, CurrentTask, _>(|_| {
            Err(StoreError::Failed(
                "injected typed transform failure".into(),
            ))
        })
        .unwrap();
    let backup = dir.path().join("retained.db");
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    assert!(
        maintenance
            .execute_evolution(
                &rejected,
                &backup,
                limits(),
                EvolutionAuthorization::ExecuteWithVerifiedBackup,
            )
            .is_err()
    );
    assert!(backup.exists());
    assert_eq!(rows(&backup), before);
    assert_eq!(rows(&db), before);
    drop(SqliteStore::open(&db, source_models()).unwrap());
}

#[test]
fn sqlite_failure_after_first_update_rolls_back_all_rows_and_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let store = SqliteStore::open(&db, source_models()).unwrap();
    store
        .create("task", json!({"title":"second","completed":true}))
        .unwrap();
    drop(store);
    // Force the trigger on the *last* sorted task row so at least one
    // UPDATE succeeds before SQLite raises an error. UUID allocation order
    // does not define SQL primary-key order.
    let last_task_id = rows(&db)
        .into_iter()
        .filter(|row| row.0 == "task")
        .map(|row| row.1)
        .max()
        .unwrap();
    let reject_id: String = last_task_id
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    let conn = Connection::open(&db).unwrap();
    conn.execute_batch(&format!(
        "CREATE TRIGGER reject_evolution BEFORE UPDATE ON records
         WHEN NEW.model='task' AND hex(NEW.id)='{}'
         BEGIN SELECT RAISE(ABORT, 'injected update failure'); END;",
        reject_id
    ))
    .unwrap();
    drop(conn);
    let before = rows(&db);
    let backup = dir.path().join("retained.db");
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    let error = maintenance
        .execute_evolution(
            &plan(),
            &backup,
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap_err();
    assert!(matches!(error, StoreError::Failed(_)));
    assert_eq!(rows(&db), before);
    assert_eq!(rows(&backup), before);
    drop(SqliteStore::open(&db, source_models()).unwrap());
}

#[test]
fn occupied_backup_and_invalid_limits_prevent_evolution_and_write() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let before = rows(&db);
    let occupied = dir.path().join("occupied.db");
    std::fs::write(&occupied, b"must survive").unwrap();
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    assert!(
        maintenance
            .execute_evolution(
                &plan(),
                &occupied,
                limits(),
                EvolutionAuthorization::ExecuteWithVerifiedBackup,
            )
            .is_err()
    );
    assert_eq!(rows(&db), before);
    assert_eq!(std::fs::read(&occupied).unwrap(), b"must survive");

    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    let impossible = EvolutionLimits {
        snapshot: SnapshotBudget {
            max_snapshot_bytes: u64::MAX,
            ..budget()
        },
        ..limits()
    };
    assert!(
        maintenance
            .execute_evolution(
                &plan(),
                dir.path().join("unpublished.db"),
                impossible,
                EvolutionAuthorization::ExecuteWithVerifiedBackup,
            )
            .is_err()
    );
    assert_eq!(rows(&db), before);
}

#[test]
fn max_revision_refuses_evolution_before_transaction_writes_and_keeps_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let (id, _) = fixture(&db);
    let direct = Connection::open(&db).unwrap();
    direct
        .execute(
            "UPDATE records SET revision=?1 WHERE model='task' AND id=?2",
            rusqlite::params![&u64::MAX.to_be_bytes()[..], id.as_bytes()],
        )
        .unwrap();
    drop(direct);
    let before = rows(&db);
    let saved = dir.path().join("overflow-backup.db");
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    assert!(
        maintenance
            .execute_evolution(
                &plan(),
                &saved,
                limits(),
                EvolutionAuthorization::ExecuteWithVerifiedBackup,
            )
            .is_err()
    );
    assert_eq!(rows(&db), before);
    assert_eq!(rows(&saved), before);
    let old = SqliteStore::open(&db, source_models()).unwrap();
    assert_eq!(old.get("task", id).unwrap().unwrap().revision, u64::MAX);
    drop(old);
    assert!(SqliteStore::open(&db, target_models::<CurrentTask>()).is_err());
}

#[test]
fn unexpected_schema_valid_trigger_rewrite_rolls_back_instead_of_committing() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let original = rows(&db);
    let connection = Connection::open(&db).unwrap();
    let replacement =
        serde_json::to_vec(&json!({"title":"replaced","completed":false,"priority":0})).unwrap();
    let hex_payload: String = replacement
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    connection
        .execute_batch(&format!(
            "CREATE TRIGGER rewrite_evolution AFTER UPDATE ON records
             WHEN NEW.model='task'
             BEGIN
               UPDATE records SET data=x'{hex_payload}'
               WHERE model=NEW.model AND id=NEW.id;
             END;"
        ))
        .unwrap();
    drop(connection);

    let saved = dir.path().join("retained.db");
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    let error = maintenance
        .execute_evolution(
            &plan(),
            &saved,
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap_err();
    assert!(
        format!("{error}").contains("typed transform output"),
        "unexpectedly accepted or misclassified trigger rewrite: {error}"
    );
    assert_eq!(rows(&db), original);
    assert_eq!(rows(&saved), original);
    drop(SqliteStore::open(&db, source_models()).unwrap());
}
