//! Opt-in, process-local typed value edges using the existing session graph
//! runner. This bounded adapter does not define a wire codec, publish a GUI
//! endpoint, schedule an ECS, or replace S2 control-only execution.
//!
//! This experimental first profile supports only trusted Send + Sync owned
//! values. Domain-affine, !Send, remote and GPU resources remain owned by their
//! domain and require a separately approved reference/authorization adapter.

use crate::execution_observation::GraphRunObserver;
use crate::execution_plan::{
    BoundExecutionPlan, ExecutionPlanDraft, ExecutionPlanError, OperationKey,
    ValidatedExecutionPlan,
};
use crate::execution_runner::{
    AsyncOperation, ExecutionGraphRunner, GraphRunError, GraphRunReport, OperationOutcome,
    OperationState, async_operation,
};
use crate::runtime::RunningApp;
use std::any::{Any, TypeId};
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use thiserror::Error;

/// Application-attested, exact-version local port identity, including its
/// actual Rust type. The semantic name is not by itself a type proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalPortType {
    pub semantic: &'static str,
    pub version: u32,
    rust: TypeId,
}

impl LocalPortType {
    pub fn of<T: Any + Send + Sync>(semantic: &'static str, version: u32) -> Self {
        Self {
            semantic,
            version,
            rust: TypeId::of::<T>(),
        }
    }
}

/// One required input or output. Byte limits count payloads only; the
/// trusted Rust handler must report its true payload size on publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPort {
    pub name: &'static str,
    pub ty: LocalPortType,
    pub max_bytes: usize,
}

