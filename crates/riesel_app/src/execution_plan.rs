//! Side-effect-free, versioned application operation-plan validation.
//!
//! This describes an application operation graph, NOT the provider startup DAG
//! or DeploymentPlan. The executable registry retains ordinary Rust handlers;
//! validation and binding never call them. Scheduling, session ownership,
//! runtime effects, authentication and a persisted plan format are separate.

use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use thiserror::Error;

/// A conservative bounded, non-whitespace logical name. This is not a URI or
/// a cross-process identity credential, and does not establish app provenance.
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && !name.chars().any(|ch| ch.is_control() || ch.is_whitespace())
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ExecutionPlanError {
    #[error("invalid execution plan id: {id:?}")]
    InvalidPlanId { id: String },
    #[error("execution plan revision must be nonzero")]
    InvalidRevision,
    #[error("execution plan must contain at least one operation")]
    EmptyPlan,
    #[error("invalid operation id: {id:?}")]
    InvalidNodeId { id: String },
    #[error("duplicate operation id: {id}")]
    DuplicateNode { id: String },
    #[error("operation {node} cannot depend on itself")]
    SelfDependency { node: String },
    #[error("operation {node} references unknown dependency {dependency}")]
    UnknownDependency { node: String, dependency: String },
    #[error("operation {node} repeats dependency {dependency}")]
    DuplicateDependency { node: String, dependency: String },
    #[error("cyclic operation dependencies: {nodes:?}")]
    Cycle { nodes: Vec<String> },
    #[error("invalid signal for operation {node}: {signal:?}")]
    InvalidSignal { node: String, signal: String },
    #[error("invalid resource for operation {node}: {resource:?}")]
    InvalidResource { node: String, resource: String },
    #[error("operation {node} repeats resource {resource}")]
    DuplicateResource { node: String, resource: String },
    #[error("invalid registered operation key {name:?} at version {version}")]
    InvalidOperationKey { name: String, version: u32 },
    #[error("operation {name} version {version} is already registered")]
    DuplicateRegistration { name: String, version: u32 },
    #[error("no registered Rust operation for node {node}: {name} version {version}")]
    MissingBinding {
        node: String,
        name: String,
        version: u32,
    },
    #[error("plan {id} semantic revision {revision} is already published")]
    DuplicatePlanRevision { id: String, revision: u64 },
}

/// An exact-version symbolic reference to registered Rust executable behavior.
///
/// This is deliberately NOT serialized Rust closure data or a transport API.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct OperationKey {
    name: String,
    version: u32,
}

impl OperationKey {
    pub fn new(name: impl Into<String>, version: u32) -> Result<Self, ExecutionPlanError> {
        let name = name.into();
        if version == 0 || !valid_name(&name) {
            return Err(ExecutionPlanError::InvalidOperationKey { name, version });
        }
        Ok(Self { name, version })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> u32 {
        self.version
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "signal", rename_all = "snake_case")]
pub enum OperationTrigger {
    /// Eligible once its declared dependencies are completed.
    Immediate,
    /// Requires an explicit matching signal in addition to dependencies.
    Signal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessMode {
    Read,
    Write,
}

/// Only declarative scheduling metadata; does not prove Rust borrow safety.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceAccess {
    resource: String,
    mode: AccessMode,
}

impl ResourceAccess {
    pub fn new(resource: impl Into<String>, mode: AccessMode) -> Self {
        Self {
            resource: resource.into(),
            mode,
        }
    }

    pub fn resource(&self) -> &str {
        &self.resource
    }

    pub fn mode(&self) -> AccessMode {
        self.mode
    }
}

/// A draft operation; dependencies are application-operation IDs, not
/// capability requirements, processes, provider instances or ECS systems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OperationNode {
    id: String,
    binding: OperationKey,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    depends_on: Vec<String>,
    trigger: OperationTrigger,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    access: Vec<ResourceAccess>,
}

impl OperationNode {
    pub fn new(id: impl Into<String>, binding: OperationKey) -> Self {
        Self {
            id: id.into(),
            binding,
            depends_on: Vec::new(),
            trigger: OperationTrigger::Immediate,
            access: Vec::new(),
        }
    }

    pub fn after(mut self, prerequisite: impl Into<String>) -> Self {
        self.depends_on.push(prerequisite.into());
        self
    }

