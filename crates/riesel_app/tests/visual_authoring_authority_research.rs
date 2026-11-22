//! Research-only #111: probe descriptor safety and V2 publication boundaries
//! against the REAL production execution-plan validator and binding registry.
//! This test-local gate/ledger is not a published SDK, durable store, exporter,
//! plan document schema, authorization system, or accepted architecture.
//! The adapter deliberately has no ability to invoke callbacks during preflight.

use riesel_app::{
    AccessMode, DomainAdmissionMode, ExecutionPlanDraft, OperationKey, OperationNode,
    OperationRegistry, OperationTrigger, ValidatedExecutionPlan,
};
use serde::Deserialize;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc, Barrier, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

type Callback = dyn Fn() + Send + Sync;

fn key(name: &str, version: u32) -> OperationKey {
    OperationKey::new(name, version).unwrap()
}

fn validated(id: &str, revision: u64, nodes: Vec<OperationNode>) -> ValidatedExecutionPlan {
    let mut draft = ExecutionPlanDraft::new(id, revision);
    for node in nodes {
        draft = draft.operation(node);
    }
    draft.validate().unwrap()
}

fn baseline() -> ValidatedExecutionPlan {
    validated(
        "research.plan",
        1,
        vec![
            OperationNode::new("load", key("research.load", 1)).access("store", AccessMode::Write),
            OperationNode::new("frame", key("research.frame", 2))
                .after("load")
                .delegated_frame("simulation"),
        ],
    )
}

fn registry(hits: Arc<AtomicUsize>) -> OperationRegistry<Callback> {
    let mut registry = OperationRegistry::<Callback>::new();
    let first = Arc::clone(&hits);
    registry
        .register(
            key("research.load", 1),
            Arc::new(move || {
                first.fetch_add(1, Ordering::SeqCst);
            }),
        )
        .unwrap();
    registry
        .register(key("research.frame", 2), Arc::new(|| {}))
        .unwrap();
    registry
}

