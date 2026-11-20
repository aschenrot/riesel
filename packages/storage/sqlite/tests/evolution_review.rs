//! Operator review sessions pin a source snapshot and a single Rust typed plan.
use riesel_schema::{ModelDefinition, RieselModel, TypeRef};
use riesel_storage::RecordStore;
use riesel_storage_sqlite::{
    EvolutionLimits, ModelEvolution, ReviewedEvolutionAuthorization, SnapshotBudget,
    SqliteMaintenance, SqliteStore,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, path::Path, time::Duration};

#[derive(Serialize, Deserialize)]
struct Previous {
    title: String,
}
impl RieselModel for Previous {
    const MODEL_NAME: &'static str = "task";
    fn model_definition() -> ModelDefinition {
        ModelDefinition {
            fields: BTreeMap::from([("title".into(), TypeRef::String)]),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Next {
    title: String,
    priority: i64,
}
impl RieselModel for Next {
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

fn source() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([("task".into(), Previous::model_definition())])
}
fn target() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([("task".into(), Next::model_definition())])
}
fn plan() -> ModelEvolution {
    ModelEvolution::new(source(), target())
        .unwrap()
        .transform::<Previous, Next, _>(|old| {
            Ok(Next {
                title: old.title,
                priority: 0,
            })
        })
        .unwrap()
}
fn limits() -> EvolutionLimits {
    EvolutionLimits {
        snapshot: SnapshotBudget {
            max_records: 20,
            max_json_bytes: 200_000,
            max_snapshot_bytes: 1_048_576,
            max_sidecar_bytes: 1_048_576,
            max_elapsed: Duration::from_secs(30),
        },
        max_elapsed: Duration::from_secs(30),
    }
}
fn setup(path: &Path) -> uuid::Uuid {
    let provider = SqliteStore::open(path, source()).unwrap();
    let id = provider
        .create("task", json!({"title":"original"}))
        .unwrap()
        .id;
    drop(provider);
    id
}

#[test]
fn reviewed_execute_fences_revision_and_retains_the_exact_preimage() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("current.db");
    let archive = dir.path().join("previous.db");
    let id = setup(&database);
    let plan = plan();
    let maintenance =
        SqliteMaintenance::open_existing(&database, source(), limits().snapshot).unwrap();
    let reviewed = maintenance.review_evolution(&plan, limits()).unwrap();
    assert_eq!(reviewed.preview().records, 1);
    assert_eq!(reviewed.preview().affected_records, 1);
    assert_eq!(reviewed.preview().changed_models, 1);
    assert_eq!(reviewed.database_path(), database.canonicalize().unwrap());
    assert!(!archive.exists());
    // Preview retains the maintenance owner; a cooperating live provider
    // cannot be launched while the operator considers the decision.
    assert!(SqliteStore::open(&database, source()).is_err());
    let receipt = reviewed
        .execute(
            &archive,
            ReviewedEvolutionAuthorization::ApproveInspectedPreview,
        )
        .unwrap();
    assert_eq!(receipt.retained_backup.path, archive);
    assert_eq!(receipt.affected_records, 1);
    assert!(SqliteStore::open(&database, source()).is_err());
    let current = SqliteStore::open(&database, target()).unwrap();
    let row = current.get("task", id).unwrap().unwrap();
    assert_eq!(row.revision, 2);
    assert_eq!(row.data["priority"], 0);
    drop(current);
    let historical = SqliteStore::open(&archive, source()).unwrap();
    let original = historical.get("task", id).unwrap().unwrap();
    assert_eq!(original.revision, 1);
    assert_eq!(original.data, json!({"title":"original"}));
}

#[test]
fn byte_neutral_uncooperative_edit_after_preview_fails_closed_before_update() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("current.db");
    let archive = dir.path().join("post-drift-snapshot.db");
    let id = setup(&database);
    let plan = plan();
    let maintenance =
        SqliteMaintenance::open_existing(&database, source(), limits().snapshot).unwrap();
    let reviewed = maintenance.review_evolution(&plan, limits()).unwrap();

    // Simulate a legacy direct SQLite writer that ignores the cooperative
    // owner. Same persisted byte length and revision, different JSON.
    let direct = Connection::open(&database).unwrap();
    let initial: Vec<u8> = direct
        .query_row("SELECT data FROM records WHERE model='task'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let modified = serde_json::to_vec(&json!({"title":"modified"})).unwrap();
    assert_eq!(initial.len(), modified.len());
    direct
        .execute(
            "UPDATE records SET data=?1 WHERE model='task' AND id=?2",
            rusqlite::params![modified, id.as_bytes()],
        )
        .unwrap();
    drop(direct);
    let error = reviewed
        .execute(
            &archive,
            ReviewedEvolutionAuthorization::ApproveInspectedPreview,
        )
        .unwrap_err();
    assert!(format!("{error}").contains("source drift"), "{error}");
    let current = SqliteStore::open(&database, source()).unwrap();
    let unchanged = current.get("task", id).unwrap().unwrap();
    assert_eq!(unchanged.revision, 1);
    assert_eq!(unchanged.data["title"], "modified");
    drop(current);
    // The archived pre-mutation image was validly retained; the rejected
    // migration never ran. The archive reflects the most recent raw source.
    let saved = SqliteStore::open(&archive, source()).unwrap();
    assert_eq!(
        saved.get("task", id).unwrap().unwrap().data["title"],
        "modified"
    );
    drop(saved);
    assert!(SqliteStore::open(&database, target()).is_err());
}

#[test]
fn dropping_unapproved_review_has_no_mutation_or_backup_side_effect() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("current.db");
    let archive = dir.path().join("never-authorized.db");
    let id = setup(&database);
    let plan = plan();
    let maintenance =
        SqliteMaintenance::open_existing(&database, source(), limits().snapshot).unwrap();
    let reviewed = maintenance.review_evolution(&plan, limits()).unwrap();
    assert_eq!(reviewed.preview().records, 1);
    drop(reviewed);
    assert!(!archive.exists());
    let old = SqliteStore::open(&database, source()).unwrap();
    assert_eq!(old.get("task", id).unwrap().unwrap().revision, 1);
    drop(old);
    assert!(SqliteStore::open(&database, target()).is_err());
}

#[test]
fn added_row_between_preview_and_execute_is_rejected_without_schema_change() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("current.db");
    let archive = dir.path().join("after-insert.db");
    setup(&database);
    let plan = plan();
    let maintenance =
        SqliteMaintenance::open_existing(&database, source(), limits().snapshot).unwrap();
    let reviewed = maintenance.review_evolution(&plan, limits()).unwrap();
    let direct = Connection::open(&database).unwrap();
    direct
        .execute(
            "INSERT INTO records(model,id,revision,data) VALUES (?1,?2,?3,?4)",
            rusqlite::params![
                "task",
                uuid::Uuid::new_v4().as_bytes(),
                &1_u64.to_be_bytes()[..],
                serde_json::to_vec(&json!({"title":"later"})).unwrap(),
            ],
        )
        .unwrap();
    drop(direct);
    let error = reviewed
        .execute(
            &archive,
            ReviewedEvolutionAuthorization::ApproveInspectedPreview,
        )
        .unwrap_err();
    assert!(format!("{error}").contains("source drift"), "{error}");
    assert!(archive.exists());
    assert!(SqliteStore::open(&database, target()).is_err());
    drop(SqliteStore::open(&database, source()).unwrap());
}