    pub fn on_signal(mut self, name: impl Into<String>) -> Self {
        self.trigger = OperationTrigger::Signal(name.into());
        self
    }

    pub fn access(mut self, resource: impl Into<String>, mode: AccessMode) -> Self {
        self.access.push(ResourceAccess::new(resource, mode));
        self
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn binding(&self) -> &OperationKey {
        &self.binding
    }

    pub fn dependencies(&self) -> &[String] {
        &self.depends_on
    }

    pub fn trigger(&self) -> &OperationTrigger {
        &self.trigger
    }

    pub fn resources(&self) -> &[ResourceAccess] {
        &self.access
    }
}

/// A machine-readable, **read-only** projection of an application-owned plan.
/// Serialize only for inspection; no stable document ABI, parsing/import,
// executable manifest or visual-editor round-trip is accepted at this stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExecutionPlanDescription {
    id: String,
    revision: u64,
    operations: Vec<OperationNode>,
}

impl ExecutionPlanDescription {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn operations(&self) -> &[OperationNode] {
        &self.operations
    }
}

/// A mutable authoring draft. Validation consumes it, isolating the validated
/// run plan from any subsequent edits to the author's draft/registered code.
pub struct ExecutionPlanDraft {
    description: ExecutionPlanDescription,
}

impl ExecutionPlanDraft {
    pub fn new(id: impl Into<String>, revision: u64) -> Self {
        Self {
            description: ExecutionPlanDescription {
                id: id.into(),
                revision,
                operations: Vec::new(),
            },
        }
    }

    pub fn operation(mut self, node: OperationNode) -> Self {
        self.description.operations.push(node);
        self
    }

    /// Pure validation: never invokes handlers, constructs providers or starts
    /// an application. Order is stable for independent nodes by declaration.
    pub fn validate(self) -> Result<ValidatedExecutionPlan, ExecutionPlanError> {
        let description = self.description;
        if !valid_name(&description.id) {
            return Err(ExecutionPlanError::InvalidPlanId { id: description.id });
        }
        if description.revision == 0 {
            return Err(ExecutionPlanError::InvalidRevision);
        }
        if description.operations.is_empty() {
            return Err(ExecutionPlanError::EmptyPlan);
        }

        let mut ids = BTreeMap::new();
        for (index, operation) in description.operations.iter().enumerate() {
            if !valid_name(&operation.id) {
                return Err(ExecutionPlanError::InvalidNodeId {
                    id: operation.id.clone(),
                });
            }
            if ids.insert(operation.id.as_str(), index).is_some() {
                return Err(ExecutionPlanError::DuplicateNode {
                    id: operation.id.clone(),
                });
            }
        }

        let mut indegrees = vec![0usize; description.operations.len()];
        let mut dependents = vec![Vec::<usize>::new(); description.operations.len()];
        for (index, operation) in description.operations.iter().enumerate() {
            if let OperationTrigger::Signal(signal) = &operation.trigger {
                if !valid_name(signal) {
                    return Err(ExecutionPlanError::InvalidSignal {
                        node: operation.id.clone(),
                        signal: signal.clone(),
                    });
                }
            }
            let mut resources = BTreeSet::new();
            for access in &operation.access {
                if !valid_name(&access.resource) {
                    return Err(ExecutionPlanError::InvalidResource {
                        node: operation.id.clone(),
                        resource: access.resource.clone(),
                    });
                }
                if !resources.insert(access.resource.as_str()) {
                    return Err(ExecutionPlanError::DuplicateResource {
                        node: operation.id.clone(),
                        resource: access.resource.clone(),
                    });
                }
            }
            let mut visited = BTreeSet::new();
            for prerequisite in &operation.depends_on {
                if prerequisite == &operation.id {
                    return Err(ExecutionPlanError::SelfDependency {
                        node: operation.id.clone(),
                    });
                }
                let Some(&dep) = ids.get(prerequisite.as_str()) else {
                    return Err(ExecutionPlanError::UnknownDependency {
                        node: operation.id.clone(),
                        dependency: prerequisite.clone(),
                    });
                };
                if !visited.insert(prerequisite) {
                    return Err(ExecutionPlanError::DuplicateDependency {
                        node: operation.id.clone(),
                        dependency: prerequisite.clone(),
                    });
                }
                indegrees[index] += 1;
                dependents[dep].push(index);
            }
        }

        let mut eligible: BTreeSet<usize> = indegrees
            .iter()
            .enumerate()
            .filter_map(|(i, &n)| (n == 0).then_some(i))
            .collect();
        let mut order = Vec::with_capacity(description.operations.len());
        while let Some(&next) = eligible.first() {
            eligible.remove(&next);
            order.push(next);
            for &dependent in &dependents[next] {
                indegrees[dependent] -= 1;
                if indegrees[dependent] == 0 {
                    eligible.insert(dependent);
                }
            }
        }
        if order.len() != description.operations.len() {
            return Err(ExecutionPlanError::Cycle {
                nodes: description
                    .operations
                    .iter()
                    .enumerate()
                    .filter_map(|(i, node)| (indegrees[i] != 0).then(|| node.id.clone()))
                    .collect(),
            });
        }

        Ok(ValidatedExecutionPlan {
            description: Arc::new(description),
            order,
        })
    }
}