/// Research-only metadata: the authoritative publisher, not an editor,
/// supplies minimum safety constraints for an exact registered Rust operation.
#[derive(Clone)]
struct Descriptor {
    binding: OperationKey,
    required_resources: Vec<(&'static str, AccessMode)>,
    domain: Option<(&'static str, DomainAdmissionMode)>,
    allowed_signal: Option<&'static str>,
}

fn descriptors() -> BTreeMap<OperationKey, Descriptor> {
    let load = Descriptor {
        binding: key("research.load", 1),
        required_resources: vec![("store", AccessMode::Write)],
        domain: None,
        allowed_signal: Some("ready"),
    };
    let frame = Descriptor {
        binding: key("research.frame", 2),
        required_resources: vec![],
        domain: Some(("simulation", DomainAdmissionMode::DelegatedFrame)),
        allowed_signal: None,
    };
    BTreeMap::from([(load.binding.clone(), load), (frame.binding.clone(), frame)])
}

/// The actual SDK's exact binding is performed AFTER publisher-policy checks.
/// Matching a registry key alone is not authorization to edit or execute.
fn research_preflight(
    plan: &ValidatedExecutionPlan,
    operations: &OperationRegistry<Callback>,
    catalog: &BTreeMap<OperationKey, Descriptor>,
    claimed_artifact: &str,
) -> Result<(), String> {
    if claimed_artifact != "trusted.artifact.v1" {
        return Err("artifact provenance mismatch".into());
    }
    for node in plan.description().operations() {
        let descriptor = catalog
            .get(node.binding())
            .ok_or_else(|| format!("unpublished operation {}", node.binding().name()))?;
        for &(resource, mode) in &descriptor.required_resources {
            if !node
                .resources()
                .iter()
                .any(|access| access.resource() == resource && access.mode() == mode)
            {
                return Err(format!("required resource declaration absent: {resource}"));
            }
        }
        match (&descriptor.domain, node.domain_boundary()) {
            (None, None) => {}
            (Some((name, mode)), Some(actual))
                if actual.domain() == *name && actual.mode() == *mode => {}
            _ => return Err("domain admission owner mismatch".into()),
        }
        match node.trigger() {
            OperationTrigger::Immediate => {}
            OperationTrigger::Signal(s) if descriptor.allowed_signal == Some(s.as_str()) => {}
            _ => return Err("undeclared signal trigger".into()),
        }
    }
    plan.bind(operations).map_err(|error| error.to_string())?;
    Ok(())
}

#[test]
fn production_bind_alone_does_not_verify_required_resource_or_domain() {
    let hits = Arc::new(AtomicUsize::new(0));
    let registry = registry(Arc::clone(&hits));
    let forged = validated(
        "research.plan",
        2,
        vec![
            OperationNode::new("load", key("research.load", 1)),
            OperationNode::new("frame", key("research.frame", 2))
                .after("load")
                .domain_tick("simulation"),
        ],
    );
    // Baseline negative: current exact handler lookup DOES allow this forged
    // declaration. It proves an editor-safe publisher gate is still needed.
    assert!(forged.bind(&registry).is_ok());
    assert!(research_preflight(&forged, &registry, &descriptors(), "trusted.artifact.v1").is_err());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[test]
fn trusted_descriptor_rejects_forgery_and_checks_exact_version_without_effects() {
    let hits = Arc::new(AtomicUsize::new(0));
    let registry = registry(Arc::clone(&hits));
    let catalog = descriptors();
    assert!(research_preflight(&baseline(), &registry, &catalog, "trusted.artifact.v1").is_ok());
    assert!(research_preflight(&baseline(), &registry, &catalog, "forged.artifact").is_err());

    let missing_resource = validated(
        "research.plan",
        2,
        vec![OperationNode::new("load", key("research.load", 1))],
    );
    assert!(
        research_preflight(
            &missing_resource,
            &registry,
            &catalog,
            "trusted.artifact.v1"
        )
        .unwrap_err()
        .contains("required resource")
    );

    let wrong_domain = validated(
        "research.plan",
        2,
        vec![OperationNode::new("frame", key("research.frame", 2)).domain_tick("simulation")],
    );
    assert!(
        research_preflight(&wrong_domain, &registry, &catalog, "trusted.artifact.v1")
            .unwrap_err()
            .contains("domain")
    );

    let unknown_version = validated(
        "research.plan",
        2,
        vec![
            OperationNode::new("load", key("research.load", 3)).access("store", AccessMode::Write),
        ],
    );
    assert!(
        research_preflight(&unknown_version, &registry, &catalog, "trusted.artifact.v1").is_err()
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0);
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResearchDocument {
    format_version: u32,
    required_features: Vec<String>,
    plan_id: String,
    semantic_revision: u64,
    operations: Vec<ResearchDocumentNode>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResearchDocumentNode {
    id: String,
    key: String,
    version: u32,
    #[serde(default)]
    dependencies: Vec<String>,
}

fn import_s2_document(doc: serde_json::Value) -> Result<ValidatedExecutionPlan, String> {
    // A test-local strict reader, NOT a public manifest or SDK codec.
    let document: ResearchDocument =
        serde_json::from_value(doc).map_err(|error| error.to_string())?;
    if document.format_version != 1 || !document.required_features.is_empty() {
        return Err("unsupported executable semantic version/features".into());
    }
    let mut draft = ExecutionPlanDraft::new(document.plan_id, document.semantic_revision);
    for node in document.operations {
        let mut operation = OperationNode::new(
            node.id,
            OperationKey::new(node.key, node.version).map_err(|error| error.to_string())?,
        );
        for dependency in node.dependencies {
            operation = operation.after(dependency);
        }
        draft = draft.operation(operation);
    }
    draft.validate().map_err(|error| error.to_string())
}

#[test]
fn unsupported_s3_required_features_are_not_dispatched_or_lossily_rewritten() {
    let simple = json!({
        "format_version": 1,
        "required_features": [],
        "plan_id": "research.document",
        "semantic_revision": 1,
        "operations": [{"id": "node", "key": "research.load", "version": 1}]
    });
    assert_eq!(
        import_s2_document(simple.clone())
            .unwrap()
            .description()
            .revision(),
        1
    );
    let mut future = simple.clone();
    future["required_features"] = json!(["typed_dataflow.v1"]);
    future["data_edges"] = json!([{"from": "node", "to": "sink", "port": "typed"}]);
    let retained_read_only = future.clone();
    assert!(import_s2_document(future).is_err());
    assert_eq!(retained_read_only["data_edges"][0]["port"], "typed");
    let mut bad_version = simple;
    bad_version["operations"][0]["version"] = json!(0);
    assert!(import_s2_document(bad_version).is_err());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Rust,
    Document,
}

/// In-memory test model ONLY: no persistent storage, remote trust,
/// atomic cross-process writes, or production activation behavior implied.
struct LocalLedger {
    source: Source,
    source_generation: u64,
    semantic_revision: u64,
    layout_revision: u64,
    active_revision: u64,
    plans: BTreeMap<u64, ValidatedExecutionPlan>,
    activation_receipts: BTreeMap<String, u64>,
}

impl LocalLedger {
    fn new() -> Self {
        Self {
            source: Source::Rust,
            source_generation: 1,
            semantic_revision: 1,
            layout_revision: 1,
            active_revision: 1,
            plans: BTreeMap::from([(1, baseline())]),
            activation_receipts: BTreeMap::new(),
        }
    }

    fn publish(
        &mut self,
        source: Source,
        expected_revision: u64,
        expected_generation: u64,
        plan: ValidatedExecutionPlan,
    ) -> Result<(), &'static str> {
        if self.source != source || self.source_generation != expected_generation {
            return Err("stale semantic source owner");
        }
        if self.semantic_revision != expected_revision
            || plan.description().revision() != expected_revision + 1
            || plan.description().id() != "research.plan"
        {
            return Err("stale or invalid semantic predecessor");
        }
        self.semantic_revision += 1;
        self.plans.insert(self.semantic_revision, plan);
        Ok(())
    }

    fn save_layout(
        &mut self,
        expected_layout: u64,
        expected_source_generation: u64,
        anchor: u64,
        node_id: &str,
    ) -> Result<(), &'static str> {
        if expected_source_generation != self.source_generation {
            return Err("stale layout source generation");
        }
        if expected_layout != self.layout_revision {
            return Err("stale layout revision");
        }
        let Some(plan) = self.plans.get(&anchor) else {
            return Err("unknown semantic layout anchor");
        };
        if !plan
            .description()
            .operations()
            .iter()
            .any(|node| node.id() == node_id)
        {
            return Err("layout references missing node");
        }
        self.layout_revision += 1;
        Ok(())
    }

    fn transfer(
        &mut self,
        expected_source: Source,
        expected_revision: u64,
        expected_generation: u64,
        new_source: Source,
        all_semantics_representable: bool,
    ) -> Result<(), &'static str> {
        if self.source != expected_source
            || self.semantic_revision != expected_revision
            || self.source_generation != expected_generation
        {
            return Err("stale source transfer");
        }
        if !all_semantics_representable {
            return Err("unsupported Rust semantics block transfer");
        }
        self.source = new_source;
        self.source_generation += 1;
        Ok(())
    }

    fn activate(
        &mut self,
        permission: &str,
        expected_active: u64,
        target_revision: u64,
        request_id: &str,
    ) -> Result<u64, &'static str> {
        if permission != "grant:activate" {
            return Err("unauthorized mutation");
        }
        if let Some(previous) = self.activation_receipts.get(request_id) {
            return (*previous == target_revision)
                .then_some(*previous)
                .ok_or("receipt intent mismatch");
        }
        if self.active_revision != expected_active {
            return Err("stale active revision");
        }
        if !self.plans.contains_key(&target_revision) {
            return Err("candidate not published");
        }
        self.active_revision = target_revision;
        self.activation_receipts
            .insert(request_id.to_owned(), target_revision);
        Ok(target_revision)
    }
}

#[test]
fn concurrent_writers_only_one_semantic_update_wins() {
    let ledger = Arc::new(Mutex::new(LocalLedger::new()));
    ledger
        .lock()
        .unwrap()
        .transfer(Source::Rust, 1, 1, Source::Document, true)
        .unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for id in ["first", "second"] {
        let ledger = Arc::clone(&ledger);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            let plan = validated(
                "research.plan",
                2,
                vec![
                    OperationNode::new(id, key("research.load", 1))
                        .access("store", AccessMode::Write),
                ],
            );
            barrier.wait();
            ledger.lock().unwrap().publish(Source::Document, 1, 2, plan)
        }));
    }
    barrier.wait();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let state = ledger.lock().unwrap();
    assert_eq!(state.semantic_revision, 2);
    assert_eq!(state.active_revision, 1);
    assert_eq!(state.layout_revision, 1);
}

