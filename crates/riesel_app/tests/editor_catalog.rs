//! #191 public Rust SDK proof: only trusted app registration can make
//! an exact Rust handler editor-selectable, and preflight never invokes it.
use riesel_app::{
    AccessMode, DomainAdmissionMode, EditorAffinity, EditorCatalogError, EditorOperationDescriptor,
    ExecutionPlanDraft, OperationKey, OperationNode, OperationRegistry, ValidatedExecutionPlan,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

type Handler = dyn Fn() + Send + Sync;

fn key(name: &str, version: u32) -> OperationKey {
    OperationKey::new(name, version).unwrap()
}

fn plan(node: OperationNode) -> ValidatedExecutionPlan {
    ExecutionPlanDraft::new("editor.example", 1)
        .operation(node)
        .validate()
        .unwrap()
}

fn descriptor() -> EditorOperationDescriptor {
    EditorOperationDescriptor::new(key("editor.update", 1))
        .require_access("state", AccessMode::Write)
        .permit_extra_access("cache", AccessMode::Read)
        .require_domain("simulation", DomainAdmissionMode::DelegatedFrame)
        .permit_signal("ready")
}

fn registry(counter: Arc<AtomicUsize>) -> OperationRegistry<Handler> {
    let mut registry = OperationRegistry::<Handler>::new();
    registry
        .register_editor_operation(
            descriptor(),
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
    registry
}

fn valid_node() -> OperationNode {
    OperationNode::new("update", key("editor.update", 1))
        .access("state", AccessMode::Write)
        .delegated_frame("simulation")
}

#[test]
fn trusted_exact_registration_and_editor_binding_are_passive() {
    let counter = Arc::new(AtomicUsize::new(0));
    let registry = registry(Arc::clone(&counter));
    let inspected = registry.editor_descriptors();
    assert_eq!(inspected.len(), 1);
    assert_eq!(inspected[0].key(), &key("editor.update", 1));
    assert_eq!(inspected[0].required_access().len(), 1);
    assert_eq!(inspected[0].permitted_extra_access().len(), 1);
    assert_eq!(
        inspected[0].execution_affinity(),
        EditorAffinity::SessionOwned
    );
    assert_eq!(
        inspected[0].required_domain(),
        Some(("simulation", DomainAdmissionMode::DelegatedFrame))
    );
    let static_json = serde_json::to_value(inspected[0]).unwrap();
    assert_eq!(static_json["key"]["version"], 1);
    assert_eq!(static_json["descriptor_version"], 1);
    assert_eq!(inspected[0].descriptor_version(), 1);

    let edited = plan(
        valid_node()
            .access("cache", AccessMode::Read)
            .on_signal("ready"),
    );
    let bound = registry.bind_editor_plan(&edited).unwrap();
    assert!(bound.handler("update").is_some());
    assert_eq!(counter.load(Ordering::SeqCst), 0);
    // Explicit invocation by the trusted app remains a separate operation.
    bound.handler("update").unwrap()();
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[test]
fn editor_cannot_omit_or_weaken_publisher_required_write_access() {
    let counter = Arc::new(AtomicUsize::new(0));
    let registry = registry(Arc::clone(&counter));
    let missing =
        plan(OperationNode::new("update", key("editor.update", 1)).delegated_frame("simulation"));
    assert!(matches!(
        registry.bind_editor_plan(&missing),
        Err(EditorCatalogError::DisallowedNode { .. })
    ));
    let read_only = plan(
        OperationNode::new("update", key("editor.update", 1))
            .access("state", AccessMode::Read)
            .delegated_frame("simulation"),
    );
    assert!(matches!(
        registry.bind_editor_plan(&read_only),
        Err(EditorCatalogError::DisallowedNode { .. })
    ));
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[test]
fn editor_rejects_unapproved_resources_domain_changes_and_signals() {
    let counter = Arc::new(AtomicUsize::new(0));
    let registry = registry(Arc::clone(&counter));
    let unknown_resource = plan(valid_node().access("secret", AccessMode::Write));
    let wrong_mode_for_optional = plan(valid_node().access("cache", AccessMode::Write));
    let wrong_domain = plan(
        OperationNode::new("update", key("editor.update", 1))
            .access("state", AccessMode::Write)
            .domain_tick("simulation"),
    );
    let wrong_name = plan(
        OperationNode::new("update", key("editor.update", 1))
            .access("state", AccessMode::Write)
            .delegated_frame("another-domain"),
    );
    let unexpected_signal = plan(valid_node().on_signal("other"));
    for proposal in [
        unknown_resource,
        wrong_mode_for_optional,
        wrong_domain,
        wrong_name,
        unexpected_signal,
    ] {
        assert!(matches!(
            registry.bind_editor_plan(&proposal),
            Err(EditorCatalogError::DisallowedNode { .. })
        ));
    }
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[test]
fn exact_version_missing_and_legacy_handler_are_not_editor_approved() {
    let counter = Arc::new(AtomicUsize::new(0));
    let registry = registry(Arc::clone(&counter));
    let new_version = plan(OperationNode::new("update", key("editor.update", 2)));
    assert!(matches!(
        registry.bind_editor_plan(&new_version),
        Err(EditorCatalogError::NotEditable { version: 2, .. })
    ));

    let mut legacy = OperationRegistry::<Handler>::new();
    legacy
        .register(key("editor.update", 1), Arc::new(|| {}))
        .unwrap();
    let old = plan(valid_node());
    // Existing trusted Rust-only binding remains backwards compatible.
    assert!(old.bind(&legacy).is_ok());
    assert!(matches!(
        legacy.bind_editor_plan(&old),
        Err(EditorCatalogError::NotEditable { version: 1, .. })
    ));
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[test]
fn unsafe_publisher_descriptors_are_rejected_before_handler_registration() {
    let mut registry = OperationRegistry::<Handler>::new();
    let repeated = EditorOperationDescriptor::new(key("editor.bad", 1))
        .require_access("state", AccessMode::Write)
        .permit_extra_access("state", AccessMode::Read);
    assert!(matches!(
        registry.register_editor_operation(repeated, Arc::new(|| {})),
        Err(EditorCatalogError::InvalidDescriptor { .. })
    ));
    let invalid_signal =
        EditorOperationDescriptor::new(key("editor.bad", 2)).permit_signal("invalid signal");
    assert!(matches!(
        registry.register_editor_operation(invalid_signal, Arc::new(|| {})),
        Err(EditorCatalogError::InvalidDescriptor { .. })
    ));
    let affine = EditorOperationDescriptor::new(key("editor.native", 1))
        .affinity(EditorAffinity::NativeThread);
    assert!(matches!(
        registry.register_editor_operation(affine, Arc::new(|| {})),
        Err(EditorCatalogError::UnsupportedAffinity { .. })
    ));
    let zero_version =
        EditorOperationDescriptor::new(key("editor.zero", 1)).with_descriptor_version(0);
    assert!(matches!(
        registry.register_editor_operation(zero_version, Arc::new(|| {})),
        Err(EditorCatalogError::InvalidDescriptor { .. })
    ));
    assert_eq!(registry.editor_descriptors().len(), 0);
    assert!(registry.get(&key("editor.bad", 1)).is_none());
    assert!(registry.get(&key("editor.native", 1)).is_none());
}

#[test]
fn publisher_required_signal_cannot_be_removed_for_immediate_admission() {
    let mut registry = OperationRegistry::<Handler>::new();
    registry
        .register_editor_operation(
            EditorOperationDescriptor::new(key("editor.signal", 1)).require_signal("clock"),
            Arc::new(|| {}),
        )
        .unwrap();

    let immediate = plan(OperationNode::new("step", key("editor.signal", 1)));
    assert!(matches!(
        registry.bind_editor_plan(&immediate),
        Err(EditorCatalogError::DisallowedNode { .. })
    ));
    let correctly_signalled =
        plan(OperationNode::new("step", key("editor.signal", 1)).on_signal("clock"));
    assert!(registry.bind_editor_plan(&correctly_signalled).is_ok());
}

#[test]
fn duplicate_editor_registration_fails_without_replacing_existing_callback() {
    let counter = Arc::new(AtomicUsize::new(0));
    let mut registry = registry(Arc::clone(&counter));
    assert!(matches!(
        registry.register_editor_operation(descriptor(), Arc::new(|| {})),
        Err(EditorCatalogError::Plan(_))
    ));
    assert_eq!(registry.editor_descriptors().len(), 1);
    let bound = registry.bind_editor_plan(&plan(valid_node())).unwrap();
    bound.handler("update").unwrap()();
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}