/// Immutable semantic revision, validated for identity, dependency shape and
/// declared constraints. Binding is checked separately against actual Rust
/// handlers; no operation can run simply by deserializing metadata.
#[derive(Debug, Clone)]
pub struct ValidatedExecutionPlan {
    description: Arc<ExecutionPlanDescription>,
    order: Vec<usize>,
}

impl ValidatedExecutionPlan {
    pub fn description(&self) -> &ExecutionPlanDescription {
        &self.description
    }

    /// Deterministic topological traversal, not a runtime scheduling policy.
    pub fn dependency_order(&self) -> impl Iterator<Item = &OperationNode> {
        self.order.iter().map(|&i| &self.description.operations[i])
    }

    /// Requires a matching exact-version handler for every graph node. This is
    /// a pure lookup; handlers are not invoked, and providers are not started.
    pub fn bind<H: ?Sized>(
        &self,
        registry: &OperationRegistry<H>,
    ) -> Result<BoundExecutionPlan<H>, ExecutionPlanError> {
        let mut handlers = BTreeMap::new();
        for node in &self.description.operations {
            let Some(handler) = registry.get(&node.binding) else {
                return Err(ExecutionPlanError::MissingBinding {
                    node: node.id.clone(),
                    name: node.binding.name.clone(),
                    version: node.binding.version,
                });
            };
            handlers.insert(node.id.clone(), Arc::clone(handler));
        }
        Ok(BoundExecutionPlan {
            plan: self.clone(),
            handlers,
        })
    }
}

/// Application-owned catalog of *real* Rust handler objects, type-erased by the
/// application if necessary. This does not choose providers or define a
/// compulsory async/native execution signature before runner integration.
pub struct OperationRegistry<H: ?Sized> {
    handlers: BTreeMap<OperationKey, Arc<H>>,
}

impl<H: ?Sized> Default for OperationRegistry<H> {
    fn default() -> Self {
        Self {
            handlers: BTreeMap::new(),
        }
    }
}

impl<H: ?Sized> OperationRegistry<H> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        key: OperationKey,
        handler: Arc<H>,
    ) -> Result<(), ExecutionPlanError> {
        if self.handlers.contains_key(&key) {
            return Err(ExecutionPlanError::DuplicateRegistration {
                name: key.name,
                version: key.version,
            });
        }
        self.handlers.insert(key, handler);
        Ok(())
    }

    pub fn get(&self, key: &OperationKey) -> Option<&Arc<H>> {
        self.handlers.get(key)
    }
}

/// An immutable validated plan paired with its exact Rust code handlers.
/// The upcoming session-owned graph runner is responsible for invoking them.
pub struct BoundExecutionPlan<H: ?Sized> {
    plan: ValidatedExecutionPlan,
    handlers: BTreeMap<String, Arc<H>>,
}

impl<H: ?Sized> std::fmt::Debug for BoundExecutionPlan<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BoundExecutionPlan")
            .field("plan", &self.plan.description)
            .field("bound_nodes", &self.handlers.keys().collect::<Vec<_>>())
            .finish()
    }
}

impl<H: ?Sized> BoundExecutionPlan<H> {
    pub fn plan(&self) -> &ValidatedExecutionPlan {
        &self.plan
    }

