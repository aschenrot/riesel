//! Session-local authoritative read-only observation of the existing graph runner.
//!
//! The runner is the only publisher. Snapshots are copied under one short
//! mutex, while the bounded transition ring is an optional, lossy projection.
//! Neither the trace nor its cursor authorizes operations or executes a graph.
use crate::execution_plan::OperationNode;
use crate::execution_runner::OperationState;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use thiserror::Error;
use uuid::Uuid;

/// Maximum retained transitions *per observed run*. Overflow never blocks
/// the runner; readers must resynchronize from the authoritative snapshot.
pub const MAX_GRAPH_TRANSITIONS: usize = 128;

/// A runtime-allocated session generation and per-session run sequence, bound
/// to one validated immutable logical plan version. These values are not an
/// authenticated external application identity or a persistence protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRunIdentity {
    session_generation: Uuid,
    run_sequence: u64,
    plan_id: String,
    revision: u64,
}

impl GraphRunIdentity {
    pub(crate) fn new(
        session_generation: Uuid,
        run_sequence: u64,
        plan_id: String,
        revision: u64,
    ) -> Self {
        Self {
            session_generation,
            run_sequence,
            plan_id,
            revision,
        }
    }

    pub fn session_generation(&self) -> Uuid {
        self.session_generation
    }

    pub fn run_sequence(&self) -> u64 {
        self.run_sequence
    }

