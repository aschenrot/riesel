//! Research-only #119 fixture: real linked SQLite model evolution and single-field Query.
//! Not a Todo application implementation or accepted lifecycle representation.
use riesel_schema::{ModelDefinition, RieselModel, TypeRef};
use riesel_storage::{
    RecordCompare, RecordPredicate, RecordQuery, RecordQueryRequest, RecordStore, StoreError,
};
use riesel_storage_sqlite::{
    EvolutionAuthorization, EvolutionLimits, ModelEvolution, RestoreAuthorization, SnapshotBudget,
    SqliteMaintenance, SqliteStore,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[derive(Debug, Serialize, Deserialize)]
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

#[derive(Debug, Serialize, Deserialize)]
struct CandidateTask {
    title: String,
    completed: bool,
    // Research shape only: production must decide a Rust-derived typed field.
    location: String,
}

impl RieselModel for CandidateTask {
    const MODEL_NAME: &'static str = "task";

    fn model_definition() -> ModelDefinition {
        let mut fields = HistoricalTask::model_definition().fields;
        fields.insert("location".into(), TypeRef::String);
        ModelDefinition { fields }
    }
}

fn models<T: RieselModel>() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([(T::MODEL_NAME.into(), T::model_definition())])
}

fn limits() -> EvolutionLimits {
    EvolutionLimits {
        snapshot: SnapshotBudget {
            max_records: 20,
            max_json_bytes: 300_000,
            max_snapshot_bytes: 1_048_576,
            max_sidecar_bytes: 1_048_576,
            max_elapsed: Duration::from_secs(30),
        },
        max_elapsed: Duration::from_secs(30),
    }
}

fn candidate_plan() -> ModelEvolution {
    ModelEvolution::new(models::<HistoricalTask>(), models::<CandidateTask>())
        .unwrap()
        .transform::<HistoricalTask, CandidateTask, _>(|historical| {
            Ok(CandidateTask {
                title: historical.title,
                completed: historical.completed,
                location: "active".into(),
            })
        })
        .unwrap()
}

fn eq(field: &str, value: serde_json::Value) -> RecordQueryRequest {
    RecordQueryRequest {
        model: "task".into(),
        predicate: Some(RecordPredicate {
            field: field.into(),
            operator: RecordCompare::Eq,
            value: Some(value),
        }),
        limit: 32,
        scan_budget: 256,
        cursor: None,
    }
}