#[test]
fn layouts_do_not_mutate_semantics_and_reject_missing_nodes_and_stale_edits() {
    let mut ledger = LocalLedger::new();
    let old = serde_json::to_value(ledger.plans.get(&1).unwrap().description()).unwrap();
    ledger.save_layout(1, 1, 1, "load").unwrap();
    assert!(ledger.save_layout(1, 1, 1, "frame").is_err());
    assert!(ledger.save_layout(2, 1, 1, "ghost").is_err());
    assert!(ledger.save_layout(2, 1, 999, "load").is_err());
    assert_eq!(ledger.semantic_revision, 1);
    assert_eq!(ledger.active_revision, 1);
    assert_eq!(
        serde_json::to_value(ledger.plans.get(&1).unwrap().description()).unwrap(),
        old
    );
}

#[test]
fn explicit_source_handoff_refuses_loss_and_fences_old_writers() {
    let mut ledger = LocalLedger::new();
    assert!(
        ledger
            .transfer(Source::Rust, 1, 1, Source::Document, false)
            .is_err()
    );
    assert_eq!(ledger.source, Source::Rust);
    ledger
        .transfer(Source::Rust, 1, 1, Source::Document, true)
        .unwrap();
    assert_eq!(ledger.source_generation, 2);
    assert!(
        ledger
            .publish(
                Source::Rust,
                1,
                1,
                validated(
                    "research.plan",
                    2,
                    vec![OperationNode::new("stale", key("research.load", 1))]
                )
            )
            .is_err()
    );
    assert!(ledger.save_layout(1, 1, 1, "load").is_err());
    assert!(
        ledger
            .transfer(Source::Rust, 1, 1, Source::Document, true)
            .is_err()
    );
    assert_eq!(ledger.semantic_revision, 1);
}

