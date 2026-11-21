//! Issue #83: isolated *research* document/binding/round-trip feasibility probe.
//! This is not a public plan file, accepted SDK API, or authorization to run code.
//!
//! Core boundary: only metadata is serialized; Rust operations and access facts
//! come from an explicit in-process registry. Application provider selection is
//! NOT duplicated here. External callers are not authenticated by this model.
use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

const SUPPORTED_FORMAT: u32 = 1;

/// Wire/editor input: every execution-relevant field has an exact v1 shape.
/// Additional, explicitly non-executable view metadata is allowed separately.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    format_version: u32,
    plan_id: String,
    revision: u64,
    signals: Vec<String>,
    nodes: Vec<DocumentNode>,
    #[serde(default)]
    data_edges: Vec<DataEdge>,
    #[serde(default)]
    view: BTreeMap<String, ViewPlacement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DocumentNode {
    id: String,
    operation: BindingKey,
    #[serde(default)]
    depends_on: Vec<String>,
    #[serde(default)]
    signal: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingKey {
    /// Logical operation key; NOT a capability provider, URL, or serialized closure.
    key: String,
    version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DataEdge {
    from: String,
    output: String,
    to: String,
    input: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewPlacement {
    x: i32,
    y: i32,
    #[serde(default)]
    label: Option<String>,
}

#[derive(Clone)]
struct Port {
    name: &'static str,
    data_type: &'static str,
}

#[derive(Clone)]
struct RegisteredOperation {
    version: u32,
    run: Operation,
    access: Vec<Access>,
    inputs: Vec<Port>,
    outputs: Vec<Port>,
}

/// Local test-only registry. A future production version must derive permitted
/// bindings from app/session-owned declarations, never from untrusted JSON.
#[derive(Default)]
struct Registry(BTreeMap<String, RegisteredOperation>);

impl Registry {
    fn register(&mut self, key: &str, implementation: RegisteredOperation) {
        assert!(self.0.insert(key.to_string(), implementation).is_none());
    }

    fn resolve(&self, key: &BindingKey) -> Result<&RegisteredOperation, Problem> {
        let Some(registered) = self.0.get(&key.key) else {
            return Err(Problem::UnknownOperation(key.key.clone()));
        };
        if key.version != registered.version {
            return Err(Problem::UnsupportedOperationVersion {
                key: key.key.clone(),
                requested: key.version,
            });
        }
        Ok(registered)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Problem {
    UnsupportedDocumentFormat(u32),
    MissingPlanId,
    InvalidRevision,
    DuplicateSignal(String),
    MissingSignal { node: String, signal: String },
    UnknownOperation(String),
    UnsupportedOperationVersion { key: String, requested: u32 },
    InvalidGraph(InvalidPlan),
    UnknownViewNode(String),
    UnknownEdgeNode(String),
    UnknownPort { node: String, port: String },
    TypeMismatch { from: String, to: String },
    DuplicateDataTarget { node: String, port: String },
    /// Port types can be validated, but this probe has no typed value binding
    /// or lifetime-safe transport. Execution MUST fail closed.
    DataFlowExecutionUnsupported,
    RevisionCollision { plan_id: String, revision: u64 },
}

/// Prevent reusing a (plan ID, revision) for different bytes in one observer.
/// It is NOT a persistent registry, signature, stable content hash or session
/// authorization check. A production format needs a process-independent rule.
#[derive(Default)]
struct RevisionLedger(BTreeMap<(String, u64), Vec<u8>>);

impl RevisionLedger {
    fn observe(&mut self, doc: &Document) -> Result<(), Problem> {
        let key = (doc.plan_id.clone(), doc.revision);
        let bytes = serde_json::to_vec(doc).expect("research document is serializable");
        match self.0.get(&key) {
            Some(previous) if previous != &bytes => Err(Problem::RevisionCollision {
                plan_id: doc.plan_id.clone(), revision: doc.revision,
            }),
            Some(_) => Ok(()),
            None => {
                self.0.insert(key, bytes);
                Ok(())
            }
        }
    }
}

/// Validate author-supplied metadata without calling *any* registered Rust
/// operations, opening providers, allocating a worker or starting a session.
fn inspect_validated(doc: &Document, registry: &Registry) -> Result<(), Problem> {
    if doc.format_version != SUPPORTED_FORMAT {
        return Err(Problem::UnsupportedDocumentFormat(doc.format_version));
    }
    if doc.plan_id.is_empty() {
        return Err(Problem::MissingPlanId);
    }
    if doc.revision == 0 {
        return Err(Problem::InvalidRevision);
    }

    let mut signals = BTreeSet::new();
    for signal in &doc.signals {
        if signal.is_empty() || !signals.insert(signal.as_str()) {
            return Err(Problem::DuplicateSignal(signal.clone()));
        }
    }

    let mut definitions = BTreeMap::new();
    for node in &doc.nodes {
        if definitions.insert(node.id.as_str(), node).is_some() {
            return Err(Problem::InvalidGraph(InvalidPlan::DuplicateId(node.id.clone())));
        }
        registry.resolve(&node.operation)?;
        if let Some(signal) = &node.signal {
            if !signals.contains(signal.as_str()) {
                return Err(Problem::MissingSignal {
                    node: node.id.clone(), signal: signal.clone(),
                });
            }
        }
    }

    for key in doc.view.keys() {
        if !definitions.contains_key(key.as_str()) {
            return Err(Problem::UnknownViewNode(key.clone()));
        }
    }

    // A data edge contributes dependency precedence even though executable
    // data transport is deliberately NOT implemented in this research probe.
    let mut dependencies: BTreeMap<String, Vec<String>> = doc.nodes.iter()
        .map(|node| (node.id.clone(), node.depends_on.clone())).collect();
    let mut port_targets = BTreeSet::new();
    for edge in &doc.data_edges {
        let Some(source) = definitions.get(edge.from.as_str()) else {
            return Err(Problem::UnknownEdgeNode(edge.from.clone()));
        };
        let Some(target) = definitions.get(edge.to.as_str()) else {
            return Err(Problem::UnknownEdgeNode(edge.to.clone()));
        };
        let from_binding = registry.resolve(&source.operation)?;
        let to_binding = registry.resolve(&target.operation)?;
        let Some(source_type) = from_binding.outputs.iter()
            .find(|port| port.name == edge.output).map(|port| port.data_type) else {
            return Err(Problem::UnknownPort {
                node: edge.from.clone(), port: edge.output.clone(),
            });
        };
        let Some(target_type) = to_binding.inputs.iter()
            .find(|port| port.name == edge.input).map(|port| port.data_type) else {
            return Err(Problem::UnknownPort {
                node: edge.to.clone(), port: edge.input.clone(),
            });
        };
        if source_type != target_type {
            return Err(Problem::TypeMismatch {
                from: source_type.to_owned(), to: target_type.to_owned(),
            });
        }
        if !port_targets.insert((edge.to.as_str(), edge.input.as_str())) {
            return Err(Problem::DuplicateDataTarget {
                node: edge.to.clone(), port: edge.input.clone(),
            });
        }
        let target_deps = dependencies.get_mut(&edge.to).expect("target exists");
        if !target_deps.contains(&edge.from) {
            target_deps.push(edge.from.clone());
        }
    }

    let graph: Vec<_> = doc.nodes.iter().map(|n| {
        let binding = registry.resolve(&n.operation).expect("checked above");
        Node::owned(
            n.id.clone(),
            dependencies.remove(&n.id).expect("node exists"),
            binding.access.clone(), Arc::clone(&binding.run),
        )
    }).collect();
    Plan::validate(graph).map_err(Problem::InvalidGraph)?;
    Ok(())
}

/// Binding is explicitly separate from parsing and metadata validation.
/// The production choice of operation authorization / version migration remains open.
fn bind(doc: &Document, registry: &Registry) -> Result<Arc<Plan>, Problem> {
    inspect_validated(doc, registry)?;
    if !doc.data_edges.is_empty() {
        return Err(Problem::DataFlowExecutionUnsupported);
    }

    let graph: Vec<_> = doc.nodes.iter().map(|node| {
        let binding = registry.resolve(&node.operation).expect("validated operation");
        let mut bound = Node::owned(
            node.id.clone(), node.depends_on.clone(), binding.access.clone(),
            Arc::clone(&binding.run),
        );
        if let Some(signal) = &node.signal {
            bound = bound.triggered_owned(signal.clone());
        }
        bound
    }).collect();
    Ok(Arc::new(Plan::validate(graph).map_err(Problem::InvalidGraph)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, atomic::{AtomicUsize, Ordering}};

    fn op(log: Arc<Mutex<Vec<&'static str>>>, name: &'static str) -> Operation {
        operation(move || {
            let log = Arc::clone(&log);
            async move {
                log.lock().unwrap().push(name);
                Ok(())
            }
        })
    }

    fn registry(log: Arc<Mutex<Vec<&'static str>>>) -> Registry {
        let mut registry = Registry::default();
        for key in ["ingest", "simulate", "publish", "bool_sink"] {
            let (inputs, outputs): (Vec<Port>, Vec<Port>) = match key {
                "ingest" => (vec![], vec![Port { name: "value", data_type: "i64" }]),
                "simulate" => (vec![Port { name: "input", data_type: "i64" }],
                               vec![Port { name: "state", data_type: "i64" }]),
                "bool_sink" => (vec![Port { name: "input", data_type: "bool" }], vec![]),
                _ => (vec![Port { name: "input", data_type: "i64" }], vec![]),
            };
            registry.register(key, RegisteredOperation {
                version: 1,
                run: op(Arc::clone(&log), key),
                // Trusted registration supplies access constraints, not JSON.
                access: if key == "simulate" { vec![Access::Write("simulation")] }
                        else { vec![] },
                inputs, outputs,
            });
        }
        registry
    }

    fn authored_document() -> Document {
        Document {
            format_version: SUPPORTED_FORMAT,
            plan_id: "simulation.pipeline".to_string(),
            revision: 1,
            signals: vec![],
            nodes: vec![
                DocumentNode {
                    id: "input".into(),
                    operation: BindingKey { key: "ingest".into(), version: 1 },
                    depends_on: vec![], signal: None,
                },
                DocumentNode {
                    id: "tick".into(),
                    operation: BindingKey { key: "simulate".into(), version: 1 },
                    depends_on: vec!["input".into()], signal: None,
                },
                DocumentNode {
                    id: "snapshot".into(),
                    operation: BindingKey { key: "publish".into(), version: 1 },
                    depends_on: vec!["tick".into()], signal: None,
                },
            ],
            data_edges: vec![],
            view: BTreeMap::from([(
                "tick".into(),
                ViewPlacement { x: 40, y: 30, label: Some("Simulate".into()) },
            )]),
        }
    }

    #[tokio::test]
    async fn code_document_json_round_trip_edit_and_same_runner_execution() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let registry = registry(Arc::clone(&log));
        let doc = authored_document();

        // Source-of-truth is code-authored declarations; serialization never
        // serializes or invokes the registry's closures.
        let json = serde_json::to_string_pretty(&doc).unwrap();
        let parsed: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, parsed);
        inspect_validated(&parsed, &registry).unwrap();
        assert!(log.lock().unwrap().is_empty());

        let mut ledger = RevisionLedger::default();
        ledger.observe(&parsed).unwrap();
        ledger.observe(&parsed).unwrap();
        let original = bind(&parsed, &registry).unwrap();
        assert_eq!(original.describe().len(), 3);
        let mut run = Run::new(Arc::clone(&original), 401, 1, 2, Trace::new(true, 20));
        run.advance(usize::MAX).await;
        assert_eq!(run.state("snapshot"), Some(Status::Completed));
        assert_eq!(*log.lock().unwrap(), vec!["ingest", "simulate", "publish"]);

        // A visual edit is metadata-only until the author explicitly applies
        // it as a new revision. Existing bound plans remain immutable.
        let mut edited = parsed.clone();
        edited.revision = 2;
        edited.view.get_mut("tick").unwrap().x = 150;
        edited.nodes.push(DocumentNode {
            id: "audit".into(),
            operation: BindingKey { key: "publish".into(), version: 1 },
            depends_on: vec!["snapshot".into()], signal: None,
        });
        edited.view.insert("audit".into(),
            ViewPlacement { x: 220, y: 70, label: Some("Audit".into()) });
        let encoded = serde_json::to_string(&edited).unwrap();
        let decoded: Document = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, edited);
        ledger.observe(&decoded).unwrap();
        assert_eq!(original.describe().len(), 3, "revision 1 cannot be edited in-place");

        let mut new_run = Run::new(bind(&decoded, &registry).unwrap(), 401, 2, 2, Trace::new(true, 20));
        new_run.advance(usize::MAX).await;
        assert_eq!(new_run.state("audit"), Some(Status::Completed));
        assert_eq!(new_run.state("snapshot"), Some(Status::Completed));
        assert_eq!(new_run.snapshot().len(), 4);
        assert_eq!(*log.lock().unwrap(),
            vec!["ingest", "simulate", "publish", "ingest", "simulate", "publish", "publish"]);
    }

    #[test]
    fn version_and_binding_checks_fail_closed_before_invocation() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let registry = registry(Arc::clone(&log));
        let original = authored_document();
        let mut edited = original.clone();
        edited.format_version = 2;
        assert_eq!(inspect_validated(&edited, &registry),
            Err(Problem::UnsupportedDocumentFormat(2)));

        let mut edited = original.clone();
        edited.nodes[1].operation.version = 2;
        assert_eq!(inspect_validated(&edited, &registry),
            Err(Problem::UnsupportedOperationVersion { key: "simulate".into(), requested: 2 }));

        let mut edited = original.clone();
        edited.nodes[1].operation.key = "missing".into();
        assert_eq!(inspect_validated(&edited, &registry),
            Err(Problem::UnknownOperation("missing".into())));

        let mut edited = original.clone();
        edited.nodes[0].id = "tick".into();
        assert_eq!(inspect_validated(&edited, &registry),
            Err(Problem::InvalidGraph(InvalidPlan::DuplicateId("tick".into()))));

        let mut edited = original.clone();
        edited.nodes[0].depends_on.push("snapshot".into());
        assert_eq!(inspect_validated(&edited, &registry),
            Err(Problem::InvalidGraph(InvalidPlan::Cycle)));

        let mut edited = original.clone();
        edited.view.insert("not_a_node".into(),
            ViewPlacement { x: 1, y: 1, label: None });
        assert_eq!(inspect_validated(&edited, &registry),
            Err(Problem::UnknownViewNode("not_a_node".into())));

        let mut edited = original.clone();
        edited.nodes[1].signal = Some("unknown".into());
        assert_eq!(inspect_validated(&edited, &registry),
            Err(Problem::MissingSignal { node: "tick".into(), signal: "unknown".into() }));

        assert!(log.lock().unwrap().is_empty());
    }

    #[test]
    fn unknown_execution_fields_cannot_be_silently_lost_by_json_editors() {
        let original = authored_document();
        let mut as_value = serde_json::to_value(&original).unwrap();
        as_value["nodes"][1]["unknown_semantic_flag"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Document>(as_value).is_err());

        let mut as_value = serde_json::to_value(&original).unwrap();
        as_value["future_runtime_kind"] = serde_json::json!("actor");
        assert!(serde_json::from_value::<Document>(as_value).is_err());

        let mut ledger = RevisionLedger::default();
        ledger.observe(&original).unwrap();
        let mut edited = original.clone();
        edited.view.get_mut("tick").unwrap().x = 99;
        assert_eq!(ledger.observe(&edited),
            Err(Problem::RevisionCollision {
                plan_id: original.plan_id, revision: original.revision,
            }), "same revision must not silently alias new content");
    }

    #[test]
    fn typed_data_edges_are_validated_but_not_pretended_executable() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let registry = registry(Arc::clone(&log));
        let mut doc = authored_document();
        doc.data_edges.push(DataEdge {
            from: "input".into(), output: "value".into(),
            to: "tick".into(), input: "input".into(),
        });
        inspect_validated(&doc, &registry).unwrap();
        assert!(matches!(bind(&doc, &registry), Err(Problem::DataFlowExecutionUnsupported)));

        let mut bad_type = doc.clone();
        bad_type.nodes[1].operation.key = "bool_sink".into();
        assert_eq!(inspect_validated(&bad_type, &registry),
            Err(Problem::TypeMismatch { from: "i64".into(), to: "bool".into() }));

        let mut bad_port = doc.clone();
        bad_port.data_edges[0].output = "no_such_output".into();
        assert_eq!(inspect_validated(&bad_port, &registry),
            Err(Problem::UnknownPort { node: "input".into(), port: "no_such_output".into() }));

        let mut duplicate = doc.clone();
        duplicate.data_edges.push(duplicate.data_edges[0].clone());
        assert_eq!(inspect_validated(&duplicate, &registry),
            Err(Problem::DuplicateDataTarget { node: "tick".into(), port: "input".into() }));

        let mut cycle = doc.clone();
        cycle.data_edges.clear();
        cycle.data_edges.push(DataEdge {
            from: "tick".into(), output: "state".into(),
            to: "input".into(), input: "missing".into(),
        });
        // A nonexistent port must fail before presenting a spurious DAG result.
        assert_eq!(inspect_validated(&cycle, &registry),
            Err(Problem::UnknownPort { node: "input".into(), port: "missing".into() }));
        assert!(log.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn declared_trigger_is_not_invoked_before_its_signal_arrives() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let registry = registry(Arc::clone(&log));
        let mut doc = authored_document();
        doc.signals.push("ready".into());
        doc.nodes[1].signal = Some("ready".into());
        inspect_validated(&doc, &registry).unwrap();
        let mut run = Run::new(bind(&doc, &registry).unwrap(), 22, 7, 2, Trace::new(true, 10));
        run.advance(usize::MAX).await;
        assert_eq!(run.state("input"), Some(Status::Completed));
        assert_eq!(run.state("tick"), Some(Status::Pending));
        assert_eq!(*log.lock().unwrap(), vec!["ingest"]);
        run.fire("ready");
        run.advance(usize::MAX).await;
        assert_eq!(run.state("snapshot"), Some(Status::Completed));
        assert_eq!(*log.lock().unwrap(), vec!["ingest", "simulate", "publish"]);
    }

    #[tokio::test]
    async fn static_validation_does_not_execute_registered_side_effects() {
        let invoked = Arc::new(AtomicUsize::new(0));
        let copy = Arc::clone(&invoked);
        let mut registry = Registry::default();
        registry.register("side_effect", RegisteredOperation {
            version: 1,
            run: operation(move || {
                let copy = Arc::clone(&copy);
                async move {
                    copy.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            }),
            access: vec![], inputs: vec![], outputs: vec![],
        });
        let mut doc = authored_document();
        doc.nodes = vec![DocumentNode {
            id: "work".into(),
            operation: BindingKey { key: "side_effect".into(), version: 1 },
            depends_on: vec![], signal: None,
        }];
        doc.view.clear();
        inspect_validated(&doc, &registry).unwrap();
        assert_eq!(invoked.load(Ordering::SeqCst), 0);
        let json = serde_json::to_string(&doc).unwrap();
        let encoded: Document = serde_json::from_str(&json).unwrap();
        let plan = bind(&encoded, &registry).unwrap();
        assert_eq!(invoked.load(Ordering::SeqCst), 0);
        Run::new(plan, 1, 1, 1, Trace::new(false, 0)).advance(1).await;
        assert_eq!(invoked.load(Ordering::SeqCst), 1);
    }
}