#[test]
fn offline_lifecycle_model_evolution_and_trash_restore_keep_identity_and_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("todo-old.db");
    let backup = dir.path().join("todo-before-upgrade.db");
    let before_restore = dir.path().join("todo-before-restore.db");

    let old = SqliteStore::open(&database, models::<HistoricalTask>()).unwrap();
    let open = old
        .create("task", json!({"title":"Open source","completed":false}))
        .unwrap();
    let done = old
        .create("task", json!({"title":"Done source","completed":true}))
        .unwrap();
    drop(old);

    let maintenance =
        SqliteMaintenance::open_existing(&database, models::<HistoricalTask>(), limits().snapshot)
            .unwrap();
    let preview = maintenance
        .preview_evolution(&candidate_plan(), limits())
        .unwrap();
    assert_eq!(preview.affected_records, 2);
    let receipt = maintenance
        .execute_evolution(
            &candidate_plan(),
            &backup,
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap();
    assert_eq!(receipt.affected_records, 2);

    // Model v1 cannot be opened against migrated state. The retained source
    // backup is independently reopenable as the exact old schema.
    assert!(SqliteStore::open(&database, models::<HistoricalTask>()).is_err());
    let old_backup = SqliteStore::open(&backup, models::<HistoricalTask>()).unwrap();
    assert_eq!(
        old_backup.get("task", open.id).unwrap().unwrap().revision,
        open.revision
    );
    assert_eq!(
        old_backup.get("task", done.id).unwrap().unwrap().revision,
        done.revision
    );
    drop(old_backup);

    let current = SqliteStore::open(&database, models::<CandidateTask>())
        .unwrap()
        .with_query_grants(BTreeSet::from(["task".into()]))
        .unwrap();
    let after_open = current.get("task", open.id).unwrap().unwrap();
    let after_done = current.get("task", done.id).unwrap().unwrap();
    assert_eq!(after_open.revision, open.revision + 1);
    assert_eq!(after_done.revision, done.revision + 1);
    assert_eq!(
        after_open.data,
        json!({"title":"Open source","completed":false,"location":"active"})
    );
    assert_eq!(
        after_done.data,
        json!({"title":"Done source","completed":true,"location":"active"})
    );

    // Moving/restoring one *existing* record uses Storage's observed-revision
    // update: no recreated UUID and no cross-model move transaction needed.
    let trashed = current
        .update(
            "task",
            open.id,
            after_open.revision,
            json!({"title":"Open source","completed":false,"location":"trash"}),
        )
        .unwrap();
    assert_eq!(trashed.id, open.id);
    assert_eq!(trashed.revision, after_open.revision + 1);
    assert!(matches!(
        current.update(
            "task",
            open.id,
            after_open.revision,
            json!({"title":"Stale overwrite","completed":true,"location":"active"}),
        ),
        Err(StoreError::RevisionConflict)
    ));

    // Proof that a single completed=false predicate *includes* trashed Open
    // tasks. Query v1 cannot correctly power an Active-and-Open view alone.
    let open_status = current.query_page(eq("completed", json!(false))).unwrap();
    assert!(open_status.exhausted);
    assert!(
        open_status
            .records
            .iter()
            .any(|record| record.id == open.id)
    );

    let trash_location = current.query_page(eq("location", json!("trash"))).unwrap();
    assert!(trash_location.exhausted);
    assert_eq!(trash_location.records.len(), 1);
    assert_eq!(trash_location.records[0].id, open.id);

    let restored = current
        .update(
            "task",
            open.id,
            trashed.revision,
            json!({"title":"Open source","completed":false,"location":"active"}),
        )
        .unwrap();
    assert_eq!(restored.id, open.id);
    assert_eq!(restored.revision, trashed.revision + 1);
    assert_eq!(restored.data["completed"], false);
    assert!(
        current
            .query_page(eq("location", json!("trash")))
            .unwrap()
            .records
            .is_empty()
    );
    drop(current);

    // Distinct, explicitly authorized *offline* restore discards all writes
    // since the source snapshot; it is not Undo and cannot be automatic.
    let maintenance =
        SqliteMaintenance::open_existing(&database, models::<CandidateTask>(), limits().snapshot)
            .unwrap();
    maintenance
        .restore_from_backup(
            &backup,
            models::<HistoricalTask>(),
            &before_restore,
            limits().snapshot,
            RestoreAuthorization::DiscardPostSnapshotWrites,
        )
        .unwrap();
    let restored_old = SqliteStore::open(&database, models::<HistoricalTask>()).unwrap();
    assert_eq!(
        restored_old.get("task", open.id).unwrap().unwrap().revision,
        open.revision
    );
    assert_eq!(
        restored_old.get("task", done.id).unwrap().unwrap().data["completed"],
        true
    );
}

#[test]
fn failed_todo_lifecycle_transform_keeps_original_database_readable() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("todo-broken-transform.db");
    let old = SqliteStore::open(&database, models::<HistoricalTask>()).unwrap();
    let done = old
        .create("task", json!({"title":"Must survive","completed":true}))
        .unwrap();
    drop(old);

    let rejected = ModelEvolution::new(models::<HistoricalTask>(), models::<CandidateTask>())
        .unwrap()
        .transform::<HistoricalTask, CandidateTask, _>(|historical| {
            if historical.completed {
                return Err(StoreError::Failed("injected transform refusal".into()));
            }
            Ok(CandidateTask {
                title: historical.title,
                completed: historical.completed,
                location: "active".into(),
            })
        })
        .unwrap();
    let maintenance =
        SqliteMaintenance::open_existing(&database, models::<HistoricalTask>(), limits().snapshot)
            .unwrap();
    let error = maintenance.execute_evolution(
        &rejected,
        dir.path().join("recovery.db"),
        limits(),
        EvolutionAuthorization::ExecuteWithVerifiedBackup,
    );
    assert!(error.is_err());
    let still_old = SqliteStore::open(&database, models::<HistoricalTask>()).unwrap();
    let preserved = still_old.get("task", done.id).unwrap().unwrap();
    assert_eq!(preserved.revision, done.revision);
    assert_eq!(preserved.data, done.data);
    drop(still_old);
    assert!(SqliteStore::open(&database, models::<CandidateTask>()).is_err());
}
