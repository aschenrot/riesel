//! #193: report exact unsupported Rust-to-S2 authoring constructs before a
//! trusted source owner deliberately transfers authorship. No handlers execute.
use riesel_app::{
    AccessMode, CheckedSourceHandoff, EditorOperationDescriptor, ExecutionPlanDraft, OperationKey,
    OperationNode, OperationRegistry, PlanRevisionError, PlanSourceCoverageReport,
    S2DocumentAccess, S2DocumentAccessMode, S2DocumentLimits, S2DocumentTrigger,
    S2OperationDocument, S2PlanDocument, SourceCoverageProblem,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

type Handler = dyn Fn() + Send + Sync;

fn key(name: &str) -> OperationKey {
    OperationKey::new(name, 1).unwrap()
}

fn registered(hits: Arc<AtomicUsize>) -> OperationRegistry<Handler> {
    let mut handlers = OperationRegistry::<Handler>::new();
    handlers
        .register_editor_operation(
            EditorOperationDescriptor::new(key("app.update"))
                .with_descriptor_version(2)
                .require_access("state", AccessMode::Write)
                .require_signal("ready"),
            Arc::new(move || {
                hits.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
    handlers
}

fn code_plan() -> riesel_app::ValidatedExecutionPlan {
    ExecutionPlanDraft::new("app.plan", 1)
        .operation(
            OperationNode::new("update", key("app.update"))
                .access("state", AccessMode::Write)
                .on_signal("ready"),
        )
        .validate()
        .unwrap()
}

fn document() -> S2PlanDocument {
    let mut update = S2OperationDocument::new("update", &key("app.update"), 2);
    update.trigger = S2DocumentTrigger::Signal("ready".into());
    update.resources.push(S2DocumentAccess {
        resource: "state".into(),
        mode: S2DocumentAccessMode::Write,
    });
    S2PlanDocument::new("app.plan", 1).operation(update)
}

fn analyze(
    doc: &S2PlanDocument,
    registry: &OperationRegistry<Handler>,
) -> PlanSourceCoverageReport {
    PlanSourceCoverageReport::compare_rust_to_s2(
        &code_plan(),
        doc,
        registry,
        S2DocumentLimits::default(),
    )
}

fn has_problem(report: &PlanSourceCoverageReport, problem: SourceCoverageProblem) -> bool {
    report
        .findings
        .iter()
        .any(|finding| finding.problem == problem)
}

#[test]
fn fully_representable_rust_source_yields_complete_passive_report() {
    let hits = Arc::new(AtomicUsize::new(0));
    let handlers = registered(Arc::clone(&hits));
    let report = analyze(&document(), &handlers);
    assert!(report.is_complete(), "{report:?}");
    assert_eq!(report.plan_id, "app.plan");
    assert_eq!(report.semantic_revision, 1);
    let proof = CheckedSourceHandoff::rust_to_document(
        &code_plan(),
        &document(),
        &handlers,
        S2DocumentLimits::default(),
    )
    .unwrap();
    assert_eq!(proof.plan_id(), "app.plan");
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn changed_compulsory_resource_and_signal_are_explained_before_handoff() {
    let handlers = registered(Arc::new(AtomicUsize::new(0)));
    let mut changed = document();
    changed.operations[0].resources[0].mode = S2DocumentAccessMode::Read;
    changed.operations[0].trigger = S2DocumentTrigger::Immediate;
    let report = analyze(&changed, &handlers);
    assert!(has_problem(
        &report,
        SourceCoverageProblem::ResourceAccessChanged
    ));
    assert!(has_problem(&report, SourceCoverageProblem::TriggerChanged));
    assert!(
        report
            .findings
            .iter()
            .any(|item| matches!(item.problem, SourceCoverageProblem::RejectedByPreflight(_)))
    );
    assert!(matches!(
        CheckedSourceHandoff::rust_to_document(
            &code_plan(),
            &changed,
            &handlers,
            S2DocumentLimits::default()
        ),
        Err(PlanRevisionError::Invalid(_))
    ));
}

#[test]
fn missing_extra_and_mismatched_exact_binding_nodes_are_reported() {
    let handlers = registered(Arc::new(AtomicUsize::new(0)));
    let missing = S2PlanDocument::new("app.plan", 1);
    assert!(has_problem(
        &analyze(&missing, &handlers),
        SourceCoverageProblem::MissingNode
    ));
    let mut added = document();
    added.operations.push(S2OperationDocument::new(
        "unexpected",
        &key("app.update"),
        2,
    ));
    assert!(has_problem(
        &analyze(&added, &handlers),
        SourceCoverageProblem::ExtraNode
    ));
    let mut mismatch = document();
    mismatch.operations[0].binding.version = 3;
    assert!(has_problem(
        &analyze(&mismatch, &handlers),
        SourceCoverageProblem::BindingMismatch
    ));
    let mut stale = document();
    stale.operations[0].binding.descriptor_version = 1;
    assert!(has_problem(
        &analyze(&stale, &handlers),
        SourceCoverageProblem::DescriptorVersionMismatch
    ));
}

#[test]
fn non_editor_authorized_rust_handlers_cannot_be_silently_exported() {
    let handlers = registered(Arc::new(AtomicUsize::new(0)));
    let unsupported = ExecutionPlanDraft::new("app.plan", 1)
        .operation(OperationNode::new("private", key("app.private")))
        .validate()
        .unwrap();
    let source = S2PlanDocument::new("app.plan", 1).operation(S2OperationDocument::new(
        "private",
        &key("app.private"),
        1,
    ));
    let report = PlanSourceCoverageReport::compare_rust_to_s2(
        &unsupported,
        &source,
        &handlers,
        S2DocumentLimits::default(),
    );
    assert!(has_problem(
        &report,
        SourceCoverageProblem::MissingEditorDescriptor
    ));
    assert!(
        CheckedSourceHandoff::rust_to_document(
            &unsupported,
            &source,
            &handlers,
            S2DocumentLimits::default()
        )
        .is_err()
    );
}

#[test]
fn plan_revision_and_identity_changes_are_not_source_transfers() {
    let handlers = registered(Arc::new(AtomicUsize::new(0)));
    let mut changed = document();
    changed.plan_id = "different".into();
    changed.semantic_revision = 2;
    let report = analyze(&changed, &handlers);
    assert!(has_problem(
        &report,
        SourceCoverageProblem::PlanIdentityMismatch
    ));
    assert!(has_problem(
        &report,
        SourceCoverageProblem::SemanticRevisionMismatch
    ));
    assert!(!report.is_complete());
}
