//! Verify *actual production adapter* transaction boundaries with real OS
//! process death, not a duplicate research-only SQL transaction implementation.
//! Requires the opt-in v2-fault-injection feature. No runtime activation,
//! hostile-client credential or power-loss guarantee is asserted.
#![cfg(feature = "v2-fault-injection")]

use riesel_app::{
    AccessMode, ExecutionPlanDraft, OperationKey, OperationNode, PlanAuthoringSource,
    PlanLayoutDraft, PlanLayoutPoint, PlanRevisionError, PlanRevisionRepository,
    PlanSemanticCandidate,
};
use riesel_storage_sqlite::{SqlitePlanRevisionRepository, V2FaultStage};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tempfile::tempdir;

const PLAN: &str = "v2.crash.plan";
const STAMP: &str = "trusted-test-app.1";

fn candidate(revision: u64) -> PlanSemanticCandidate {
    let plan = ExecutionPlanDraft::new(PLAN, revision)
        .operation(
            OperationNode::new("step", OperationKey::new("app.step", 1).unwrap())
                .access("data", AccessMode::Write),
        )
        .validate()
        .unwrap();
    PlanSemanticCandidate::from_rust(&plan).unwrap()
}

fn layout() -> PlanLayoutDraft {
    PlanLayoutDraft {
        layout_id: "desktop".to_owned(),
        semantic_anchor: 1,
        node_positions: BTreeMap::from([(
            "step".to_owned(),
            PlanLayoutPoint { x: 5, y: 10 },
        )]),
    }
}

fn repo(db: &Path) -> SqlitePlanRevisionRepository {
    SqlitePlanRevisionRepository::open(db, STAMP).unwrap()
}

fn wait_for(path: &Path) {
    let started = Instant::now();
    while !path.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(18),
            "child never reached actual production transaction boundary: {path:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

fn selected_stage(mode: &str) -> V2FaultStage {
    match mode {
        "semantic-before" => V2FaultStage::SemanticBeforeCommit,
        "semantic-after" => V2FaultStage::SemanticAfterCommitBeforeReply,
        "layout-before" => V2FaultStage::LayoutBeforeCommit,
        "layout-after" => V2FaultStage::LayoutAfterCommitBeforeReply,
        _ => panic!("unknown crash fixture stage"),
    }
}

/// Child entrypoint invoked by the actual Rust integration test executable.
/// The real repository method owns the SQL transaction at the fault hook.
#[test]
fn crash_worker() {
    let Ok(db) = env::var("RIESEL_V2_CRASH_DB") else {
        return;
    };
    let db = PathBuf::from(db);
    let gate = PathBuf::from(env::var("RIESEL_V2_CRASH_GATE").unwrap());
    let mode = env::var("RIESEL_V2_CRASH_MODE").unwrap();
    let desired = selected_stage(&mode);
    let gate_copy = gate.clone();
    let store = repo(&db).with_test_hook(move |stage| {
        if stage == desired {
            File::create(gate_copy.join("at-boundary")).unwrap();
            wait_for(&gate_copy.join("release"));
        }
    });
    let head = store.head(PLAN).unwrap();
    if mode.starts_with("semantic") {
        store.append(&head, &candidate(2)).unwrap();
    } else {
        store.save_layout(&head, None, &layout()).unwrap();
    }
    panic!("parent must kill this child before the test fault hook returns");
}

fn kill_at_boundary(mode: &str) -> (PathBuf, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let db = dir.path().join("plans.sqlite");
    let gate = dir.path().join("barrier");
    fs::create_dir(&gate).unwrap();
    let store = repo(&db);
    store
        .initialize(&candidate(1), PlanAuthoringSource::Rust, STAMP)
        .unwrap();
    drop(store);

    let mut child = Command::new(env::current_exe().unwrap())
        .arg("--exact")
        .arg("crash_worker")
        .arg("--nocapture")
        .env("RIESEL_V2_CRASH_DB", &db)
        .env("RIESEL_V2_CRASH_GATE", &gate)
        .env("RIESEL_V2_CRASH_MODE", mode)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    wait_for(&gate.join("at-boundary"));
    child.kill().unwrap();
    let outcome = child.wait_with_output().unwrap();
    assert!(!outcome.status.success(), "crash child unexpectedly exited cleanly");
    (db, dir)
}

#[test]
fn real_semantic_writer_killed_precommit_rolls_back_without_partial_revision() {
    let (db, _dir) = kill_at_boundary("semantic-before");
    let store = repo(&db);
    assert_eq!(store.head(PLAN).unwrap().semantic_revision, 1);
    assert!(matches!(
        store.semantic(PLAN, 2),
        Err(PlanRevisionError::NotFound)
    ));
    let head = store.head(PLAN).unwrap();
    assert_eq!(store.append(&head, &candidate(2)).unwrap().semantic_revision, 2);
    assert_eq!(store.semantic(PLAN, 1).unwrap().revision, 1);
    assert_eq!(repo(&db).semantic(PLAN, 2).unwrap().revision, 2);
}

#[test]
fn real_semantic_writer_killed_after_commit_has_recoverable_committed_history() {
    let (db, _dir) = kill_at_boundary("semantic-after");
    let store = repo(&db);
    let head = store.head(PLAN).unwrap();
    assert_eq!(head.semantic_revision, 2);
    assert_eq!(store.semantic(PLAN, 1).unwrap().revision, 1);
    assert_eq!(store.semantic(PLAN, 2).unwrap().revision, 2);
    assert!(matches!(
        store.append(
            &riesel_app::PlanAuthoringHead {
                semantic_revision: 1,
                ..head.clone()
            },
            &candidate(2)
        ),
        Err(PlanRevisionError::Conflict)
    ));
}

#[test]
fn real_layout_writer_killed_precommit_rolls_back_without_partial_layout() {
    let (db, _dir) = kill_at_boundary("layout-before");
    let store = repo(&db);
    let head = store.head(PLAN).unwrap();
    assert_eq!(head.semantic_revision, 1);
    assert!(matches!(
        store.layout(PLAN, "desktop", 1),
        Err(PlanRevisionError::NotFound)
    ));
    let saved = store.save_layout(&head, None, &layout()).unwrap();
    assert_eq!(saved.version, 1);
    assert_eq!(repo(&db).layout(PLAN, "desktop", 1).unwrap(), saved);
}

#[test]
fn real_layout_writer_killed_after_commit_is_recovered_without_semantic_mutation() {
    let (db, _dir) = kill_at_boundary("layout-after");
    let store = repo(&db);
    let head = store.head(PLAN).unwrap();
    assert_eq!(head.semantic_revision, 1);
    let first = store.layout(PLAN, "desktop", 1).unwrap();
    assert_eq!(first.version, 1);
    assert!(matches!(
        store.save_layout(&head, None, &layout()),
        Err(PlanRevisionError::Conflict)
    ));
    let second = store.save_layout(&head, Some(1), &layout()).unwrap();
    assert_eq!(second.version, 2);
    assert_eq!(repo(&db).layout(PLAN, "desktop", 1).unwrap(), first);
    assert_eq!(store.head(PLAN).unwrap().semantic_revision, 1);
}
