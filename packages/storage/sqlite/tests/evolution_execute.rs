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

#[test]
fn verified_retained_archive_excludes_a_cooperating_provider_until_commit() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let archive = dir.path().join("retained.db");
    let archive_for_transform = archive.clone();
    let source_for_transform = source_models();
    let guarded = ModelEvolution::new(source_models(), target_models::<CurrentTask>())
        .unwrap()
        .transform::<HistoricalTask, CurrentTask, _>(move |old| {
            assert!(
                SqliteStore::open(&archive_for_transform, source_for_transform.clone()).is_err(),
                "a cooperating provider must not own the retained snapshot during evolution"
            );
            Ok(CurrentTask {
                title: old.title,
                completed: old.completed,
                priority: 0,
            })
        })
        .unwrap();
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    maintenance
        .execute_evolution(
            &guarded,
            &archive,
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap();
    drop(SqliteStore::open(&archive, source_models()).unwrap());
    drop(SqliteStore::open(&db, target_models::<CurrentTask>()).unwrap());
}

#[derive(Serialize, Deserialize)]
struct RenamedTask {
    name: String,
    completed: bool,
}

impl RieselModel for RenamedTask {
    const MODEL_NAME: &'static str = "task";
    fn model_definition() -> ModelDefinition {
        ModelDefinition {
            fields: BTreeMap::from([
                ("name".into(), TypeRef::String),
                ("completed".into(), TypeRef::Bool),
            ]),
        }
    }
}

fn validate_note_links(
    records: &riesel_storage_sqlite::EvolutionRecords<'_>,
) -> Result<(), StoreError> {
    records.for_each("note", |note| {
        let id = note.data["task_id"]
            .as_str()
            .ok_or_else(|| StoreError::Failed("missing note task_id".into()))?;
        let id = Uuid::parse_str(id).map_err(|error| StoreError::Failed(error.to_string()))?;
        if records.get("task", id)?.is_none() {
            return Err(StoreError::Failed("orphan note task reference".into()));
        }
        Ok(())
    })
}

#[test]
fn successful_cross_model_invariant_sees_transformed_target_records() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let (task_id, note_id) = fixture(&db);
    let original = rows(&db);
    let saved = dir.path().join("before.db");
    let checked = plan().invariant(move |records| {
        validate_note_links(records)?;
        let task = records
            .get("task", task_id)?
            .ok_or_else(|| StoreError::Failed("missing migrated task".into()))?;
        if task.revision != 2 || task.data["priority"] != 0 {
            return Err(StoreError::Failed(
                "invariant saw unexpected target task".into(),
            ));
        }
        if records.get("not_registered", note_id).is_ok() {
            return Err(StoreError::Failed(
                "unknown invariant model accepted".into(),
            ));
        }
        Ok(())
    });
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    let receipt = maintenance
        .execute_evolution(
            &checked,
            &saved,
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap();
    assert_eq!(receipt.records, 2);
    assert_eq!(rows(&saved), original);
    let current = SqliteStore::open(&db, target_models::<CurrentTask>()).unwrap();
    assert_eq!(
        current.get("note", note_id).unwrap().unwrap().data["task_id"],
        task_id.to_string()
    );
    drop(current);
}

#[test]
fn orphan_reference_invariant_rolls_back_after_typed_updates() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let store = SqliteStore::open(&db, source_models()).unwrap();
    store
        .create("note", json!({"task_id": Uuid::new_v4(), "text":"orphan"}))
        .unwrap();
    drop(store);
    let before = rows(&db);
    let saved = dir.path().join("retained.db");
    let checked = plan().invariant(validate_note_links);
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    let failure = maintenance
        .execute_evolution(
            &checked,
            &saved,
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap_err();
    assert!(
        format!("{failure}").contains("orphan note task reference"),
        "{failure}"
    );
    assert_eq!(rows(&db), before);
    assert_eq!(rows(&saved), before);
    drop(SqliteStore::open(&db, source_models()).unwrap());
    assert!(SqliteStore::open(&db, target_models::<CurrentTask>()).is_err());
}

#[test]
fn deliberate_field_rename_is_a_typed_transform_not_an_inferred_upgrade() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    let (task_id, note_id) = fixture(&db);
    let before = rows(&db);
    let target = target_models::<RenamedTask>();
    let renamed = ModelEvolution::new(source_models(), target.clone())
        .unwrap()
        .transform::<HistoricalTask, RenamedTask, _>(|old| {
            Ok(RenamedTask {
                name: old.title,
                completed: old.completed,
            })
        })
        .unwrap()
        .invariant(validate_note_links);
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    maintenance
        .execute_evolution(
            &renamed,
            dir.path().join("old.db"),
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap();
    let current = SqliteStore::open(&db, target).unwrap();
    let task = current.get("task", task_id).unwrap().unwrap();
    assert_eq!(task.revision, 2);
    assert_eq!(task.data["name"], "original");
    assert!(task.data.get("title").is_none());
    let note = current.get("note", note_id).unwrap().unwrap();
    assert_eq!(note.revision, 1);
    drop(current);
    assert_eq!(
        rows(&db).iter().find(|row| row.0 == "note").unwrap(),
        before.iter().find(|row| row.0 == "note").unwrap()
    );
}