    pub fn handler(&self, operation_id: &str) -> Option<&Arc<H>> {
        self.handlers.get(operation_id)
    }
}

/// Within a catalog lifetime, a logical plan/revision pair has one immutable
/// meaning. This is NOT a durable, shared or authenticated plan registry.
#[derive(Default)]
pub struct PlanRevisionCatalog {
    revisions: BTreeMap<(String, u64), Arc<ValidatedExecutionPlan>>,
}

impl PlanRevisionCatalog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn publish(
        &mut self,
        plan: ValidatedExecutionPlan,
    ) -> Result<Arc<ValidatedExecutionPlan>, ExecutionPlanError> {
        let key = (plan.description.id.clone(), plan.description.revision);
        if self.revisions.contains_key(&key) {
            return Err(ExecutionPlanError::DuplicatePlanRevision {
                id: key.0,
                revision: key.1,
            });
        }
        let plan = Arc::new(plan);
        self.revisions.insert(key, Arc::clone(&plan));
        Ok(plan)
    }

    pub fn get(&self, id: &str, revision: u64) -> Option<&Arc<ValidatedExecutionPlan>> {
        self.revisions.get(&(id.to_owned(), revision))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn key(name: &str, version: u32) -> OperationKey {
        OperationKey::new(name, version).unwrap()
    }

    fn make_plan() -> ValidatedExecutionPlan {
        ExecutionPlanDraft::new("example.process", 4)
            .operation(
                OperationNode::new("join", key("join", 1))
                    .after("left")
                    .after("right"),
            )
            .operation(OperationNode::new("right", key("handler", 2)).after("input"))
            .operation(OperationNode::new("input", key("handler", 2)).on_signal("message"))
            .operation(OperationNode::new("left", key("handler", 2)).after("input"))
            .validate()
            .unwrap()
    }

    #[test]
    fn validates_and_inspects_a_stably_ordered_headless_graph() {
        let plan = make_plan();
        assert_eq!(plan.description().id(), "example.process");
        assert_eq!(plan.description().revision(), 4);
        let ordered: Vec<_> = plan.dependency_order().map(OperationNode::id).collect();
        assert_eq!(ordered, vec!["input", "right", "left", "join"]);
        assert!(matches!(plan.description().operations()[2].trigger(),
            OperationTrigger::Signal(s) if s == "message"));
        let value = serde_json::to_value(plan.description()).unwrap();
        assert_eq!(value["id"], "example.process");
        assert_eq!(value["revision"], 4);
        assert_eq!(value["operations"][0]["binding"]["name"], "join");
    }

    #[test]
    fn rejects_empty_and_invalid_logical_identity() {
        assert!(matches!(
            ExecutionPlanDraft::new("example", 1).validate(),
            Err(ExecutionPlanError::EmptyPlan)
        ));
        assert!(matches!(
            ExecutionPlanDraft::new("bad name", 1)
                .operation(OperationNode::new("one", key("op", 1)))
                .validate(),
            Err(ExecutionPlanError::InvalidPlanId { .. })
        ));
        assert!(matches!(
            ExecutionPlanDraft::new("example", 0)
                .operation(OperationNode::new("one", key("op", 1)))
                .validate(),
            Err(ExecutionPlanError::InvalidRevision)
        ));
        assert!(OperationKey::new("bad name", 1).is_err());
        assert!(OperationKey::new("op", 0).is_err());
    }

    #[test]
    fn rejects_duplicate_unknown_self_and_cyclic_dependencies() {
        let op = key("op", 1);
        let draft = ExecutionPlanDraft::new("example", 1)
            .operation(OperationNode::new("x", op.clone()))
            .operation(OperationNode::new("x", op.clone()));
        assert!(matches!(
            draft.validate(),
            Err(ExecutionPlanError::DuplicateNode { .. })
        ));

        let draft = ExecutionPlanDraft::new("example", 1)
            .operation(OperationNode::new("x", op.clone()).after("missing"));
        assert!(matches!(
            draft.validate(),
            Err(ExecutionPlanError::UnknownDependency { .. })
        ));

        let draft = ExecutionPlanDraft::new("example", 1)
            .operation(OperationNode::new("x", op.clone()).after("x"));
        assert!(matches!(
            draft.validate(),
            Err(ExecutionPlanError::SelfDependency { .. })
        ));

        let draft = ExecutionPlanDraft::new("example", 1)
            .operation(OperationNode::new("x", op.clone()).after("y").after("y"))
            .operation(OperationNode::new("y", op.clone()));
        assert!(matches!(
            draft.validate(),
            Err(ExecutionPlanError::DuplicateDependency { .. })
        ));

        let draft = ExecutionPlanDraft::new("example", 1)
            .operation(OperationNode::new("x", op.clone()).after("y"))
            .operation(OperationNode::new("y", op).after("x"));
        assert!(matches!(
            draft.validate(),
            Err(ExecutionPlanError::Cycle { .. })
        ));
    }

    #[test]
    fn checks_signals_and_resource_declarations_without_claiming_borrow_safety() {
        let op = key("op", 1);
        let bad_signal = ExecutionPlanDraft::new("example", 1)
            .operation(OperationNode::new("x", op.clone()).on_signal(""));
        assert!(matches!(
            bad_signal.validate(),
            Err(ExecutionPlanError::InvalidSignal { .. })
        ));

        let invalid_resource = ExecutionPlanDraft::new("example", 1).operation(
            OperationNode::new("x", op.clone()).access("bad resource", AccessMode::Read),
        );
        assert!(matches!(
            invalid_resource.validate(),
            Err(ExecutionPlanError::InvalidResource { .. })
        ));

        let duplicate = ExecutionPlanDraft::new("example", 1).operation(
            OperationNode::new("x", op.clone())
                .access("world", AccessMode::Read)
                .access("world", AccessMode::Write),
        );
        assert!(matches!(
            duplicate.validate(),
            Err(ExecutionPlanError::DuplicateResource { .. })
        ));

        let valid = ExecutionPlanDraft::new("example", 1)
            .operation(OperationNode::new("x", op).access("world", AccessMode::Write))
            .validate()
            .unwrap();
        assert_eq!(
            valid.description().operations()[0].resources()[0].mode(),
            AccessMode::Write
        );
    }

    #[test]
    fn requires_exact_versioned_registered_rust_handlers_without_invoking_them() {
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_for_handler = Arc::clone(&counter);
        let mut registry = OperationRegistry::<dyn Fn() + Send + Sync>::new();
        registry
            .register(
                key("handler", 2),
                Arc::new(move || {
                    counter_for_handler.fetch_add(1, Ordering::SeqCst);
                }),
            )
            .unwrap();
        registry.register(key("join", 1), Arc::new(|| {})).unwrap();

        let plan = make_plan();
        assert_eq!(counter.load(Ordering::SeqCst), 0);
        let bound = plan.bind(&registry).unwrap();
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "plan validation and binding must not execute application code"
        );
        assert!(bound.handler("not_a_node").is_none());
        assert!(bound.handler("input").is_some());
        assert_eq!(bound.plan().description().revision(), 4);

        let mismatched = ExecutionPlanDraft::new("example", 5)
            .operation(OperationNode::new("x", key("handler", 3)))
            .validate()
            .unwrap();
        assert!(matches!(
            mismatched.bind(&registry),
            Err(ExecutionPlanError::MissingBinding { version: 3, .. })
        ));
        assert!(matches!(
            registry.register(key("handler", 2), Arc::new(|| {})),
            Err(ExecutionPlanError::DuplicateRegistration { .. })
        ));
    }

    #[test]
    fn immutable_revisions_cannot_be_silently_redefined_or_reused() {
        let mut catalog = PlanRevisionCatalog::new();
        let first = catalog.publish(make_plan()).unwrap();
        let duplicate = make_plan();
        assert!(matches!(
            catalog.publish(duplicate),
            Err(ExecutionPlanError::DuplicatePlanRevision { revision: 4, .. })
        ));

        let edited = ExecutionPlanDraft::new("example.process", 5)
            .operation(OperationNode::new("different", key("other", 7)))
            .validate()
            .unwrap();
        let changed = catalog.publish(edited).unwrap();
        assert_eq!(first.description().revision(), 4);
        assert_eq!(first.description().operations().len(), 4);
        assert_eq!(changed.description().revision(), 5);
        assert_eq!(
            catalog
                .get("example.process", 4)
                .unwrap()
                .description()
                .revision(),
            4
        );
        assert!(catalog.get("example.process", 6).is_none());
    }
}
