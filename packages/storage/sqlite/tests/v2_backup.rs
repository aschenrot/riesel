//! #193 trusted-local V2 snapshot recovery, checked source provenance,
//! deliberate new-file restore and fail-closed compatibility. The snapshot
//! is NOT a signed publisher or actual application runtime activation.
use riesel_app::{
    AccessMode, CheckedSourceHandoff, EditorOperationDescriptor, ExecutionPlanDraft,
    OperationKey, OperationNode, OperationRegistry, PlanAuthoringSource, PlanLayoutDraft,
    PlanLayoutPoint, PlanRevisionError, PlanRevisionRepository, PlanSemanticCandidate,
    S2DocumentAccess, S2DocumentAccessMode, S2DocumentLimits, S2OperationDocument,
    S2PlanDocument,
};
use riesel_storage_sqlite::{
    SqlitePlanRevisionRepository, V2SnapshotBudget,
};
use rusqlite::Connection;
use std::{collections::BTreeMap, fs, path::Path, sync::Arc};
use tempfile::tempdir;

const PLAN: &str = "backup.pipeline";
const STAMP: &str = "trusted-app-build.2026";
type Handler = dyn Fn() + Send + Sync;

fn key() -> OperationKey {
    OperationKey::new("backup.operation", 1).unwrap()
}

fn validated(revision: u64) -> riesel_app::ValidatedExecutionPlan {
    ExecutionPlanDraft::new(PLAN, revision)
        .operation(
            OperationNode::new("execute", key()).access("state", AccessMode::Write),
        )
        .validate()
        .unwrap()
}

fn code(revision: u64) -> PlanSemanticCandidate {
    PlanSemanticCandidate::from_rust(&validated(revision)).unwrap()
}

fn registry() -> OperationRegistry<Handler> {
    let mut handlers = OperationRegistry::<Handler>::new();
    handlers
        .register_editor_operation(
            EditorOperationDescriptor::new(key())
                .require_access("state", AccessMode::Write),
            Arc::new(|| {}),
        )
        .unwrap();
    handlers
}

fn document(revision: u64) -> S2PlanDocument {
    let mut node = S2OperationDocument::new("execute", &key(), 1);
    node.resources.push(S2DocumentAccess {
        resource: "state".into(),
        mode: S2DocumentAccessMode::Write,
    });
    S2PlanDocument::new(PLAN, revision).operation(node)
}

fn layout(revision: u64) -> PlanLayoutDraft {
    PlanLayoutDraft {
        layout_id: "desktop".into(),
        semantic_anchor: revision,
        node_positions: BTreeMap::from([(
            "execute".into(),
            PlanLayoutPoint { x: 6, y: 12 },
        )]),
    }
}

fn repo(path: &Path) -> SqlitePlanRevisionRepository {
    SqlitePlanRevisionRepository::open(path, STAMP).unwrap()
}

#[test]
fn verified_snapshot_restores_immutable_semantics_layouts_and_document_ownership() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.sqlite");
    let snapshot = temp.path().join("backup.sqlite");
    let recovered = temp.path().join("restored.sqlite");
    let store = repo(&source);
    let head = store.initialize(&code(1), PlanAuthoringSource::Rust, STAMP).unwrap();
    let handlers = registry();
    let proof = CheckedSourceHandoff::rust_to_document(
        &validated(1), &document(1), &handlers, S2DocumentLimits::default(),
    ).unwrap();
    let document_head = store.handoff(&head, &proof).unwrap();
    let candidate = PlanSemanticCandidate::from_s2(
        &document(2), &handlers, S2DocumentLimits::default(),
    ).unwrap();
    let new_head = store.append(&document_head, &candidate).unwrap();
    store.save_layout(&new_head, None, &layout(2)).unwrap();

    let budget = V2SnapshotBudget::default();
    let verified = store.backup_to(&snapshot, budget).unwrap();
    assert_eq!(verified.path, snapshot);
    assert_eq!(verified.plan_count, 1);
    assert_eq!(verified.semantic_count, 2);
    assert_eq!(verified.layout_count, 1);
    assert!(verified.snapshot_bytes > 0);
    assert_eq!(verified.registration_stamp, STAMP);
    assert_eq!(
        SqlitePlanRevisionRepository::verify_backup(&snapshot, STAMP, budget).unwrap(),
        verified
    );

    // A later change to source must not mutate the independently published snapshot.
    let next = PlanSemanticCandidate::from_s2(
        &document(3), &handlers, S2DocumentLimits::default(),
    ).unwrap();
    store.append(&new_head, &next).unwrap();
    assert_eq!(store.head(PLAN).unwrap().semantic_revision, 3);
    drop(store);

    let restored = SqlitePlanRevisionRepository::restore_backup_to_new_file(
        &snapshot, &recovered, STAMP, budget,
    ).unwrap();
    assert_eq!(restored.plan_count, 1);
    assert_eq!(restored.semantic_count, 2);
    let reopened = repo(&recovered);
    let head = reopened.head(PLAN).unwrap();
    assert_eq!(head.semantic_revision, 2);
    assert_eq!(head.source, PlanAuthoringSource::Document);
    assert_eq!(head.source_generation, 2);
    assert_eq!(reopened.semantic(PLAN, 1).unwrap().revision, 1);
    assert_eq!(reopened.semantic(PLAN, 2).unwrap().revision, 2);
    assert_eq!(reopened.layout(PLAN, "desktop", 1).unwrap().layout.semantic_anchor, 2);
    let bytes = reopened.current_authoring(PLAN).unwrap().document_bytes.unwrap();
    let source_doc: S2PlanDocument = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(source_doc.semantic_revision, 2);
    assert!(matches!(reopened.semantic(PLAN, 3), Err(PlanRevisionError::NotFound)));
}