fn persisted_definitions(path: &Path) -> Vec<(String, Vec<u8>)> {
    let conn = Connection::open(path).unwrap();
    let mut statement = conn
        .prepare("SELECT name,definition FROM models ORDER BY name")
        .unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn unsupported_or_incomplete_evolution_plan_cannot_create_backup_or_change_source() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let before = rows(&db);
    let definitions = persisted_definitions(&db);

    assert!(ModelEvolution::new(source_models(), source_models()).is_err());
    let mut added = target_models::<CurrentTask>();
    added.insert("unexpected".into(), HistoricalTask::model_definition());
    assert!(ModelEvolution::new(source_models(), added).is_err());
    let mut removed = target_models::<CurrentTask>();
    removed.remove("note");
    assert!(ModelEvolution::new(source_models(), removed).is_err());

    let without_transform =
        ModelEvolution::new(source_models(), target_models::<CurrentTask>()).unwrap();
    let backup = dir.path().join("not-created.db");
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    assert!(
        maintenance
            .execute_evolution(
                &without_transform,
                &backup,
                limits(),
                EvolutionAuthorization::ExecuteWithVerifiedBackup,
            )
            .is_err()
    );
    assert!(!backup.exists(), "invalid plan must not create an archive");
    assert_eq!(rows(&db), before);
    assert_eq!(persisted_definitions(&db), definitions);

    // The normal provider must never infer or apply a schema update.
    assert!(SqliteStore::open(&db, target_models::<CurrentTask>()).is_err());
    assert_eq!(rows(&db), before);
    drop(SqliteStore::open(&db, source_models()).unwrap());

    assert!(
        ModelEvolution::new(source_models(), target_models::<CurrentTask>())
            .unwrap()
            .transform::<HistoricalTask, MetadataTask, _>(|old| {
                Ok(MetadataTask {
                    title: old.title,
                    completed: old.completed,
                    note: None,
                })
            })
            .is_err(),
        "typed target definition must match the declared target plan"
    );
    assert!(
        plan()
            .transform::<HistoricalTask, CurrentTask, _>(|old| {
                Ok(CurrentTask {
                    title: old.title,
                    completed: old.completed,
                    priority: 0,
                })
            })
            .is_err(),
        "a second transform for the same persistent model must be rejected"
    );
}

#[test]
fn corrupted_source_metadata_and_record_payloads_fail_before_backup_or_mutation() {
    for fault in [
        "changed-metadata",
        "extra-model",
        "unknown-record-model",
        "invalid-json",
        "schema-invalid-json",
        "oversized-source-record",
        "wrong-physical-format",
        "short-revision",
        "short-uuid",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("source.db");
        let (task_id, _) = fixture(&db);
        let connection = Connection::open(&db).unwrap();
        match fault {
            "changed-metadata" => {
                connection
                    .execute(
                        "UPDATE models SET definition=?1 WHERE name='task'",
                        rusqlite::params![
                            serde_json::to_vec(&CurrentTask::model_definition()).unwrap()
                        ],
                    )
                    .unwrap();
            }
            "extra-model" => {
                connection
                    .execute(
                        "INSERT INTO models(name,definition) VALUES(?1,?2)",
                        rusqlite::params![
                            "unregistered",
                            serde_json::to_vec(&HistoricalTask::model_definition()).unwrap()
                        ],
                    )
                    .unwrap();
            }
            "unknown-record-model" => {
                connection
                    .execute(
                        "INSERT INTO records(model,id,revision,data) VALUES(?1,?2,?3,?4)",
                        rusqlite::params![
                            "unregistered",
                            Uuid::new_v4().as_bytes(),
                            &1_u64.to_be_bytes()[..],
                            serde_json::to_vec(&json!({"title":"orphan"})).unwrap()
                        ],
                    )
                    .unwrap();
            }
            "invalid-json" | "schema-invalid-json" | "oversized-source-record" => {
                let payload = match fault {
                    "invalid-json" => b"not valid JSON".to_vec(),
                    "schema-invalid-json" => {
                        serde_json::to_vec(&json!({"title":42,"completed":false})).unwrap()
                    }
                    _ => serde_json::to_vec(&json!({
                        "title":"x".repeat(65_536),
                        "completed":false
                    }))
                    .unwrap(),
                };
                connection
                    .execute(
                        "UPDATE records SET data=?1 WHERE model='task' AND id=?2",
                        rusqlite::params![payload, task_id.as_bytes()],
                    )
                    .unwrap();
            }
            "wrong-physical-format" => {
                connection.pragma_update(None, "user_version", 2).unwrap();
            }
            "short-revision" | "short-uuid" => {
                // Simulate a corrupt/nonconforming SQLite writer without
                // changing the maintained v1 schema's CHECK constraints.
                connection
                    .pragma_update(None, "ignore_check_constraints", "ON")
                    .unwrap();
                if fault == "short-revision" {
                    connection
                        .execute(
                            "UPDATE records SET revision=x'01' WHERE model='task'",
                            [],
                        )
                        .unwrap();
                } else {
                    connection
                        .execute(
                            "UPDATE records SET id=x'01' WHERE model='task' AND id=?1",
                            rusqlite::params![task_id.as_bytes()],
                        )
                        .unwrap();
                }
            }
            _ => unreachable!(),
        }
        drop(connection);
        let before = rows(&db);
        let model_bytes = persisted_definitions(&db);
        let destination = dir.path().join("must-not-exist.db");
        assert!(
            SqliteMaintenance::open_existing(&db, source_models(), budget()).is_err(),
            "invalid source accepted: {fault}"
        );
        assert!(!destination.exists());
        assert_eq!(rows(&db), before, "source changed on refusal: {fault}");
        assert_eq!(
            persisted_definitions(&db),
            model_bytes,
            "metadata changed on refusal: {fault}"
        );
    }
}

