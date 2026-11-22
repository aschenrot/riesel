//! #193 explicit, verified offline physical-format evolution.
//! Tests exercise real persisted validated plans and independent V2 backups;
//! there is no implicit schema upgrade on repository open.
use riesel_app::{
    AccessMode, CheckedSourceHandoff, EditorOperationDescriptor, ExecutionPlanDraft, OperationKey,
    OperationNode, OperationRegistry, PlanAuthoringHead, PlanAuthoringSource, PlanLayoutDraft,
    PlanLayoutPoint, PlanRevisionError, PlanRevisionRepository, PlanSemanticCandidate,
    S2DocumentAccess, S2DocumentAccessMode, S2DocumentLimits, S2OperationDocument, S2PlanDocument,
};
use riesel_storage_sqlite::{SqlitePlanRevisionRepository, V2SnapshotBudget};
use rusqlite::Connection;
use std::{collections::BTreeMap, path::Path, sync::Arc};
use tempfile::tempdir;

const PLAN: &str = "upgrade.plan";
const STAMP: &str = "old-trusted-app-build";
type Handler = dyn Fn() + Send + Sync;

fn key() -> OperationKey {
    OperationKey::new("upgrade.operation", 1).unwrap()
}

fn rust_plan(id: &str, revision: u64) -> riesel_app::ValidatedExecutionPlan {
    ExecutionPlanDraft::new(id, revision)
        .operation(OperationNode::new("work", key()).access("state", AccessMode::Write))
        .validate()
        .unwrap()
}

fn registry() -> OperationRegistry<Handler> {
    let mut handlers = OperationRegistry::<Handler>::new();
    handlers
        .register_editor_operation(
            EditorOperationDescriptor::new(key()).require_access("state", AccessMode::Write),
            Arc::new(|| {}),
        )
        .unwrap();
    handlers
}

fn document(id: &str, revision: u64) -> S2PlanDocument {
    let mut node = S2OperationDocument::new("work", &key(), 1);
    node.resources.push(S2DocumentAccess {
        resource: "state".into(),
        mode: S2DocumentAccessMode::Write,
    });
    S2PlanDocument::new(id, revision).operation(node)
}

fn repo(path: &Path) -> SqlitePlanRevisionRepository {
    SqlitePlanRevisionRepository::open(path, STAMP).unwrap()
}

/// Construct real format-1 fixture from current product SQLite tables while
/// preserving actual pre-migration semantic/layout/document data.
fn legacy_fixture(path: &Path) -> (PlanAuthoringHead, PlanAuthoringHead) {
    let store = repo(path);
    let head = store
        .initialize(
            &PlanSemanticCandidate::from_rust(&rust_plan(PLAN, 1)).unwrap(),
            PlanAuthoringSource::Rust,
            STAMP,
        )
        .unwrap();
    let other = store
        .initialize(
            &PlanSemanticCandidate::from_rust(&rust_plan("upgrade.secondary", 1)).unwrap(),
            PlanAuthoringSource::Rust,
            STAMP,
        )
        .unwrap();
    let handlers = registry();
    let proof = CheckedSourceHandoff::rust_to_document(
        &rust_plan(PLAN, 1),
        &document(PLAN, 1),
        &handlers,
        S2DocumentLimits::default(),
    )
    .unwrap();
    let owned = store.handoff(&head, &proof).unwrap();
    let candidate =
        PlanSemanticCandidate::from_s2(&document(PLAN, 2), &handlers, S2DocumentLimits::default())
            .unwrap();
    let newer = store.append(&owned, &candidate).unwrap();
    store
        .save_layout(
            &newer,
            None,
            &PlanLayoutDraft {
                layout_id: "desktop".into(),
                semantic_anchor: 2,
                node_positions: BTreeMap::from([("work".into(), PlanLayoutPoint { x: 20, y: 40 })]),
            },
        )
        .unwrap();
    drop(store);
    let connection = Connection::open(path).unwrap();
    connection
        .execute_batch("DROP TABLE plan_migrations; PRAGMA user_version=1;")
        .unwrap();
    drop(connection);
    (newer, other)
}

fn format_version(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap()
}

