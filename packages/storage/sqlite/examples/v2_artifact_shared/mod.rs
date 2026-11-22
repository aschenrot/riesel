//! Shared contract-only plan fixtures for three independently compiled
//! old, incompatible replacement and compatible replacement application binaries.
//! This is real cross-process *binding* evidence, not artifact authentication.

use riesel_app::{
    AccessMode, EditorOperationDescriptor, ExecutionPlanDraft, OperationKey, OperationNode,
    OperationRegistry, PlanAuthoringHead, PlanRevisionRepository, S2DocumentAccess,
    S2DocumentAccessMode, S2OperationDocument, S2PlanDocument, ValidatedExecutionPlan,
};
use riesel_storage_sqlite::SqlitePlanRevisionRepository;
use std::{path::Path, sync::Arc};

pub const OLD: &str = "fixture.build.old";
pub const NEW: &str = "fixture.build.new";
pub const CODE_ID: &str = "fixture.code";
pub const DOCUMENT_ID: &str = "fixture.document";

pub type Handler = dyn Fn() + Send + Sync;

fn key() -> OperationKey {
    OperationKey::new("fixture.operation", 1).unwrap()
}

pub fn rust_plan() -> ValidatedExecutionPlan {
    ExecutionPlanDraft::new(CODE_ID, 1)
        .operation(OperationNode::new("work", key()).access("state", AccessMode::Write))
        .validate()
        .unwrap()
}

pub fn document(revision: u64) -> S2PlanDocument {
    let mut node = S2OperationDocument::new("work", &key(), 1);
    node.resources.push(S2DocumentAccess {
        resource: "state".into(),
        mode: S2DocumentAccessMode::Write,
    });
    S2PlanDocument::new(DOCUMENT_ID, revision).operation(node)
}

pub fn handlers(version: u32) -> OperationRegistry<Handler> {
    let mut registry = OperationRegistry::<Handler>::new();
    registry
        .register_editor_operation(
            EditorOperationDescriptor::new(key())
                .with_descriptor_version(version)
                .require_access("state", AccessMode::Write),
            Arc::new(|| {}),
        )
        .unwrap();
    registry
}

pub fn store(path: &Path, stamp: &str) -> SqlitePlanRevisionRepository {
    SqlitePlanRevisionRepository::open(path, stamp).unwrap()
}

pub fn old_heads(store: &SqlitePlanRevisionRepository) -> [PlanAuthoringHead; 2] {
    [
        store.head(CODE_ID).unwrap(),
        store.head(DOCUMENT_ID).unwrap(),
    ]
}
