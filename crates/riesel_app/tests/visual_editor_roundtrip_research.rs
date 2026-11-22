//! Research-only #111: executable plan import via an explicitly bounded,
//! local-authority document adapter. This is NOT a production document ABI,
//! public SDK API, authorized external editor or Rust source round-trip.
//! Test against the actual immutable ExecutionPlanDraft/Registry/Catalog API.

use riesel_app::{
    AccessMode, ExecutionPlanDraft, ExecutionPlanError, OperationKey, OperationNode,
    OperationRegistry, PlanRevisionCatalog, ValidatedExecutionPlan,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditDocument {
    format_version: u32,
    id: String,
    semantic_revision: u64,
    operations: Vec<EditNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditNode {
    id: String,
    binding: SymbolicBinding,
    #[serde(default)]
    dependencies: Vec<String>,
    #[serde(default)]
    signal: Option<String>,
    #[serde(default)]
    accesses: Vec<EditAccess>,
    #[serde(default)]
    domain: Option<EditDomain>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SymbolicBinding {
    name: String,
    version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditAccess {
    resource: String,
    mode: EditAccessMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EditAccessMode {
    Read,
    Write,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditDomain {
    name: String,
    mode: EditDomainMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EditDomainMode {
    RieselTick,
    DelegatedFrame,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutDocument {
    format_version: u32,
    plan_id: String,
    semantic_revision: u64,
    layout_revision: u64,
    positions: BTreeMap<String, Position>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Position {
    x: i32,
    y: i32,
}

/// Research conversion; no format, trust or publishing policy is approved.
/// Import must construct actual SDK nodes and pass the existing validator.
fn reconstruct(doc: &EditDocument) -> Result<ValidatedExecutionPlan, String> {
    if doc.format_version != 1 {
        return Err("unsupported semantic document format".into());
    }
    let mut draft = ExecutionPlanDraft::new(&doc.id, doc.semantic_revision);
    for node in &doc.operations {
        let key = OperationKey::new(&node.binding.name, node.binding.version)
            .map_err(|e| e.to_string())?;
        let mut operation = OperationNode::new(&node.id, key);
        for dependency in &node.dependencies {
            operation = operation.after(dependency);
        }
        if let Some(signal) = &node.signal {
            operation = operation.on_signal(signal);
        }
        for access in &node.accesses {
            let mode = match access.mode {
                EditAccessMode::Read => AccessMode::Read,
                EditAccessMode::Write => AccessMode::Write,
            };
            operation = operation.access(&access.resource, mode);
        }
        if let Some(domain) = &node.domain {
            operation = match domain.mode {
                EditDomainMode::RieselTick => operation.domain_tick(&domain.name),
                EditDomainMode::DelegatedFrame => operation.delegated_frame(&domain.name),
            };
        }
        draft = draft.operation(operation);
    }
    draft.validate().map_err(|e| e.to_string())
}

fn check_layout(layout: &LayoutDocument, plan: &ValidatedExecutionPlan) -> Result<(), String> {
    if layout.format_version != 1 || layout.layout_revision == 0 {
        return Err("unsupported layout format or revision".into());
    }
    if layout.plan_id != plan.description().id()
        || layout.semantic_revision != plan.description().revision()
    {
        return Err("layout is stale or belongs to another semantic plan".into());
    }
    for node_id in layout.positions.keys() {
        if !plan
            .description()
            .operations()
            .iter()
            .any(|operation| operation.id() == node_id)
        {
            return Err(format!("layout references unknown operation {node_id}"));
        }
    }
    Ok(())
}

fn fixture() -> EditDocument {
    EditDocument {
        format_version: 1,
        id: "research.pipeline".into(),
        semantic_revision: 1,
        operations: vec![
            EditNode {
                id: "input".into(),
                binding: SymbolicBinding {
                    name: "research.input".into(),
                    version: 1,
                },
                dependencies: vec![],
                signal: Some("ready".into()),
                accesses: vec![EditAccess {
                    resource: "records".into(),
                    mode: EditAccessMode::Read,
                }],
                domain: None,
            },
            EditNode {
                id: "frame".into(),
                binding: SymbolicBinding {
                    name: "research.frame".into(),
                    version: 2,
                },
                dependencies: vec!["input".into()],
                signal: None,
                accesses: vec![],
                domain: Some(EditDomain {
                    name: "simulation".into(),
                    mode: EditDomainMode::DelegatedFrame,
                }),
            },
        ],
    }
}

fn production_projection(plan: &ValidatedExecutionPlan) -> Value {
    serde_json::to_value(plan.description()).expect("supported production inspection projection")
}

#[test]
fn bounded_document_and_code_authored_subset_agree_on_actual_plan_semantics() {
    let document = fixture();
    let code_owned = ExecutionPlanDraft::new("research.pipeline", 1)
        .operation(
            OperationNode::new("input", OperationKey::new("research.input", 1).unwrap())
                .on_signal("ready")
                .access("records", AccessMode::Read),
        )
        .operation(
            OperationNode::new("frame", OperationKey::new("research.frame", 2).unwrap())
                .after("input")
                .delegated_frame("simulation"),
        )
        .validate()
        .unwrap();
    let encoded = serde_json::to_string(&document).unwrap();
    let decoded: EditDocument = serde_json::from_str(&encoded).unwrap();
    let imported = reconstruct(&decoded).unwrap();

    assert_eq!(decoded, document);
    assert_eq!(production_projection(&code_owned), production_projection(&imported));
    assert_eq!(
        imported
            .dependency_order()
            .map(OperationNode::id)
            .collect::<Vec<_>>(),
        vec!["input", "frame"]
    );
}

#[test]
fn import_and_bind_are_passive_and_missing_or_changed_code_fails_closed() {
    let counter = Arc::new(AtomicUsize::new(0));
    let copy = Arc::clone(&counter);
    let mut registry = OperationRegistry::<dyn Fn() + Send + Sync>::new();
    registry
        .register(
            OperationKey::new("research.input", 1).unwrap(),
            Arc::new(move || {
                copy.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
    registry
        .register(
            OperationKey::new("research.frame", 2).unwrap(),
            Arc::new(|| {}),
        )
        .unwrap();

    let document = fixture();
    let plan = reconstruct(&document).unwrap();
    let bound = plan.bind(&registry).unwrap();
    assert!(bound.handler("frame").is_some());
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    let mut unknown = document;
    unknown.operations[1].binding.version = 3;
    let plan = reconstruct(&unknown).unwrap();
    assert!(matches!(
        plan.bind(&registry),
        Err(ExecutionPlanError::MissingBinding { version: 3, .. })
    ));
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

#[test]
fn unsupported_fields_versions_and_invalid_graphs_fail_before_binding() {
    let document = fixture();
    let mut untrusted = serde_json::to_value(&document).unwrap();
    untrusted["operations"][0]["arbitrary_rust"] = json!("std::process::Command");
    assert!(serde_json::from_value::<EditDocument>(untrusted).is_err());

    let mut fake_typed_dataflow = serde_json::to_value(&document).unwrap();
    fake_typed_dataflow["data_edges"] = json!([{"from": "input", "to": "frame"}]);
    assert!(serde_json::from_value::<EditDocument>(fake_typed_dataflow).is_err());

    let mut unsupported = document.clone();
    unsupported.format_version = 2;
    assert!(reconstruct(&unsupported).is_err());

    let mut cycle = document.clone();
    cycle.operations[0].dependencies.push("frame".into());
    assert!(reconstruct(&cycle).unwrap_err().contains("cyclic"));

    let mut conflict = document;
    conflict.operations[0].domain = Some(EditDomain {
        name: "simulation".into(),
        mode: EditDomainMode::RieselTick,
    });
    assert!(reconstruct(&conflict).unwrap_err().contains("conflicting"));
}

#[test]
fn semantic_changes_require_new_immutable_revision_and_old_plan_is_stable() {
    let original = fixture();
    let old_plan = reconstruct(&original).unwrap();
    let old_projection = production_projection(&old_plan);
    let mut catalog = PlanRevisionCatalog::new();
    catalog.publish(old_plan).unwrap();

    let mut edited = original.clone();
    edited.semantic_revision = 2;
    edited.operations.push(EditNode {
        id: "audit".into(),
        binding: SymbolicBinding {
            name: "research.audit".into(),
            version: 1,
        },
        dependencies: vec!["frame".into()],
        signal: None,
        accesses: vec![],
        domain: None,
    });
    catalog.publish(reconstruct(&edited).unwrap()).unwrap();

    // Same revision cannot be republished even if bytewise identical.
    assert!(matches!(
        catalog.publish(reconstruct(&original).unwrap()),
        Err(ExecutionPlanError::DuplicatePlanRevision { revision: 1, .. })
    ));
    assert_eq!(
        production_projection(catalog.get("research.pipeline", 1).unwrap()),
        old_projection
    );
    assert_eq!(
        catalog.get("research.pipeline", 2).unwrap().description().operations().len(),
        3
    );
}

#[test]
fn separate_layout_revision_cannot_mutate_execution_and_rejects_stale_nodes() {
    let plan = reconstruct(&fixture()).unwrap();
    let before = production_projection(&plan);
    let mut layout = LayoutDocument {
        format_version: 1,
        plan_id: "research.pipeline".into(),
        semantic_revision: 1,
        layout_revision: 1,
        positions: BTreeMap::from([("input".into(), Position { x: 4, y: 8 })]),
    };
    check_layout(&layout, &plan).unwrap();
    let round_trip: LayoutDocument =
        serde_json::from_str(&serde_json::to_string(&layout).unwrap()).unwrap();
    assert_eq!(round_trip, layout);

    layout.layout_revision = 2;
    layout
        .positions
        .insert("frame".into(), Position { x: 300, y: 8 });
    check_layout(&layout, &plan).unwrap();
    assert_eq!(before, production_projection(&plan));

    layout.positions.insert("ghost".into(), Position { x: 0, y: 0 });
    assert!(check_layout(&layout, &plan).is_err());
    layout.positions.remove("ghost");
    layout.semantic_revision = 2;
    assert!(check_layout(&layout, &plan).is_err());
}
