//! Research-only #119: migration from Todo's *actual* Rust-derived TaskData
//! to a candidate Rust-derived two-state lifecycle. Not production migration.
use riesel::RieselModel;
use riesel::storage::{RecordStore, StoreError};
use riesel_storage_sqlite::{
    EvolutionAuthorization, EvolutionLimits, ModelEvolution, RestoreAuthorization, SnapshotBudget,
    SqliteMaintenance, SqliteStore,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, process::Command, time::Duration};
use todo::TaskData;

#[derive(Debug, Clone, Serialize, Deserialize, RieselModel)]
#[riesel(model = "task")]
#[serde(deny_unknown_fields)]
struct CandidateTask {
    title: String,
    completed: bool,
    trashed: bool,
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

#[test]
fn real_todo_derived_old_to_bool_trash_migration_and_legacy_cli_boundary() {
    let historical = || BTreeMap::from([("task".into(), TaskData::model_definition())]);
    let candidate = || BTreeMap::from([("task".into(), CandidateTask::model_definition())]);

    assert_eq!(TaskData::MODEL_NAME, CandidateTask::MODEL_NAME);
    assert!(
        CandidateTask::model_definition()
            .validate_record(&json!({
                "title": "invalid bool state", "completed": false, "trashed": "trash"
            }))
            .is_err()
    );

    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("existing-todo.db");
    let backup = dir.path().join("verified-old-todo.db");
    let before_restore = dir.path().join("retain-evolved-todo.db");

    let old = SqliteStore::open(&database, historical()).unwrap();
    let open = old
        .create(
            "task",
            serde_json::to_value(TaskData {
                title: "Keep this open".into(),
                completed: false,
            })
            .unwrap(),
        )
        .unwrap();
    let done = old
        .create(
            "task",
            serde_json::to_value(TaskData {
                title: "Keep this done".into(),
                completed: true,
            })
            .unwrap(),
        )
        .unwrap();
    drop(old);

    let plan = ModelEvolution::new(historical(), candidate())
        .unwrap()
        .transform::<TaskData, CandidateTask, _>(|prior| {
            Ok(CandidateTask {
                title: prior.title,
                completed: prior.completed,
                trashed: false,
            })
        })
        .unwrap();
    let maintenance =
        SqliteMaintenance::open_existing(&database, historical(), limits().snapshot).unwrap();
    let preview = maintenance.preview_evolution(&plan, limits()).unwrap();
    assert_eq!(preview.affected_records, 2);
    let receipt = maintenance
        .execute_evolution(
            &plan,
            &backup,
            limits(),
            EvolutionAuthorization::ExecuteWithVerifiedBackup,
        )
        .unwrap();
    assert_eq!(receipt.affected_records, 2);
    drop(maintenance);

    let original_backup = SqliteStore::open(&backup, historical()).unwrap();
    assert_eq!(
        original_backup
            .get("task", open.id)
            .unwrap()
            .unwrap()
            .revision,
        open.revision
    );
    assert_eq!(
        original_backup
            .get("task", done.id)
            .unwrap()
            .unwrap()
            .revision,
        done.revision
    );
    drop(original_backup);

    // Old Todo binaries cannot open the changed model metadata. A release
    // must deliberately coordinate binaries, old clients and offline upgrade.
    assert!(SqliteStore::open(&database, historical()).is_err());
    let legacy_cli = Command::new(env!("CARGO_BIN_EXE_todo_cli"))
        .arg("--db")
        .arg(&database)
        .arg("list")
        .output()
        .unwrap();
    assert!(
        !legacy_cli.status.success(),
        "the old CLI must fail closed on evolved metadata"
    );

    let current = SqliteStore::open(&database, candidate()).unwrap();
    let migrated_open = current.get("task", open.id).unwrap().unwrap();
    let migrated_done = current.get("task", done.id).unwrap().unwrap();
    assert_eq!(migrated_open.id, open.id);
    assert_eq!(migrated_open.revision, open.revision + 1);
    assert_eq!(
        migrated_open.data,
        json!({"title":"Keep this open","completed":false,"trashed":false})
    );
    assert_eq!(migrated_done.id, done.id);
    assert_eq!(migrated_done.revision, done.revision + 1);
    assert_eq!(
        migrated_done.data,
        json!({"title":"Keep this done","completed":true,"trashed":false})
    );

    let moved = current
        .update(
            "task",
            done.id,
            migrated_done.revision,
            json!({"title":"Keep this done","completed":true,"trashed":true}),
        )
        .unwrap();
    assert_eq!(moved.id, done.id);
    assert_eq!(moved.revision, migrated_done.revision + 1);
    assert!(matches!(
        current.update(
            "task",
            done.id,
            migrated_done.revision,
            json!({"title":"Stale update","completed":false,"trashed":false})
        ),
        Err(StoreError::RevisionConflict)
    ));
    drop(current);

    // Persisting the real two-state shape must preserve trashed completion.
    let reopened = SqliteStore::open(&database, candidate()).unwrap();
    let persisted = reopened.get("task", done.id).unwrap().unwrap();
    assert_eq!(persisted.id, done.id);
    assert_eq!(persisted.revision, moved.revision);
    assert_eq!(persisted.data["completed"], true);
    assert_eq!(persisted.data["trashed"], true);
    drop(reopened);

    // Offline restore is deliberate and loses post-snapshot writes. It is
    // never a user-level Undo or automatic recovery from an uncertain write.
    let maintenance =
        SqliteMaintenance::open_existing(&database, candidate(), limits().snapshot).unwrap();
    maintenance
        .restore_from_backup(
            &backup,
            historical(),
            &before_restore,
            limits().snapshot,
            RestoreAuthorization::DiscardPostSnapshotWrites,
        )
        .unwrap();
    drop(maintenance);

    let back_to_old = SqliteStore::open(&database, historical()).unwrap();
    assert_eq!(
        back_to_old.get("task", done.id).unwrap().unwrap().revision,
        done.revision
    );
    drop(back_to_old);

    // The independently retained pre-restore evolved file remains openable.
    let retained = SqliteStore::open(&before_restore, candidate()).unwrap();
    assert_eq!(
        retained.get("task", done.id).unwrap().unwrap().data["trashed"],
        true
    );
    drop(retained);

    let restored_cli = Command::new(env!("CARGO_BIN_EXE_todo_cli"))
        .arg("--db")
        .arg(&database)
        .arg("list")
        .output()
        .unwrap();
    assert!(restored_cli.status.success());
    assert!(String::from_utf8_lossy(&restored_cli.stdout).contains("Keep this done"));
}