#[test]
fn candidate_publication_is_distinct_from_activation_and_uncertain_reply_can_resnapshot() {
    let mut ledger = LocalLedger::new();
    ledger
        .transfer(Source::Rust, 1, 1, Source::Document, true)
        .unwrap();
    ledger
        .publish(
            Source::Document,
            1,
            2,
            validated(
                "research.plan",
                2,
                vec![
                    OperationNode::new("load", key("research.load", 1))
                        .access("store", AccessMode::Write),
                ],
            ),
        )
        .unwrap();
    assert_eq!(ledger.active_revision, 1);
    assert!(ledger.activate("grant:read", 1, 2, "request-1").is_err());
    assert_eq!(ledger.active_revision, 1);
    // After dispatch the acknowledgement could be lost. In this test-local
    // state the authoritative activation/receipt can be resampled.
    assert_eq!(ledger.activate("grant:activate", 1, 2, "request-1"), Ok(2));
    assert_eq!(ledger.active_revision, 2);
    assert_eq!(ledger.activation_receipts.get("request-1"), Some(&2));
    assert_eq!(ledger.activate("grant:activate", 1, 2, "request-1"), Ok(2));
    assert!(
        ledger
            .activate("grant:activate", 1, 1, "request-1")
            .is_err()
    );
    assert!(
        ledger
            .activate("grant:activate", 1, 1, "request-2")
            .is_err()
    );
}

#[test]
fn accepted_semantic_revisions_stay_stable_when_later_candidates_are_published() {
    let mut ledger = LocalLedger::new();
    let old = serde_json::to_value(ledger.plans.get(&1).unwrap().description()).unwrap();
    ledger
        .publish(
            Source::Rust,
            1,
            1,
            validated(
                "research.plan",
                2,
                vec![OperationNode::new("next", key("research.load", 1))],
            ),
        )
        .unwrap();
    assert_eq!(ledger.active_revision, 1);
    assert_eq!(
        serde_json::to_value(ledger.plans.get(&1).unwrap().description()).unwrap(),
        old
    );
    assert_eq!(ledger.plans.get(&2).unwrap().description().revision(), 2);
    assert_eq!(ledger.plans.len(), 2);
}

#[test]
fn separate_view_does_not_invent_value_edges() {
    let plan = baseline();
    let nodes: BTreeSet<_> = plan
        .description()
        .operations()
        .iter()
        .map(|node| node.id())
        .collect();
    assert_eq!(nodes, BTreeSet::from(["load", "frame"]));
    let frame = plan
        .description()
        .operations()
        .iter()
        .find(|node| node.id() == "frame")
        .unwrap();
    assert_eq!(frame.dependencies(), &["load".to_owned()]);
    // Current production plan dependencies are control prerequisites; no
    // typed payload or data-ready semantics are exposed by this contract.
    assert_eq!(plan.description().revision(), 1);
}
