//! #192: strict untrusted S2 authoring documents must use the real Riesel
//! graph validator, trusted editor-catalog gate and exact Rust binding.
use riesel_app::{
    AccessMode, DomainAdmissionMode, EditorOperationDescriptor, ExecutionPlanDraft, OperationKey,
    OperationNode, OperationRegistry, S2DocumentAccess, S2DocumentAccessMode, S2DocumentDomain,
    S2DocumentDomainMode, S2DocumentError, S2DocumentLimits, S2DocumentTrigger,
    S2OperationDocument, S2PlanDocument, S2SourceDocument, bind_s2_document, inspect_s2_source,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

type Handler = dyn Fn() + Send + Sync;

fn key(name: &str, version: u32) -> OperationKey {
    OperationKey::new(name, version).unwrap()
}

fn registry(hits: Arc<AtomicUsize>) -> OperationRegistry<Handler> {
    let mut handlers = OperationRegistry::<Handler>::new();
    handlers
        .register_editor_operation(
            EditorOperationDescriptor::new(key("sample.load", 1))
                .with_descriptor_version(4)
                .require_access("data", AccessMode::Write)
                .require_signal("ready"),
            Arc::new(move || {
                hits.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
    handlers
        .register_editor_operation(
            EditorOperationDescriptor::new(key("sample.frame", 2))
                .require_domain("simulation", DomainAdmissionMode::DelegatedFrame),
            Arc::new(|| {}),
        )
        .unwrap();
    handlers
}

fn fixture() -> S2PlanDocument {
    let mut load = S2OperationDocument::new("load", &key("sample.load", 1), 4);
    load.trigger = S2DocumentTrigger::Signal("ready".into());
    load.resources.push(S2DocumentAccess {
        resource: "data".into(),
        mode: S2DocumentAccessMode::Write,
    });
    let mut frame = S2OperationDocument::new("frame", &key("sample.frame", 2), 1);
    frame.dependencies.push("load".into());
    frame.domain = Some(S2DocumentDomain {
        name: "simulation".into(),
        mode: S2DocumentDomainMode::DelegatedFrame,
    });
    S2PlanDocument::new("sample.pipeline", 1)
        .operation(load)
        .operation(frame)
}

fn source() -> Vec<u8> {
    serde_json::to_vec(&fixture()).unwrap()
}

#[test]
fn document_control_plan_matches_actual_rust_plan_and_binds_without_effects() {
    let hits = Arc::new(AtomicUsize::new(0));
    let handlers = registry(Arc::clone(&hits));
    let mut inspected = inspect_s2_source(&source(), S2DocumentLimits::default()).unwrap();
    let bound = match &inspected {
        S2SourceDocument::Editable(doc) => {
            bind_s2_document(doc, &handlers, S2DocumentLimits::default()).unwrap()
        }
        _ => panic!("supported S2 source must be editable"),
    };
    let rust = ExecutionPlanDraft::new("sample.pipeline", 1)
        .operation(
            OperationNode::new("load", key("sample.load", 1))
                .access("data", AccessMode::Write)
                .on_signal("ready"),
        )
        .operation(
            OperationNode::new("frame", key("sample.frame", 2))
                .after("load")
                .delegated_frame("simulation"),
        )
        .validate()
        .unwrap();
    assert_eq!(
        serde_json::to_value(bound.plan().description()).unwrap(),
        serde_json::to_value(rust.description()).unwrap()
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    let checked = inspected
        .serialize_checked(&handlers, S2DocumentLimits::default())
        .unwrap();
    assert!(matches!(
        inspect_s2_source(&checked, S2DocumentLimits::default()).unwrap(),
        S2SourceDocument::Editable(_)
    ));
    inspected.editable_mut().unwrap().semantic_revision = 2;
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn unknown_future_s3_features_are_read_only_and_preserved_byte_for_byte() {
    let mut future: Value = serde_json::from_slice(&source()).unwrap();
    future["required_features"] = json!(["typed_dataflow.v1"]);
    future["data_edges"] = json!([{"from": "load", "to": "frame", "port": "output"}]);
    let raw = format!("\n  {}\n", serde_json::to_string_pretty(&future).unwrap()).into_bytes();
    let mut view = inspect_s2_source(&raw, S2DocumentLimits::default()).unwrap();
    assert_eq!(view.original_unsupported_bytes(), Some(raw.as_slice()));
    assert!(matches!(
        view.editable_mut(),
        Err(S2DocumentError::UnsupportedReadOnly)
    ));
    let handlers = registry(Arc::new(AtomicUsize::new(0)));
    assert!(matches!(
        view.serialize_checked(&handlers, S2DocumentLimits::default()),
        Err(S2DocumentError::UnsupportedReadOnly)
    ));
    assert_eq!(view.original_unsupported_bytes(), Some(raw.as_slice()));

    future["format_version"] = json!(2);
    future["required_features"] = json!([]);
    let bytes = serde_json::to_vec(&future).unwrap();
    assert!(matches!(
        inspect_s2_source(&bytes, S2DocumentLimits::default()).unwrap(),
        S2SourceDocument::UnsupportedReadOnly { .. }
    ));
}

#[test]
fn unknown_executable_fields_and_duplicate_top_level_keys_fail_closed() {
    let mut unknown: Value = serde_json::from_slice(&source()).unwrap();
    unknown["operations"][0]["dynamic_rust_body"] = json!("do unsafe work");
    assert!(matches!(
        inspect_s2_source(
            &serde_json::to_vec(&unknown).unwrap(),
            S2DocumentLimits::default()
        ),
        Err(S2DocumentError::InvalidSource(_))
    ));
    let mut raw = String::from_utf8(source()).unwrap();
    raw = raw.replacen(
        "\"format_version\":1",
        "\"format_version\":1,\"format_version\":1",
        1,
    );
    assert!(matches!(
        inspect_s2_source(raw.as_bytes(), S2DocumentLimits::default()),
        Err(S2DocumentError::InvalidSource(_))
    ));
}

#[test]
fn wrong_descriptor_version_and_unapproved_handlers_fail_closed() {
    let handlers = registry(Arc::new(AtomicUsize::new(0)));
    let mut changed = fixture();
    changed.operations[0].binding.descriptor_version = 3;
    assert!(matches!(
        bind_s2_document(&changed, &handlers, S2DocumentLimits::default()),
        Err(S2DocumentError::DescriptorVersionMismatch {
            expected: 4,
            actual: 3,
            ..
        })
    ));
    changed.operations[0].binding.version = 99;
    assert!(matches!(
        bind_s2_document(&changed, &handlers, S2DocumentLimits::default()),
        Err(S2DocumentError::MissingEditorDescriptor { .. })
    ));
}

#[test]
fn forged_resource_domain_and_trigger_cannot_bypass_real_binding_gate() {
    let hits = Arc::new(AtomicUsize::new(0));
    let handlers = registry(Arc::clone(&hits));
    let mut wrong = fixture();
    wrong.operations[0].resources[0].mode = S2DocumentAccessMode::Read;
    assert!(bind_s2_document(&wrong, &handlers, S2DocumentLimits::default()).is_err());
    let mut wrong = fixture();
    wrong.operations[1].domain.as_mut().unwrap().mode = S2DocumentDomainMode::RieselTick;
    assert!(bind_s2_document(&wrong, &handlers, S2DocumentLimits::default()).is_err());
    let mut wrong = fixture();
    wrong.operations[0].trigger = S2DocumentTrigger::Immediate;
    assert!(bind_s2_document(&wrong, &handlers, S2DocumentLimits::default()).is_err());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn cycles_and_missing_dependencies_fail_in_the_actual_plan_validator() {
    let handlers = registry(Arc::new(AtomicUsize::new(0)));
    let mut cycle = fixture();
    cycle.operations[0].dependencies.push("frame".into());
    assert!(matches!(
        bind_s2_document(&cycle, &handlers, S2DocumentLimits::default()),
        Err(S2DocumentError::InvalidPlan(_))
    ));
    let mut absent = fixture();
    absent.operations[1].dependencies.push("ghost".into());
    assert!(matches!(
        bind_s2_document(&absent, &handlers, S2DocumentLimits::default()),
        Err(S2DocumentError::InvalidPlan(_))
    ));
}

#[test]
fn configurable_input_and_graph_size_budgets_reject_before_binding() {
    let handlers = registry(Arc::new(AtomicUsize::new(0)));
    let mut limits = S2DocumentLimits {
        max_bytes: 10,
        ..S2DocumentLimits::default()
    };
    assert!(matches!(
        inspect_s2_source(&source(), limits),
        Err(S2DocumentError::LimitExceeded(_))
    ));
    limits = S2DocumentLimits {
        max_nodes: 1,
        ..S2DocumentLimits::default()
    };
    assert!(matches!(
        bind_s2_document(&fixture(), &handlers, limits),
        Err(S2DocumentError::LimitExceeded(_))
    ));
    limits.max_nodes = 2;
    limits.max_dependencies = 0;
    assert!(matches!(
        bind_s2_document(&fixture(), &handlers, limits),
        Err(S2DocumentError::LimitExceeded(_))
    ));
    limits = S2DocumentLimits {
        max_resource_entries: 0,
        ..S2DocumentLimits::default()
    };
    assert!(matches!(
        bind_s2_document(&fixture(), &handlers, limits),
        Err(S2DocumentError::LimitExceeded(_))
    ));
}

#[test]
fn duplicate_nodes_and_dependencies_are_rejected_by_the_real_validator() {
    let handlers = registry(Arc::new(AtomicUsize::new(0)));
    let mut duplicate_node = fixture();
    duplicate_node.operations[1].id = "load".to_owned();
    assert!(matches!(
        bind_s2_document(&duplicate_node, &handlers, S2DocumentLimits::default()),
        Err(S2DocumentError::InvalidPlan(_))
    ));

    let mut duplicate_edge = fixture();
    duplicate_edge.operations[1]
        .dependencies
        .push("load".into());
    assert!(matches!(
        bind_s2_document(&duplicate_edge, &handlers, S2DocumentLimits::default()),
        Err(S2DocumentError::InvalidPlan(_))
    ));
}

#[test]
fn malformed_plan_identity_and_unregistered_editor_permissions_fail_before_effects() {
    let hits = Arc::new(AtomicUsize::new(0));
    let handlers = registry(Arc::clone(&hits));
    let mut invalid_identity = fixture();
    invalid_identity.plan_id = "invalid plan name".into();
    assert!(matches!(
        bind_s2_document(&invalid_identity, &handlers, S2DocumentLimits::default()),
        Err(S2DocumentError::InvalidPlan(_))
    ));

    let mut invalid_revision = fixture();
    invalid_revision.semantic_revision = 0;
    assert!(matches!(
        bind_s2_document(&invalid_revision, &handlers, S2DocumentLimits::default()),
        Err(S2DocumentError::InvalidPlan(_))
    ));

    let unapproved = OperationRegistry::<Handler>::new();
    assert!(matches!(
        bind_s2_document(&fixture(), &unapproved, S2DocumentLimits::default()),
        Err(S2DocumentError::MissingEditorDescriptor { .. })
    ));
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn permitted_document_node_edits_reuse_the_existing_plan_semantics() {
    let hits = Arc::new(AtomicUsize::new(0));
    let handlers = registry(Arc::clone(&hits));
    let mut edited = fixture();
    edited.operations.pop();
    edited.semantic_revision = 2;
    let bound = bind_s2_document(&edited, &handlers, S2DocumentLimits::default()).unwrap();
    assert_eq!(bound.plan().description().revision(), 2);
    assert_eq!(bound.plan().description().operations().len(), 1);
    edited.operations.push(fixture().operations[1].clone());
    assert_eq!(
        bind_s2_document(&edited, &handlers, S2DocumentLimits::default())
            .unwrap()
            .plan()
            .description()
            .operations()
            .len(),
        2
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn byte_limit_rejects_an_oversized_untrusted_document_without_parsing() {
    let limit = S2DocumentLimits::default();
    let input = vec![b' '; limit.max_bytes + 1];
    assert!(matches!(
        inspect_s2_source(&input, limit),
        Err(S2DocumentError::LimitExceeded("source byte budget"))
    ));
}