    pub fn plan_id(&self) -> &str {
        &self.plan_id
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphRunPhase {
    Created,
    Running,
    Finished,
    /// The owning graph future was dropped without producing a report.
    /// In-flight dispatched effects remain conservatively unknown.
    Abandoned,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphObservedOperation {
    pub state: OperationState,
    /// Zero means never admitted. First admission is attempt one; there are
    /// no automatic retries. A future retry policy must change this explicitly.
    pub attempt: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRunSnapshot {
    pub identity: GraphRunIdentity,
    pub phase: GraphRunPhase,
    pub operations: BTreeMap<String, GraphObservedOperation>,
    pub latest_sequence: u64,
    pub session_expired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphRunChange {
    Phase(GraphRunPhase),
    Operation {
        operation_id: String,
        attempt: u64,
        state: OperationState,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRunTransition {
    pub identity: GraphRunIdentity,
    pub sequence: u64,
    pub change: GraphRunChange,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRunTransitions {
    pub identity: GraphRunIdentity,
    pub latest_sequence: u64,
    /// True when the requested cursor preceded the oldest retained event.
    /// The caller must take a new snapshot; there is no invented replay.
    pub truncated: bool,
    pub events: Vec<GraphRunTransition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GraphObservationError {
    #[error("transition cursor {requested} is ahead of current sequence {latest}")]
    FutureCursor { requested: u64, latest: u64 },
}

struct ObservationState {
    snapshot: GraphRunSnapshot,
    transitions: VecDeque<GraphRunTransition>,
}

impl ObservationState {
    fn push(&mut self, change: GraphRunChange) {
        self.snapshot.latest_sequence = self
            .snapshot
            .latest_sequence
            .checked_add(1)
            .expect("one run exhausted u64 transition sequence");
        self.transitions.push_back(GraphRunTransition {
            identity: self.snapshot.identity.clone(),
            sequence: self.snapshot.latest_sequence,
            change,
        });
        if self.transitions.len() > MAX_GRAPH_TRANSITIONS {
            self.transitions.pop_front();
        }
    }
}

/// An in-process read-only view into a single application-owned graph run.
/// A clone retains only this run's bounded snapshot/events, never a live
/// executor, provider, process handle, or authority to mutate execution.
#[derive(Clone)]
pub struct GraphRunObserver {
    state: Arc<Mutex<ObservationState>>,
}

impl GraphRunObserver {
    pub fn snapshot(&self) -> GraphRunSnapshot {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot
            .clone()
    }

    /// Read retained transitions strictly after `cursor`. Start with zero,
    /// then use `latest_sequence`. A future cursor fails closed; a stale
    /// cursor returns `truncated` and requires resnapshot rather than replay.
    pub fn changes_since(&self, cursor: u64) -> Result<GraphRunTransitions, GraphObservationError> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let latest = state.snapshot.latest_sequence;
        if cursor > latest {
            return Err(GraphObservationError::FutureCursor {
                requested: cursor,
                latest,
            });
        }
        let first = state
            .transitions
            .front()
            .map_or(latest.saturating_add(1), |event| event.sequence);
        Ok(GraphRunTransitions {
            identity: state.snapshot.identity.clone(),
            latest_sequence: latest,
            truncated: cursor < first.saturating_sub(1),
            events: state
                .transitions
                .iter()
                .filter(|event| event.sequence > cursor)
                .cloned()
                .collect(),
        })
    }
}

/// Single-writer publication boundary owned by the graph runner. Its Drop
/// guard makes abrupt future cancellation observable without inventing an
/// external rollback, completed callback or successful downstream operation.
pub(crate) struct GraphRunPublication {
    observer: GraphRunObserver,
    nodes: Vec<String>,
    dependencies: Vec<Vec<usize>>,
    finished: bool,
}

impl GraphRunPublication {
    pub(crate) fn new(identity: GraphRunIdentity, nodes: &[OperationNode]) -> Self {
        let names: Vec<String> = nodes.iter().map(|node| node.id().to_owned()).collect();
        let index: BTreeMap<_, _> = names
            .iter()
            .enumerate()
            .map(|(i, name)| (name.as_str(), i))
            .collect();
        let dependencies = nodes
            .iter()
            .map(|node| {
                node.dependencies()
                    .iter()
                    .map(|name| index[name.as_str()])
                    .collect()
            })
            .collect();
        let operations = names
            .iter()
            .map(|name| {
                (
                    name.clone(),
                    GraphObservedOperation {
                        state: OperationState::Pending,
                        attempt: 0,
                    },
                )
            })
            .collect();
        Self {
            observer: GraphRunObserver {
                state: Arc::new(Mutex::new(ObservationState {
                    snapshot: GraphRunSnapshot {
                        identity,
                        phase: GraphRunPhase::Created,
                        operations,
                        latest_sequence: 0,
                        session_expired: false,
                    },
                    transitions: VecDeque::new(),
                })),
            },
            nodes: names,
            dependencies,
            finished: false,
        }
    }

    pub(crate) fn observer(&self) -> GraphRunObserver {
        self.observer.clone()
    }

    pub(crate) fn identity(&self) -> GraphRunIdentity {
        self.observer.snapshot().identity
    }

    pub(crate) fn start(&mut self) {
        self.phase(GraphRunPhase::Running);
    }

    fn phase(&mut self, phase: GraphRunPhase) {
        let mut state = self
            .observer
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if state.snapshot.phase != phase {
            state.snapshot.phase = phase;
            state.push(GraphRunChange::Phase(phase));
        }
    }

    /// Called only by the runner, immediately after its own state transition.
    pub(crate) fn transition(&mut self, index: usize, next: OperationState) {
        let mut state = self
            .observer
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let name = &self.nodes[index];
        let operation = state
            .snapshot
            .operations
            .get_mut(name)
            .expect("validated operation identity");
        if operation.state == next {
            return;
        }
        if matches!(next, OperationState::Running) {
            operation.attempt = operation
                .attempt
                .checked_add(1)
                .expect("one operation exhausted u64 attempts");
        }
        operation.state = next.clone();
        let attempt = operation.attempt;
        state.push(GraphRunChange::Operation {
            operation_id: name.clone(),
            attempt,
            state: next,
        });
    }

    pub(crate) fn finish(&mut self, session_expired: bool) {
        {
            let mut state = self
                .observer
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state.snapshot.session_expired = session_expired;
        }
        self.phase(GraphRunPhase::Finished);
        self.finished = true;
    }

    fn current(&self, index: usize) -> OperationState {
        let state = self
            .observer
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        state.snapshot.operations[&self.nodes[index]].state.clone()
    }
}

impl Drop for GraphRunPublication {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // The graph future may have been aborted after dispatch, not joined.
        // Conservatively preserve "unknown" instead of claiming cancellation
        // of an external effect that may already have committed.
        for index in 0..self.nodes.len() {
            if matches!(self.current(index), OperationState::Running) {
                self.transition(index, OperationState::OutcomeUnknown);
            }
        }
        // Block descendants of unknown/failed work before marking unrelated
        // not-yet-admitted operations as cancelled.
        loop {
            let mut changed = false;
            for index in 0..self.nodes.len() {
                if !matches!(self.current(index), OperationState::Pending) {
                    continue;
                }
                if self.dependencies[index].iter().any(|&dependency| {
                    matches!(
                        self.current(dependency),
                        OperationState::Failed(_)
                            | OperationState::Cancelled
                            | OperationState::OutcomeUnknown
                            | OperationState::Blocked
                    )
                }) {
                    self.transition(index, OperationState::Blocked);
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        for index in 0..self.nodes.len() {
            if matches!(self.current(index), OperationState::Pending) {
                self.transition(index, OperationState::Cancelled);
            }
        }
        self.phase(GraphRunPhase::Abandoned);
    }
}