#[test]
fn backup_and_offline_restore_never_overwrite_existing_user_files() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.sqlite");
    let destination = temp.path().join("exists.sqlite");
    let backup = temp.path().join("backup.sqlite");
    let store = repo(&source);
    store.initialize(&code(1), PlanAuthoringSource::Rust, STAMP).unwrap();
    fs::write(&destination, b"preserve preexisting file").unwrap();
    assert!(store.backup_to(&destination, V2SnapshotBudget::default()).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"preserve preexisting file");
    store.backup_to(&backup, V2SnapshotBudget::default()).unwrap();
    assert!(SqlitePlanRevisionRepository::restore_backup_to_new_file(
        &backup, &destination, STAMP, V2SnapshotBudget::default(),
    ).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"preserve preexisting file");
}

#[test]
fn backup_rejects_unapproved_new_build_and_unknown_disk_format_without_copying() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.sqlite");
    let snapshot = temp.path().join("backup.sqlite");
    let destination = temp.path().join("restored.sqlite");
    let store = repo(&source);
    let head = store.initialize(&code(1), PlanAuthoringSource::Rust, STAMP).unwrap();
    store.backup_to(&snapshot, V2SnapshotBudget::default()).unwrap();
    assert!(matches!(
        SqlitePlanRevisionRepository::verify_backup(
            &snapshot, "unverified-new-artifact", V2SnapshotBudget::default()
        ),
        Err(PlanRevisionError::Conflict)
    ));
    assert!(SqlitePlanRevisionRepository::restore_backup_to_new_file(
        &snapshot, &destination, "unverified-new-artifact", V2SnapshotBudget::default(),
    ).is_err());
    assert!(!destination.exists());

    // The old app-owned state can be inspected by a different registered
    // application, but cannot be mutated merely by copying a build label.
    let new_application =
        SqlitePlanRevisionRepository::open(&source, "unverified-new-artifact").unwrap();
    assert_eq!(new_application.head(PLAN).unwrap(), head);
    assert!(matches!(
        new_application.append(&head, &code(2)),
        Err(PlanRevisionError::Conflict)
    ));
    drop(new_application);

    let raw = Connection::open(&snapshot).unwrap();
    raw.execute_batch("PRAGMA user_version=83").unwrap();
    drop(raw);
    assert!(matches!(
        SqlitePlanRevisionRepository::verify_backup(
            &snapshot, STAMP, V2SnapshotBudget::default()
        ),
        Err(PlanRevisionError::UnsupportedFormat(83))
    ));
    assert!(!destination.exists());
}

#[test]
fn corrupted_backup_and_bounded_snapshot_are_rejected_without_publishing() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("source.sqlite");
    let backup = temp.path().join("backup.sqlite");
    let raw = temp.path().join("corrupted.sqlite");
    let dest = temp.path().join("restore.sqlite");
    let store = repo(&source);
    store.initialize(&code(1), PlanAuthoringSource::Rust, STAMP).unwrap();
    fs::write(&raw, b"not a SQLite database").unwrap();
    assert!(SqlitePlanRevisionRepository::verify_backup(
        &raw, STAMP, V2SnapshotBudget::default()
    ).is_err());
    assert!(SqlitePlanRevisionRepository::restore_backup_to_new_file(
        &raw, &dest, STAMP, V2SnapshotBudget::default()
    ).is_err());
    assert!(!dest.exists());
    let restricted = V2SnapshotBudget {
        max_snapshot_bytes: 8,
        max_rows: 100,
    };
    assert!(store.backup_to(&backup, restricted).is_err());
    assert!(!backup.exists());
    let limited_rows = V2SnapshotBudget {
        max_snapshot_bytes: 1024 * 1024,
        max_rows: 1,
    };
    assert!(store.backup_to(&backup, limited_rows).is_err());
    assert!(!backup.exists());
}
