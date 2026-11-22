//! Separate compiled compatible replacement artifact adopts every stored plan.
mod v2_artifact_shared;

use riesel_app::{
    PlanAuthoringSource, PlanLayoutDraft, PlanRevisionError, PlanRevisionRepository,
    S2DocumentLimits,
};
use riesel_storage_sqlite::V2SnapshotBudget;
use std::{collections::BTreeMap, env, path::PathBuf};
use v2_artifact_shared::{CODE_ID, DOCUMENT_ID, NEW, OLD, handlers, old_heads, rust_plan, store};

fn main() {
    let mut args = env::args_os().skip(1);
    let path = PathBuf::from(args.next().expect("store path"));
    let snapshot = PathBuf::from(args.next().expect("backup path"));
    assert!(args.next().is_none());
    let old = store(&path, OLD);
    let new = store(&path, NEW);
    let previous = old_heads(&old);
    let adopted = new
        .adopt_registered_build(
            &previous,
            &snapshot,
            V2SnapshotBudget::default(),
            &handlers(1),
            &[rust_plan()],
            S2DocumentLimits::default(),
        )
        .unwrap();
    assert_eq!(adopted.len(), 2);
    for head in adopted {
        let predecessor = previous.iter().find(|old| old.plan_id == head.plan_id).unwrap();
        assert_eq!(head.registration_stamp, NEW);
        assert_eq!(head.source_generation, predecessor.source_generation + 1);
        assert_eq!(head.semantic_revision, predecessor.semantic_revision);
        assert_eq!(head.source, predecessor.source);
        assert_eq!(
            new.semantic(&head.plan_id, head.semantic_revision).unwrap(),
            old.semantic(&head.plan_id, head.semantic_revision).unwrap(),
        );
    }
    assert_eq!(new.head(CODE_ID).unwrap().source, PlanAuthoringSource::Rust);
    let current = new.current_authoring(DOCUMENT_ID).unwrap();
    assert_eq!(current.head.source, PlanAuthoringSource::Document);
    assert!(current.document_bytes.is_some());
    assert_eq!(new.layout(DOCUMENT_ID, "desktop", 1).unwrap().layout.semantic_anchor, 2);

    // Old compiled artifact's original generation must remain unable to write.
    assert!(matches!(
        old.save_layout(
            &previous[1],
            Some(1),
            &PlanLayoutDraft {
                layout_id: "desktop".into(),
                semantic_anchor: 2,
                node_positions: BTreeMap::new(),
            },
        ),
        Err(PlanRevisionError::Conflict)
    ));
}