impl LocalPort {
    pub fn of<T: Any + Send + Sync>(
        name: &'static str,
        semantic: &'static str,
        version: u32,
        max_bytes: usize,
    ) -> Self {
        Self {
            name,
            ty: LocalPortType::of::<T>(semantic, version),
            max_bytes,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TypedOperationSpec {
    pub binding: OperationKey,
    pub inputs: Vec<LocalPort>,
    pub outputs: Vec<LocalPort>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedEdge {
    pub from_node: String,
    pub from_port: String,
    pub to_node: String,
    pub to_port: String,
}

impl TypedEdge {
    pub fn new(
        from_node: impl Into<String>,
        from_port: impl Into<String>,
        to_node: impl Into<String>,
        to_port: impl Into<String>,
    ) -> Self {
        Self {
            from_node: from_node.into(),
            from_port: from_port.into(),
            to_node: to_node.into(),
            to_port: to_port.into(),
        }
    }
}

#[derive(Debug, Error)]
pub enum TypedGraphError {
    #[error("execution plan: {0}")]
    Plan(#[from] ExecutionPlanError),
    #[error("typed operation {0:?} is not registered")]
    MissingOperation(String),
    #[error("repeated typed operation registration")]
    DuplicateRegistration,
    #[error("invalid or repeated {0} port in {1}")]
    InvalidPort(String, String),
    #[error("unknown typed {0} endpoint: {1}.{2}")]
    UnknownEndpoint(&'static str, String, String),
    #[error("typed edge mismatch from {0}.{1} to {2}.{3}")]
    IncompatibleEdge(String, String, String, String),
    #[error("input {0}.{1} has more than one producer")]
    DuplicateInput(String, String),
    #[error("required input {0}.{1} has no producer")]
    MissingInput(String, String),
    #[error("declared maximum payloads exceed the run byte budget")]
    BudgetExceeded,
}

#[derive(Clone)]
struct LocalValue {
    payload: Arc<dyn Any + Send + Sync>,
    rust: TypeId,
    bytes: usize,
}

/// Borrowed inputs are shared immutable references to the producer's exact
/// run-local artifact. A consumer cannot mutate another consumer's snapshot.
pub struct TypedInputs {
    values: BTreeMap<String, LocalValue>,
}

impl TypedInputs {
    pub fn get<T: Any + Send + Sync>(&self, port: &str) -> Option<Arc<T>> {
        Arc::clone(&self.values.get(port)?.payload)
            .downcast::<T>()
            .ok()
    }
}

/// Complete, atomic output set for one operation. Values only become visible
/// to dependent callbacks after the producer returns Completed and passes all
/// declared type and byte-budget checks.
#[derive(Default)]
pub struct TypedOutputs {
    values: BTreeMap<String, LocalValue>,
}

impl TypedOutputs {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert<T: Any + Send + Sync>(
        &mut self,
        port: impl Into<String>,
        value: T,
        bytes: usize,
    ) -> Result<(), String> {
        let port = port.into();
        if self.values.contains_key(&port) {
            return Err(format!("duplicate produced port {port}"));
        }
        self.values.insert(
            port,
            LocalValue {
                payload: Arc::new(value),
                rust: TypeId::of::<T>(),
                bytes,
            },
        );
        Ok(())
    }
}

pub enum TypedOperationOutcome {
    Completed(TypedOutputs),
    Failed(String),
    Cancelled,
    OutcomeUnknown,
}

pub type TypedOperationFuture =
    Pin<Box<dyn Future<Output = TypedOperationOutcome> + Send + 'static>>;
pub type AsyncTypedOperation =
    dyn Fn(TypedInputs) -> TypedOperationFuture + Send + Sync + 'static;

pub fn typed_operation<F, Fut>(operation: F) -> Arc<AsyncTypedOperation>
where
    F: Fn(TypedInputs) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = TypedOperationOutcome> + Send + 'static,
{
    Arc::new(move |inputs| Box::pin(operation(inputs)))
}

struct RegisteredTypedOperation {
    descriptor: TypedOperationSpec,
    handler: Arc<AsyncTypedOperation>,
}

#[derive(Default)]
pub struct TypedOperationRegistry {
    operations: BTreeMap<OperationKey, RegisteredTypedOperation>,
}

impl TypedOperationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        descriptor: TypedOperationSpec,
        handler: Arc<AsyncTypedOperation>,
    ) -> Result<(), TypedGraphError> {
        validate_ports(&descriptor)?;
        if self.operations.contains_key(&descriptor.binding) {
            return Err(TypedGraphError::DuplicateRegistration);
        }
        self.operations.insert(
            descriptor.binding.clone(),
            RegisteredTypedOperation {
                descriptor,
                handler,
            },
        );
        Ok(())
    }
}

fn valid_port(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 256
        && !name.chars().any(|ch| ch.is_whitespace() || ch.is_control())
}

fn validate_ports(desc: &TypedOperationSpec) -> Result<(), TypedGraphError> {
    for (direction, ports) in [("input", &desc.inputs), ("output", &desc.outputs)] {
        let mut names = BTreeSet::new();
        for port in ports {
            if !valid_port(port.name)
                || !valid_port(port.ty.semantic)
                || port.ty.version == 0
                || !names.insert(port.name)
            {
                return Err(TypedGraphError::InvalidPort(
                    direction.to_owned(),
                    desc.binding.name().to_owned(),
                ));
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct ArtifactStore {
    values: BTreeMap<(String, String), LocalValue>,
    used_bytes: usize,
}

pub struct TypedExecutionPlan {
    base: ValidatedExecutionPlan,
    edges: Vec<TypedEdge>,
}

impl TypedExecutionPlan {
    pub fn new(base: ValidatedExecutionPlan) -> Self {
        Self {
            base,
            edges: Vec::new(),
        }
    }

    pub fn connect(mut self, edge: TypedEdge) -> Self {
        self.edges.push(edge);
        self
    }

    /// Binds exact application-authored ports and Rust handler types before
    /// constructing the *same* existing Riesel runner's control/data ordering.
    /// This object cannot be serialized or silently executed as an S2-only
    /// document; there is no separate graph scheduler.
    pub fn prepare(
        &self,
        registry: &TypedOperationRegistry,
        max_run_bytes: usize,
    ) -> Result<PreparedTypedExecution, TypedGraphError> {
        let description = self.base.description();
        let nodes = description.operations();
        let by_id: BTreeMap<_, _> = nodes.iter().map(|n| (n.id(), n)).collect();
        let mut specs = BTreeMap::new();
        let mut static_bytes = 0usize;
        for node in nodes {
            let Some(registered) = registry.operations.get(node.binding()) else {
                return Err(TypedGraphError::MissingOperation(node.id().to_owned()));
            };
            for port in &registered.descriptor.outputs {
                static_bytes = static_bytes
                    .checked_add(port.max_bytes)
                    .ok_or(TypedGraphError::BudgetExceeded)?;
            }
            specs.insert(node.id(), &registered.descriptor);
        }
        if static_bytes > max_run_bytes {
            return Err(TypedGraphError::BudgetExceeded);
        }

        let mut input_edges = BTreeMap::<(String, String), TypedEdge>::new();
        for edge in &self.edges {
            let Some(source) = by_id.get(edge.from_node.as_str()) else {
                return Err(TypedGraphError::UnknownEndpoint(
                    "source",
                    edge.from_node.clone(),
                    edge.from_port.clone(),
                ));
            };
            let Some(target) = by_id.get(edge.to_node.as_str()) else {
                return Err(TypedGraphError::UnknownEndpoint(
                    "target",
                    edge.to_node.clone(),
                    edge.to_port.clone(),
                ));
            };
            let source_desc = specs[source.id()];
            let target_desc = specs[target.id()];
            let Some(output) = source_desc
                .outputs
                .iter()
                .find(|port| port.name == edge.from_port)
            else {
                return Err(TypedGraphError::UnknownEndpoint(
                    "output",
                    edge.from_node.clone(),
                    edge.from_port.clone(),
                ));
            };
            let Some(input) = target_desc
                .inputs
                .iter()
                .find(|port| port.name == edge.to_port)
            else {
                return Err(TypedGraphError::UnknownEndpoint(
                    "input",
                    edge.to_node.clone(),
                    edge.to_port.clone(),
                ));
            };
            if output.ty != input.ty {
                return Err(TypedGraphError::IncompatibleEdge(
                    edge.from_node.clone(),
                    edge.from_port.clone(),
                    edge.to_node.clone(),
                    edge.to_port.clone(),
                ));
            }
            if input_edges
                .insert(
                    (edge.to_node.clone(), edge.to_port.clone()),
                    edge.clone(),
                )
                .is_some()
            {
                return Err(TypedGraphError::DuplicateInput(
                    edge.to_node.clone(),
                    edge.to_port.clone(),
                ));
            }
        }

        for node in nodes {
            for port in &specs[node.id()].inputs {
                if !input_edges.contains_key(&(node.id().to_owned(), port.name.to_owned())) {
                    return Err(TypedGraphError::MissingInput(
                        node.id().to_owned(),
                        port.name.to_owned(),
                    ));
                }
            }
        }

        // The union of ordinary control prerequisites and required data edges
        // is validated by the authoritative execution-plan cycle detector.
        let mut augmented = ExecutionPlanDraft::new(description.id(), description.revision());
        for node in nodes {
            let mut updated = node.clone();
            for edge in self.edges.iter().filter(|edge| edge.to_node == node.id()) {
                if !updated
                    .dependencies()
                    .iter()
                    .any(|prerequisite| prerequisite == &edge.from_node)
                {
                    updated = updated.after(edge.from_node.clone());
                }
            }
            augmented = augmented.operation(updated);
        }
        let augmented = augmented.validate()?;

        let store = Arc::new(Mutex::new(ArtifactStore::default()));
        let mut handlers = BTreeMap::<String, Arc<AsyncOperation>>::new();
        for node in nodes {
            let registered = registry.operations.get(node.binding()).expect("checked binding");
            let descriptor = registered.descriptor.clone();
            let handler = Arc::clone(&registered.handler);
            let operation_id = node.id().to_owned();
            let incoming: Vec<_> = self
                .edges
                .iter()
                .filter(|edge| edge.to_node == node.id())
                .cloned()
                .collect();
            let store = Arc::clone(&store);
            handlers.insert(
                node.id().to_owned(),
                async_operation(move || {
                    let descriptor = descriptor.clone();
                    let handler = Arc::clone(&handler);
                    let operation_id = operation_id.clone();
                    let incoming = incoming.clone();
                    let store = Arc::clone(&store);
                    async move {
                        let mut values = BTreeMap::new();
                        {
                            let locked = store.lock().unwrap_or_else(PoisonError::into_inner);
                            for edge in &incoming {
                                let key = (edge.from_node.clone(), edge.from_port.clone());
                                let Some(value) = locked.values.get(&key) else {
                                    return OperationOutcome::Failed(format!(
                                        "producer output {}.{} was not committed",
                                        edge.from_node, edge.from_port
                                    ));
                                };
                                values.insert(edge.to_port.clone(), value.clone());
                            }
                        }
                        let outcome = handler(TypedInputs { values }).await;
                        match outcome {
                            TypedOperationOutcome::Completed(outputs) => {
                                if outputs.values.len() != descriptor.outputs.len() {
                                    return OperationOutcome::Failed(format!(
                                        "operation {operation_id}: incomplete or extra outputs"
                                    ));
                                }
                                let mut additional = 0usize;
                                for port in &descriptor.outputs {
                                    let Some(value) = outputs.values.get(port.name) else {
                                        return OperationOutcome::Failed(format!(
                                            "operation {operation_id}: missing output {}",
                                            port.name
                                        ));
                                    };
                                    if value.rust != port.ty.rust || value.bytes > port.max_bytes {
                                        return OperationOutcome::Failed(format!(
                                            "operation {operation_id}: incompatible or oversized output {}",
                                            port.name
                                        ));
                                    }
                                    let Some(next) = additional.checked_add(value.bytes) else {
                                        return OperationOutcome::Failed(
                                            "artifact byte count overflow".into(),
                                        );
                                    };
                                    additional = next;
                                }
                                let mut locked = store.lock().unwrap_or_else(PoisonError::into_inner);
                                let Some(total) = locked.used_bytes.checked_add(additional) else {
                                    return OperationOutcome::Failed(
                                        "run artifact byte count overflow".into(),
                                    );
                                };
                                if total > max_run_bytes {
                                    return OperationOutcome::Failed(
                                        "run artifact byte budget exceeded".into(),
                                    );
                                }
                                for (port, value) in outputs.values {
                                    locked.values.insert((operation_id.clone(), port), value);
                                }
                                locked.used_bytes = total;
                                OperationOutcome::Completed
                            }
                            TypedOperationOutcome::Failed(reason) => OperationOutcome::Failed(reason),
                            TypedOperationOutcome::Cancelled => OperationOutcome::Cancelled,
                            TypedOperationOutcome::OutcomeUnknown => OperationOutcome::OutcomeUnknown,
                        }
                    }
                }),
            );
        }

        Ok(PreparedTypedExecution {
            bound: augmented.bind_typed_node_handlers(handlers)?,
            store,
        })
    }
}

/// A single-use, non-serializable prepared graph. The SDK consumes it when
/// constructing the ordinary Riesel session runner.
pub struct PreparedTypedExecution {
    bound: BoundExecutionPlan<AsyncOperation>,
    store: Arc<Mutex<ArtifactStore>>,
}

pub struct TypedExecutionGraphRunner {
    runner: ExecutionGraphRunner,
    store: Arc<Mutex<ArtifactStore>>,
}

impl TypedExecutionGraphRunner {
    pub fn observer(&self) -> GraphRunObserver {
        self.runner.observer()
    }

    pub async fn execute(self) -> TypedGraphRunReport {
        let report = self.runner.execute().await;
        TypedGraphRunReport {
            report,
            store: self.store,
        }
    }
}

/// Exact-run result ownership. Failed, cancelled, unknown and blocked
/// producers never expose values, even if they attempted partial writes.
pub struct TypedGraphRunReport {
    pub report: GraphRunReport,
    store: Arc<Mutex<ArtifactStore>>,
}

impl TypedGraphRunReport {
    pub fn get<T: Any + Send + Sync>(&self, node: &str, port: &str) -> Option<Arc<T>> {
        if !matches!(self.report.operations.get(node)?, OperationState::Completed) {
            return None;
        }
        let locked = self.store.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(
            &locked
                .values
                .get(&(node.to_owned(), port.to_owned()))?
                .payload,
        )
        .downcast::<T>()
        .ok()
    }

    pub fn used_bytes(&self) -> usize {
        self.store
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .used_bytes
    }
}

impl RunningApp {
    pub fn typed_execution_graph(
        &self,
        prepared: PreparedTypedExecution,
        max_in_flight: NonZeroUsize,
    ) -> Result<TypedExecutionGraphRunner, GraphRunError> {
        let runner = self.execution_graph(prepared.bound, max_in_flight)?;
        Ok(TypedExecutionGraphRunner {
            runner,
            store: prepared.store,
        })
    }
}
