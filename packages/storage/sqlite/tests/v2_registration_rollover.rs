//! #193: offline installed-build rollover binds *real* replacement callbacks
//! against the exact stored plans before changing any durable owner stamp.
use riesel_app::{
    AccessMode, EditorOperationDescriptor, ExecutionPlanDraft, OperationKey, OperationNode,
    OperationRegistry, PlanAuthoringHead, PlanAuthoringSource, PlanLayoutDraft, PlanLayoutPoint,
    PlanRevisionError, PlanRevisionRepository, PlanSemanticCandidate, S2DocumentAccess,
    S2DocumentAccessMode, S2DocumentLimits, S2OperationDocument, S2PlanDocument,
    ValidatedExecutionPlan,
};
use riesel_storage_sqlite::{SqlitePlanRevisionRepository, V2SnapshotBudget};
use rusqlite::Connection;
use std::{collections::BTreeMap, path::Path, sync::Arc};
use tempfile::tempdir;

const OLD: &str = "installed-app-v1";
const NEW: &str = "installed-app-v2";
type Handler = dyn Fn() + Send + Sync;

fn key() -> OperationKey {
    OperationKey::new("upgrade.bound_callback", 1).unwrap()
}

fn handlers(descriptor_version: u32) -> OperationRegistry<Handler> {
    let mut registry = OperationRegistry::<Handler>::new();
    registry.register_editor_operation(
        EditorOperationDescriptor::new(key())
            .with_descriptor_version(descriptor_version)
            .require_access("state", AccessMode::Write),
        Arc::new(|| {}),
    ).unwrap();
    registry
}

fn rust_plan(revision: u64, mode: AccessMode) -> ValidatedExecutionPlan {
    ExecutionPlanDraft::new("rollover.code", revision)
        .operation(OperationNode::new("work", key()).access("state", mode))
        .validate().unwrap()
}

fn document(revision: u64) -> S2PlanDocument {
    let mut node = S2OperationDocument::new("work", &key(), 1);
    node.resources.push(S2DocumentAccess {
        resource: "state".into(),
        mode: S2DocumentAccessMode::Write,
    });
    S2PlanDocument::new("rollover.document", revision).operation(node)
}

fn setup(path: &Path) -> (PlanAuthoringHead, PlanAuthoringHead) {
    let old = SqlitePlanRevisionRepository::open(path, OLD).unwrap();
    let code = rust_plan(1, AccessMode::Write);
    let code_head = old.initialize(
        &PlanSemanticCandidate::from_rust(&code).unwrap(),
        PlanAuthoringSource::Rust, OLD,
    ).unwrap();
    let doc_head = old.initialize(
        &PlanSemanticCandidate::from_s2(
            &document(1), &handlers(1), S2DocumentLimits::default(),
        ).unwrap(),
        PlanAuthoringSource::Document, OLD,
    ).unwrap();
    old.save_layout(
        &doc_head,
        None,
        &PlanLayoutDraft {
            layout_id: "desktop".into(),
            semantic_anchor: 1,
            node_positions: BTreeMap::from([
                ("work".into(), PlanLayoutPoint { x: 50, y: -10 }),
            ]),
        },
    ).unwrap();
    (code_head, doc_head)
}

fn backup(old: &SqlitePlanRevisionRepository, path: &Path) {
    old.backup_to(path, V2SnapshotBudget::default()).unwrap();
}

fn adopt(
    new: &SqlitePlanRevisionRepository,
    heads: &[PlanAuthoringHead],
    backup_path: &Path,
    registry: &OperationRegistry<Handler>,
    rust: &[ValidatedExecutionPlan],
) -> Result<Vec<PlanAuthoringHead>, PlanRevisionError> {
    new.adopt_registered_build(
        OLD, heads, backup_path, V2SnapshotBudget::default(),
        registry, rust, S2DocumentLimits::default(),
    )
}

#[test]
fn two_plan_build_rollover_preserves_history_and_fences_all_previous_writers() {
    let temp = tempdir().unwrap();
    let db = temp.path().join("plans.sqlite");
    let snapshot = temp.path().join("previous.sqlite");
    let (code, doc) = setup(&db);
    let old = SqlitePlanRevisionRepository::open(&db, OLD).unwrap();
    backup(&old, &snapshot);
    let new = SqlitePlanRevisionRepository::open(&db, NEW).unwrap();
    let heads = adopt(
        &new, &[code.clone(), doc.clone()], &snapshot, &handlers(1),
        &[rust_plan(1, AccessMode::Write)],
    ).unwrap();
    assert_eq!(heads.len(), 2);
    for current in &heads {
        let prior = if current.plan_id == code.plan_id { &code } else { &doc };
        assert_eq!(current.semantic_revision, prior.semantic_revision);
        assert_eq!(current.source_generation, prior.source_generation + 1);
        assert_eq!(current.source, prior.source);
        assert_eq!(current.registration_stamp, NEW);
        assert_eq!(
            new.semantic(&current.plan_id, current.semantic_revision).unwrap(),
            old.semantic(&current.plan_id, current.semantic_revision).unwrap(),
        );
    }
    let reopened = SqlitePlanRevisionRepository::open(&db, NEW).unwrap();
    assert_eq!(reopened.current_authoring(&doc.plan_id).unwrap()
        .document_bytes, old.current_authoring(&doc.plan_id).unwrap().document_bytes);
    assert_eq!(reopened.layout(&doc.plan_id, "desktop", 1).unwrap().layout.semantic_anchor, 1);
    let old_candidate = PlanSemanticCandidate::from_rust(
        &rust_plan(2, AccessMode::Write)
    ).unwrap();
    assert!(matches!(old.append(&code, &old_candidate), Err(PlanRevisionError::Conflict)));
    assert!(matches!(old.save_layout(
        &doc, Some(1),
        &PlanLayoutDraft {
            layout_id: "desktop".into(), semantic_anchor: 1,
            node_positions: BTreeMap::new(),
        },
    ), Err(PlanRevisionError::Conflict)));
    let migration_count: i64 = Connection::open(&db).unwrap().query_row(
        "SELECT count(*) FROM plan_migrations WHERE kind='registration'
         AND previous_registration=?1 AND next_registration=?2",
        [OLD, NEW], |row| row.get(0),
    ).unwrap();
    assert_eq!(migration_count, 2);
    // A same-stamp second rollover or a stale backup cannot be replayed.
    assert!(adopt(
        &new, &[code, doc], &snapshot, &handlers(1),
        &[rust_plan(1, AccessMode::Write)],
    ).is_err());
}