#[test]
fn explicit_backup_verified_upgrade_preserves_all_plans_and_fences_old_generations() {
    let temp = tempdir().unwrap();
    let db = temp.path().join("legacy.sqlite");
    let backup = temp.path().join("pre-upgrade.sqlite");
    let after_backup = temp.path().join("post-upgrade.sqlite");
    let (expected, other) = legacy_fixture(&db);
    assert_eq!(format_version(&db), 1);
    let store = repo(&db);
    assert_eq!(store.current_authoring(PLAN).unwrap().head, expected);
    let budget = V2SnapshotBudget::default();
    let snapshot = store.backup_to(&backup, budget).unwrap();
    assert_eq!(snapshot.format_version, 1);
    assert_eq!(snapshot.plan_count, 2);
    let upgraded = store.migrate_v1_to_v2(&expected, &backup, budget).unwrap();
    assert_eq!(format_version(&db), 2);
    assert_eq!(upgraded.semantic_revision, 2);
    assert_eq!(upgraded.source, PlanAuthoringSource::Document);
    assert_eq!(upgraded.source_generation, expected.source_generation + 1);
    assert_eq!(
        store.head("upgrade.secondary").unwrap().source_generation,
        other.source_generation + 1
    );
    assert_eq!(store.semantic(PLAN, 1).unwrap().revision, 1);
    assert_eq!(store.semantic(PLAN, 2).unwrap().revision, 2);
    assert_eq!(
        store
            .layout(PLAN, "desktop", 1)
            .unwrap()
            .layout
            .semantic_anchor,
        2
    );
    let source = store
        .current_authoring(PLAN)
        .unwrap()
        .document_bytes
        .unwrap();
    let doc: S2PlanDocument = serde_json::from_slice(&source).unwrap();
    assert_eq!(doc.semantic_revision, 2);
    let old_registry = registry();
    let stale = PlanSemanticCandidate::from_s2(
        &document(PLAN, 3),
        &old_registry,
        S2DocumentLimits::default(),
    )
    .unwrap();
    assert!(matches!(
        store.append(&expected, &stale),
        Err(PlanRevisionError::Conflict)
    ));
    assert!(matches!(
        store.migrate_v1_to_v2(&expected, &backup, budget),
        Err(PlanRevisionError::UnsupportedFormat(2))
    ));
    let events: i64 = Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM plan_migrations WHERE kind='format'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(events, 2);
    let new_backup = store.backup_to(&after_backup, budget).unwrap();
    assert_eq!(new_backup.format_version, 2);
    assert_eq!(new_backup.plan_count, 2);
    assert_eq!(
        SqlitePlanRevisionRepository::verify_backup(&after_backup, STAMP, budget,)
            .unwrap()
            .format_version,
        2
    );
    // Original v1 snapshot remains readable and unmodified after migration.
    assert_eq!(
        SqlitePlanRevisionRepository::verify_backup(&backup, STAMP, budget,)
            .unwrap()
            .format_version,
        1
    );
}

#[test]
fn stale_legacy_backup_or_changed_predecessor_rejects_without_partial_upgrade() {
    let temp = tempdir().unwrap();
    let db = temp.path().join("legacy.sqlite");
    let backup = temp.path().join("stale.sqlite");
    let (expected, _) = legacy_fixture(&db);
    let store = repo(&db);
    let budget = V2SnapshotBudget::default();
    store.backup_to(&backup, budget).unwrap();
    let handlers = registry();
    let next =
        PlanSemanticCandidate::from_s2(&document(PLAN, 3), &handlers, S2DocumentLimits::default())
            .unwrap();
    let head = store.append(&expected, &next).unwrap();
    assert_eq!(head.semantic_revision, 3);
    assert!(store.migrate_v1_to_v2(&expected, &backup, budget).is_err());
    assert_eq!(format_version(&db), 1);
    assert!(matches!(
        store.migrate_v1_to_v2(&head, &backup, budget),
        Err(PlanRevisionError::Conflict)
    ));
    assert_eq!(format_version(&db), 1);
    let missing_table: i64 = Connection::open(&db)
        .unwrap()
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='plan_migrations'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(missing_table, 0);
    assert_eq!(store.head(PLAN).unwrap(), head);
}

#[test]
fn migration_refuses_foreign_or_corrupt_backup_without_touching_legacy_store() {
    let temp = tempdir().unwrap();
    let db = temp.path().join("legacy.sqlite");
    let backup = temp.path().join("backup.sqlite");
    let (head, _) = legacy_fixture(&db);
    let store = repo(&db);
    let budget = V2SnapshotBudget::default();
    store.backup_to(&backup, budget).unwrap();
    let raw = Connection::open(&backup).unwrap();
    raw.execute_batch("PRAGMA user_version=83").unwrap();
    drop(raw);
    assert!(matches!(
        store.migrate_v1_to_v2(&head, &backup, budget),
        Err(PlanRevisionError::UnsupportedFormat(83))
    ));
    assert_eq!(format_version(&db), 1);
    assert_eq!(store.head(PLAN).unwrap(), head);
    assert_eq!(store.semantic(PLAN, 2).unwrap().revision, 2);
}