#[test]
fn bounded_evolution_rejects_record_file_byte_and_time_limits_without_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let before = rows(&db);
    let definitions = persisted_definitions(&db);

    let too_few_records = SnapshotBudget {
        max_records: 1,
        ..budget()
    };
    assert!(SqliteMaintenance::open_existing(&db, source_models(), too_few_records).is_err());

    let too_few_bytes = SnapshotBudget {
        max_json_bytes: 1,
        ..budget()
    };
    assert!(SqliteMaintenance::open_existing(&db, source_models(), too_few_bytes).is_err());

    let too_small_file = SnapshotBudget {
        max_snapshot_bytes: 1,
        ..budget()
    };
    assert!(SqliteMaintenance::open_existing(&db, source_models(), too_small_file).is_err());

    let expired = EvolutionLimits {
        max_elapsed: Duration::from_nanos(1),
        ..limits()
    };
    let expired_archive = dir.path().join("expired.db");
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    assert!(
        maintenance
            .execute_evolution(
                &plan(),
                &expired_archive,
                expired,
                EvolutionAuthorization::ExecuteWithVerifiedBackup,
            )
            .is_err()
    );
    assert!(!expired_archive.exists());

    // The complete old database fits this limit, but the new field does not.
    // The verified backup must be retained and the IMMEDIATE txn rolled back.
    let old_bytes = before
        .iter()
        .map(|row| row.3.len() as u64)
        .sum::<u64>();
    let tight = EvolutionLimits {
        snapshot: SnapshotBudget {
            max_json_bytes: old_bytes,
            ..budget()
        },
        ..limits()
    };
    let retained = dir.path().join("projected-overflow.db");
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    assert!(
        maintenance
            .execute_evolution(
                &plan(),
                &retained,
                tight,
                EvolutionAuthorization::ExecuteWithVerifiedBackup,
            )
            .is_err()
    );
    assert_eq!(rows(&retained), before);
    assert_eq!(rows(&db), before);
    assert_eq!(persisted_definitions(&db), definitions);
}

#[test]
fn oversized_typed_result_and_late_transform_failure_retain_recoverable_source() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("source.db");
    fixture(&db);
    let store = SqliteStore::open(&db, source_models()).unwrap();
    store
        .create("task", json!({"title":"second","completed":true}))
        .unwrap();
    drop(store);
    let before = rows(&db);
    let definitions = persisted_definitions(&db);

    let too_large = ModelEvolution::new(source_models(), target_models::<CurrentTask>())
        .unwrap()
        .transform::<HistoricalTask, CurrentTask, _>(|old| {
            Ok(CurrentTask {
                title: "x".repeat(65_536),
                completed: old.completed,
                priority: 0,
            })
        })
        .unwrap();
    let size_backup = dir.path().join("large-result.db");
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    assert!(
        maintenance
            .execute_evolution(
                &too_large,
                &size_backup,
                limits(),
                EvolutionAuthorization::ExecuteWithVerifiedBackup,
            )
            .is_err()
    );
    assert_eq!(rows(&size_backup), before);
    assert_eq!(rows(&db), before);

    let evaluated = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&evaluated);
    let failing = ModelEvolution::new(source_models(), target_models::<CurrentTask>())
        .unwrap()
        .transform::<HistoricalTask, CurrentTask, _>(move |old| {
            if count.fetch_add(1, Ordering::SeqCst) != 0 {
                return Err(StoreError::Failed(
                    "injected failure on second typed record".into(),
                ));
            }
            Ok(CurrentTask {
                title: old.title,
                completed: old.completed,
                priority: 0,
            })
        })
        .unwrap();
    let failing_backup = dir.path().join("later-failure.db");
    let maintenance = SqliteMaintenance::open_existing(&db, source_models(), budget()).unwrap();
    let error = maintenance
        .execute_evolution(
            &failing,
            &failing_backup,
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap_err();
    assert!(format!("{error}").contains("second typed record"));
    assert_eq!(evaluated.load(Ordering::SeqCst), 2);
    assert_eq!(rows(&failing_backup), before);
    assert_eq!(rows(&db), before);
    assert_eq!(persisted_definitions(&db), definitions);
    drop(SqliteStore::open(&db, source_models()).unwrap());
    assert!(SqliteStore::open(&db, target_models::<CurrentTask>()).is_err());
}
