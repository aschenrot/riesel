//! A versioned, app-selected read-only facade over the existing graph
//! observer. No scheduler, provider resolver, GUI renderer, or graph-control
//! handle is created. Publication is a separate deliberate T1 application act.
use crate::{
    GraphRunChange, GraphRunIdentity, GraphRunObserver, GraphRunPhase, OperationState,
    ValidatedExecutionPlan, capability, RieselType,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

const MAX_GRAPH_READ_NODES: usize = 256;

/// An exact run address. Correlation only: not a credential or client grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct GraphReadIdentity {
    pub session_generation: String,
    pub run_sequence: u64,
    pub plan_id: String,
    pub revision: u64,
}

impl From<&GraphRunIdentity> for GraphReadIdentity {
    fn from(run: &GraphRunIdentity) -> Self {
        Self {
            session_generation: run.session_generation().to_string(),
            run_sequence: run.run_sequence(),
            plan_id: run.plan_id().to_owned(),
            revision: run.revision(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct GraphReadPrefix {
    pub stream: String,
    pub cumulative: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct GraphReadNode {
    pub id: String,
    pub binding: String,
    pub binding_version: u64,
    pub depends_on: Vec<String>,
    pub trigger: String,
    pub state: String,
    pub failure: Option<String>,
    pub attempt: u64,
    pub confirmed_prefixes: Vec<GraphReadPrefix>,
}

/// A bounded full authoritative state, not a replayed or invented trace.
/// Presentation layout revision is not fabricated by this graph-only view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct GraphReadSnapshot {
    pub schema_version: u64,
    pub identity: GraphReadIdentity,
    pub phase: String,
    pub latest_sequence: u64,
    pub session_expired: bool,
    pub nodes: Vec<GraphReadNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct GraphReadEvent {
    pub sequence: u64,
    pub kind: String,
    pub operation_id: Option<String>,
    pub attempt: Option<u64>,
    pub state: Option<String>,
    pub failure: Option<String>,
    pub stream: Option<String>,
    pub cumulative: Option<u64>,
    pub control_target: Option<String>,
    pub control_mode: Option<String>,
    pub phase: Option<String>,
}

/// Truncation means retained events are insufficient: events is empty and
/// the client must request a whole authoritative snapshot, not guess gaps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct GraphReadChanges {
    pub schema_version: u64,
    pub identity: GraphReadIdentity,
    pub latest_sequence: u64,
    pub truncated: bool,
    pub events: Vec<GraphReadEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType, Error)]
pub enum GraphReadError {
    #[error("the plan does not belong to this authoritative graph run")]
    SourceMismatch,
    #[error("graph exceeds the bounded read projection")]
    TooManyNodes,
    #[error("run identity changed; fetch a new authoritative snapshot")]
    StaleRun,
    #[error("cursor is ahead of the authoritative graph sequence")]
    FutureCursor,
}

/// A versioned application-facing capability. An app must explicitly publish
/// only these Query operations under T1, and only for non-sensitive runs.
#[capability(id = "riesel.execution.graph_read", version = 1)]
pub trait ExecutionGraphRead {
    /// Read the exact current plan graph and authoritative operation states.
    #[query]
    async fn snapshot(&self) -> Result<GraphReadSnapshot, GraphReadError>;

    /// Read bounded changes for the exact run, requiring resnapshot on loss.
    #[query]
    async fn changes(
        &self,
        session_generation: String,
        run_sequence: u64,
        cursor: u64,
    ) -> Result<GraphReadChanges, GraphReadError>;
}

#[derive(Clone)]
struct NodeDescription {
    id: String,
    binding: String,
    binding_version: u64,
    depends_on: Vec<String>,
    trigger: String,
}

/// A facade over one existing immutable validated plan and its own observer.
pub struct ExecutionGraphReadFacade {
    observer: GraphRunObserver,
    nodes: Vec<NodeDescription>,
}

impl ExecutionGraphReadFacade {
    pub fn new(
        plan: &ValidatedExecutionPlan,
        observer: GraphRunObserver,
    ) -> Result<Self, GraphReadError> {
        let current = observer.snapshot();
        if current.identity.plan_id() != plan.description().id()
            || current.identity.revision() != plan.description().revision()
        {
            return Err(GraphReadError::SourceMismatch);
        }
        if plan.description().operations().len() > MAX_GRAPH_READ_NODES {
            return Err(GraphReadError::TooManyNodes);
        }
        let nodes = plan
            .description()
            .operations()
            .iter()
            .map(|node| NodeDescription {
                id: node.id().to_owned(),
                binding: node.binding().name().to_owned(),
                binding_version: u64::from(node.binding().version()),
                depends_on: node.dependencies().to_vec(),
                trigger: match node.trigger() {
                    crate::OperationTrigger::Immediate => "immediate".to_owned(),
                    crate::OperationTrigger::Signal(signal) => format!("signal:{signal}"),
                },
            })
            .collect();
        Ok(Self { observer, nodes })
    }
}

fn phase_name(phase: GraphRunPhase) -> String {
    match phase {
        GraphRunPhase::Created => "created",
        GraphRunPhase::Running => "running",
        GraphRunPhase::Finished => "finished",
        GraphRunPhase::Abandoned => "abandoned",
    }
    .into()
}

fn state_name(state: &OperationState) -> (String, Option<String>) {
    match state {
        OperationState::Pending => ("pending", None),
        OperationState::Running => ("running", None),
        OperationState::Completed => ("completed", None),
        OperationState::Failed(error) => ("failed", Some(error.clone())),
        OperationState::Cancelled => ("cancelled", None),
        OperationState::OutcomeUnknown => ("outcome_unknown", None),
        OperationState::Blocked => ("blocked", None),
    }
    .map_state()
}

trait StatePair {
    fn map_state(self) -> (String, Option<String>);
}
impl StatePair for (&str, Option<String>) {
    fn map_state(self) -> (String, Option<String>) {
        (self.0.into(), self.1)
    }
}

impl ExecutionGraphRead for ExecutionGraphReadFacade {
    async fn snapshot(&self) -> Result<GraphReadSnapshot, GraphReadError> {
        let current = self.observer.snapshot();
        let by_id = &current.operations;
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for spec in &self.nodes {
            let Some(operation) = by_id.get(&spec.id) else {
                return Err(GraphReadError::SourceMismatch);
            };
            let (state, failure) = state_name(&operation.state);
            nodes.push(GraphReadNode {
                id: spec.id.clone(),
                binding: spec.binding.clone(),
                binding_version: spec.binding_version,
                depends_on: spec.depends_on.clone(),
                trigger: spec.trigger.clone(),
                state,
                failure,
                attempt: operation.attempt,
                confirmed_prefixes: operation
                    .confirmed_prefixes
                    .iter()
                    .map(|(stream, cumulative)| GraphReadPrefix {
                        stream: stream.clone(),
                        cumulative: *cumulative,
                    })
                    .collect(),
            });
        }
        Ok(GraphReadSnapshot {
            schema_version: 1,
            identity: GraphReadIdentity::from(&current.identity),
            phase: phase_name(current.phase),
            latest_sequence: current.latest_sequence,
            session_expired: current.session_expired,
            nodes,
        })
    }

    async fn changes(
        &self,
        session_generation: String,
        run_sequence: u64,
        cursor: u64,
    ) -> Result<GraphReadChanges, GraphReadError> {
        let changes = self
            .observer
            .changes_since(cursor)
            .map_err(|_| GraphReadError::FutureCursor)?;
        if changes.identity.session_generation().to_string() != session_generation
            || changes.identity.run_sequence() != run_sequence
        {
            return Err(GraphReadError::StaleRun);
        }
        let events = if changes.truncated {
            Vec::new()
        } else {
            changes
                .events
                .into_iter()
                .map(|event| match event.change {
                    GraphRunChange::Phase(phase) => GraphReadEvent {
                        sequence: event.sequence,
                        kind: "phase".into(),
                        phase: Some(phase_name(phase)),
                        operation_id: None,
                        attempt: None,
                        state: None,
                        failure: None,
                        stream: None,
                        cumulative: None,
                        control_target: None,
                        control_mode: None,
                    },
                    GraphRunChange::Operation {
                        operation_id, attempt, state,
                    } => {
                        let (name, failure) = state_name(&state);
                        GraphReadEvent {
                            sequence: event.sequence,
                            kind: "operation".into(),
                            operation_id: Some(operation_id),
                            attempt: Some(attempt),
                            state: Some(name),
                            failure,
                            stream: None,
                            cumulative: None,
                            phase: None,
                            control_target: None,
                            control_mode: None,
                        }
                    }
                    GraphRunChange::ConfirmedPrefix {
                        operation_id, attempt, stream, cumulative,
                    } => GraphReadEvent {
                        sequence: event.sequence,
                        kind: "confirmed_prefix".into(),
                        operation_id: Some(operation_id),
                        attempt: Some(attempt),
                        stream: Some(stream),
                        cumulative: Some(cumulative),
                        state: None,
                        failure: None,
                        phase: None,
                        control_target: None,
                        control_mode: None,
                    },
                    GraphRunChange::Control { target, mode } => GraphReadEvent {
                        sequence: event.sequence,
                        kind: "control".into(),
                        control_target: Some(format!("{target:?}")),
                        control_mode: Some(format!("{mode:?}")),
                        operation_id: None,
                        attempt: None,
                        state: None,
                        failure: None,
                        stream: None,
                        cumulative: None,
                        phase: None,
                    },
                })
                .collect()
        };
        Ok(GraphReadChanges {
            schema_version: 1,
            identity: GraphReadIdentity::from(&changes.identity),
            latest_sequence: changes.latest_sequence,
            truncated: changes.truncated,
            events,
        })
    }
}