#[test]
fn changed_descriptor_or_callback_key_rejects_before_any_owner_change() {
    let temp = tempdir().unwrap();
    let db = temp.path().join("plans.sqlite");
    let snapshot = temp.path().join("previous.sqlite");
    let (code, doc) = setup(&db);
    let old = SqlitePlanRevisionRepository::open(&db, OLD).unwrap();
    backup(&old, &snapshot);
    let new = SqlitePlanRevisionRepository::open(&db, NEW).unwrap();

    // Rust-only source may bind, but a changed document descriptor version
    // invalidates the exact persisted editor-owned plan.
    assert!(adopt(
        &new, &[code.clone(), doc.clone()], &snapshot, &handlers(2),
        &[rust_plan(1, AccessMode::Write)],
    ).is_err());
    assert_eq!(old.head(&code.plan_id).unwrap(), code);
    assert_eq!(old.head(&doc.plan_id).unwrap(), doc);

    let empty = OperationRegistry::<Handler>::new();
    assert!(adopt(
        &new, &[code.clone(), doc.clone()], &snapshot, &empty,
        &[rust_plan(1, AccessMode::Write)],
    ).is_err());
    assert_eq!(old.head(&code.plan_id).unwrap(), code);
    assert_eq!(old.head(&doc.plan_id).unwrap(), doc);

    // Even with an exact callback key, changed code-owned resource meaning
    // cannot silently become the previous immutable semantic revision.
    assert!(adopt(
        &new, &[code.clone(), doc.clone()], &snapshot, &handlers(1),
        &[rust_plan(1, AccessMode::Read)],
    ).is_err());
    assert_eq!(old.head(&code.plan_id).unwrap(), code);
    assert_eq!(old.head(&doc.plan_id).unwrap(), doc);
}

#[test]
fn changed_layout_or_incomplete_expected_heads_rejects_without_partial_adoption() {
    let temp = tempdir().unwrap();
    let db = temp.path().join("plans.sqlite");
    let snapshot = temp.path().join("previous.sqlite");
    let (code, doc) = setup(&db);
    let old = SqlitePlanRevisionRepository::open(&db, OLD).unwrap();
    backup(&old, &snapshot);
    let new = SqlitePlanRevisionRepository::open(&db, NEW).unwrap();
    assert!(adopt(
        &new, &[code.clone()], &snapshot, &handlers(1),
        &[rust_plan(1, AccessMode::Write)],
    ).is_err());
    old.save_layout(
        &doc, Some(1),
        &PlanLayoutDraft {
            layout_id: "desktop".into(), semantic_anchor: 1,
            node_positions: BTreeMap::new(),
        },
    ).unwrap();
    assert!(adopt(
        &new, &[code.clone(), doc.clone()], &snapshot, &handlers(1),
        &[rust_plan(1, AccessMode::Write)],
    ).is_err());
    assert_eq!(old.head(&code.plan_id).unwrap(), code);
    assert_eq!(old.head(&doc.plan_id).unwrap(), doc);
    let count: i64 = Connection::open(&db).unwrap().query_row(
        "SELECT count(*) FROM plan_migrations WHERE kind='registration'",
        [], |row| row.get(0),
    ).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn legacy_format_cannot_skip_backup_verified_physical_evolution() {
    let temp = tempdir().unwrap();
    let db = temp.path().join("plans.sqlite");
    let snapshot = temp.path().join("previous.sqlite");
    let (code, doc) = setup(&db);
    let raw = Connection::open(&db).unwrap();
    raw.execute_batch("DROP TABLE plan_migrations; PRAGMA user_version=1;").unwrap();
    drop(raw);
    let old = SqlitePlanRevisionRepository::open(&db, OLD).unwrap();
    backup(&old, &snapshot);
    let new = SqlitePlanRevisionRepository::open(&db, NEW).unwrap();
    assert!(matches!(
        adopt(
            &new, &[code.clone(), doc.clone()], &snapshot, &handlers(1),
            &[rust_plan(1, AccessMode::Write)],
        ),
        Err(PlanRevisionError::UnsupportedFormat(1))
    ));
    assert_eq!(old.head(&code.plan_id).unwrap(), code);
    assert_eq!(old.head(&doc.plan_id).unwrap(), doc);
}
