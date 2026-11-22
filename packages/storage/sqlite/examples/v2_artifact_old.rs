//! Separate compiled legacy application artifact creating one durable store.
#[allow(dead_code)] // Shared fixture API is intentionally a subset in each binary.
mod v2_artifact_shared;

use riesel_app::{
    PlanAuthoringSource, PlanLayoutDraft, PlanLayoutPoint, PlanRevisionRepository,
    PlanSemanticCandidate, S2DocumentLimits,
};
use riesel_storage_sqlite::V2SnapshotBudget;
use std::{collections::BTreeMap, env, path::PathBuf};
use v2_artifact_shared::{CODE_ID, DOCUMENT_ID, OLD, document, handlers, rust_plan, store};

fn main() {
    let mut args = env::args_os().skip(1);
    let path = PathBuf::from(args.next().expect("store path"));
    let snapshot = PathBuf::from(args.next().expect("backup path"));
    assert!(args.next().is_none());
    let old = store(&path, OLD);
    let code = PlanSemanticCandidate::from_rust(&rust_plan()).unwrap();
    let code_head = old
        .initialize(&code, PlanAuthoringSource::Rust, OLD)
        .unwrap();
    let doc =
        PlanSemanticCandidate::from_s2(&document(1), &handlers(1), S2DocumentLimits::default())
            .unwrap();
    let initial = old
        .initialize(&doc, PlanAuthoringSource::Document, OLD)
        .unwrap();
    let next =
        PlanSemanticCandidate::from_s2(&document(2), &handlers(1), S2DocumentLimits::default())
            .unwrap();
    let updated = old.append(&initial, &next).unwrap();
    old.save_layout(
        &updated,
        None,
        &PlanLayoutDraft {
            layout_id: "desktop".into(),
            semantic_anchor: 2,
            node_positions: BTreeMap::from([("work".into(), PlanLayoutPoint { x: 18, y: 25 })]),
        },
    )
    .unwrap();
    assert_eq!(code_head.plan_id, CODE_ID);
    assert_eq!(updated.plan_id, DOCUMENT_ID);
    old.backup_to(&snapshot, V2SnapshotBudget::default())
        .unwrap();
}
