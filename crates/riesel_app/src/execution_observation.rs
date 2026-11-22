//! Session-local authoritative read-only observation of the existing graph runner.
//!
//! The runner owns publication; a dispatched callback may submit explicit
//! owner-confirmed prefixes through its attempt-fenced reporter. Snapshots
//! and the bounded, lossy transition ring share one authoritative mutex.
//! Neither the trace nor its cursor authorizes operations or executes a graph.
use crate::execution_nested::{
    NestedAttachment, NestedDescription, NestedDomainRegistration, NestedObservationError,
    NestedObserver, NestedParent, NestedPublisher,
};
use crate::execution_plan::{DomainAdmissionMode, DomainBoundary, OperationNode};
use crate::execution_runner::OperationState;
use riesel_protocol::SessionToken;
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, PoisonError};
use thiserror::Error;
use tokio::sync::Notify;
use uuid::Uuid;

/// Maximum retained transitions *per observed run*. Overflow never blocks
/// the runner; readers must resynchronize from the authoritative snapshot.
pub const MAX_GRAPH_TRANSITIONS: usize = 128;

/// Maximum distinct confirmed-effect streams in one operation attempt.
/// The full per-stream high-water marks remain in the authoritative snapshot;
/// the separate transition ring may overflow.
pub const MAX_GRAPH_EFFECT_STREAMS: usize = 32;

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
    /// Owner-confirmed cumulative effect prefixes by application-defined
    /// stream. These are assertions from the executing callback, not
    /// independently verified remote commits or a durable history.
    pub confirmed_prefixes: BTreeMap<String, u64>,
}

/// The explicitly advertised cooperative boundaries in this run.
///
/// All operations expose a safe point *before graph dispatch*, and the whole
/// run can gate future dispatch. A domain tick group is controllable only
/// when Riesel owns each outer tick. A delegated frame supports only its
/// outer operation boundary; its internal ticks/systems are never claimed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum GraphControlTarget {
    Run,
    Operation(String),
    RieselDomainTicks(String),
}

/// Control applies to future admission, not already running Rust or remote
/// work. StepArmed grants one dispatch, then returns to Paused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphControlMode {
    Running,
    Paused,
    StepArmed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRunSnapshot {
    pub identity: GraphRunIdentity,
    pub phase: GraphRunPhase,
    pub operations: BTreeMap<String, GraphObservedOperation>,
    /// Authoritative admission controls, including available safe-point
    /// targets. Read-only observers cannot modify these modes.
    pub controls: BTreeMap<GraphControlTarget, GraphControlMode>,
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
    /// A cumulative prefix confirmed by the operation's authoritative owner.
    /// This event may be lost; the snapshot retains the current high-water mark.
    ConfirmedPrefix {
        operation_id: String,
        attempt: u64,
        stream: String,
        cumulative: u64,
    },
    /// A control policy transition. Like all trace events it may be lost;
    /// resnapshot is authoritative.
    Control {
        target: GraphControlTarget,
        mode: GraphControlMode,
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

/// A controller is a session/run-bound in-process handle, not external
/// authorization. Unsupported internal-domain control fails explicitly.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GraphControlError {
    #[error("the application session expired")]
    SessionExpired,
    #[error("the graph run has finished or been abandoned")]
    RunClosed,
    #[error("graph control target {target:?} is not advertised")]
    UnknownTarget { target: GraphControlTarget },
    #[error("domain {domain} belongs to {owner:?}, not Riesel-owned outer ticks")]
    UnsupportedDomainAuthority {
        domain: String,
        owner: DomainAdmissionMode,
    },
    #[error("control target {target:?} has no remaining pending admission safe point")]
    NoPendingSafePoint { target: GraphControlTarget },
    #[error("step requires a paused control target")]
    NotPaused,
    #[error("exactly one step is already armed for this target")]
    StepAlreadyArmed,
}

/// Invalid owner-confirmed effect reports fail without changing the ledger.
/// An acknowledged external mutation is never inferred from operation success.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GraphEffectError {
    #[error("effect stream must be 1–128 ASCII letters, digits, dots, underscores or hyphens")]
    InvalidStream,
    #[error("confirmed cumulative progress must be positive")]
    ZeroProgress,
    #[error("the operation attempt is no longer running")]
    InactiveAttempt,
    #[error("effect stream progress cannot decrease from {previous} to {requested}")]
    RegressingPrefix { previous: u64, requested: u64 },
    #[error("an operation cannot record more than {limit} distinct effect streams")]
    StreamLimitExceeded { limit: usize },
}

struct ObservationState {
    snapshot: GraphRunSnapshot,
    transitions: VecDeque<GraphRunTransition>,
    // Private authority to report from a live graph-owned callback attempt.
    // Closed by the child future's drop guard, even before the parent runner
    // processes the JoinSet completion. Not part of a public snapshot.
    recording_attempts: BTreeMap<String, u64>,
    // Private plan-derived ownership information used to reject a request
    // to step delegated internal ticks or a fully dispatched boundary.
    domain_owners: BTreeMap<String, DomainAdmissionMode>,
    domain_nodes: BTreeMap<String, Vec<String>>,
    // At most one optional attachment per plan operation; never a second
    // graph state engine or an unbounded per-attempt ledger.
    nested: BTreeMap<String, NestedAttachment>,
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
    session: SessionToken,
}

impl GraphRunObserver {
    /// Discover an opt-in nested view without guessing an ECS internal node.
    /// None means this plan/run does not publish a view for this operation.
    pub fn nested_description(&self, operation: &str) -> Option<NestedDescription> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .nested
            .get(operation)
            .map(|attachment| attachment.description.clone())
    }

    /// Acquire an immutable typed local reader. Version/type mismatches are
    /// explicit, and no controller, mutating World or process credential leaks.
    pub fn nested<T: Send + Sync + 'static>(
        &self,
        operation: &str,
        expected_version: u32,
    ) -> Result<Option<NestedObserver<T>>, NestedObservationError> {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(entry) = state.nested.get(operation) else {
            return Ok(None);
        };
        if entry.description.view_version != expected_version {
            return Err(NestedObservationError::UnsupportedVersion {
                expected: expected_version,
                actual: entry.description.view_version,
            });
        }
        entry.observer::<T>(self.session.clone()).map(Some)
    }

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

/// A trusted, local handle to one existing run's cooperative admission gate.
///
/// Obtain from `ExecutionGraphRunner::controller()` before moving the runner
/// into an async task. Pause is linearized against *future graph dispatch*
/// under the authoritative snapshot mutex; it does not suspend work that has
/// already been admitted. A step arms one future dispatch, not a completed
/// effect, frame, ECS system or external operation.
#[derive(Clone)]
pub struct GraphRunController {
    state: Arc<Mutex<ObservationState>>,
    session: SessionToken,
    changed: Arc<Notify>,
}

enum ControlAction {
    Pause,
    Step,
    Resume,
}

impl GraphRunController {
    pub fn identity(&self) -> GraphRunIdentity {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot
            .identity
            .clone()
    }

    pub fn pause(&self, target: GraphControlTarget) -> Result<(), GraphControlError> {
        self.update(target, ControlAction::Pause)
    }

    pub fn step(&self, target: GraphControlTarget) -> Result<(), GraphControlError> {
        self.update(target, ControlAction::Step)
    }

    pub fn resume(&self, target: GraphControlTarget) -> Result<(), GraphControlError> {
        self.update(target, ControlAction::Resume)
    }

    fn update(
        &self,
        target: GraphControlTarget,
        action: ControlAction,
    ) -> Result<(), GraphControlError> {
        if !self.session.is_active() {
            return Err(GraphControlError::SessionExpired);
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        // Recheck under lock before updating; a revoked session can never
        // grant a new run a control capability by sharing a plan ID.
        if !self.session.is_active() {
            return Err(GraphControlError::SessionExpired);
        }
        if matches!(
            state.snapshot.phase,
            GraphRunPhase::Finished | GraphRunPhase::Abandoned
        ) {
            return Err(GraphControlError::RunClosed);
        }
        if let GraphControlTarget::RieselDomainTicks(domain) = &target
            && let Some(&owner) = state.domain_owners.get(domain)
            && owner != DomainAdmissionMode::RieselTick
        {
            return Err(GraphControlError::UnsupportedDomainAuthority {
                domain: domain.clone(),
                owner,
            });
        }
        let Some(&current) = state.snapshot.controls.get(&target) else {
            return Err(GraphControlError::UnknownTarget { target });
        };
        let pending = match &target {
            GraphControlTarget::Run => state
                .snapshot
                .operations
                .values()
                .any(|op| matches!(op.state, OperationState::Pending)),
            GraphControlTarget::Operation(name) => state
                .snapshot
                .operations
                .get(name)
                .is_some_and(|op| matches!(op.state, OperationState::Pending)),
            GraphControlTarget::RieselDomainTicks(domain) => {
                state.domain_nodes.get(domain).is_some_and(|nodes| {
                    nodes.iter().any(|name| {
                        matches!(
                            state.snapshot.operations[name].state,
                            OperationState::Pending
                        )
                    })
                })
            }
        };
        if !pending {
            return Err(GraphControlError::NoPendingSafePoint { target });
        }
        let next = match action {
            ControlAction::Pause => GraphControlMode::Paused,
            ControlAction::Resume => GraphControlMode::Running,
            ControlAction::Step => match current {
                GraphControlMode::Running => return Err(GraphControlError::NotPaused),
                GraphControlMode::StepArmed => return Err(GraphControlError::StepAlreadyArmed),
                GraphControlMode::Paused => GraphControlMode::StepArmed,
            },
        };
        if current != next {
            state.snapshot.controls.insert(target.clone(), next);
            state.push(GraphRunChange::Control { target, mode: next });
            drop(state);
            // The runner already registers a shared admission notification
            // before testing capacity. Reuse it; no second scheduler or
            // polling loop is introduced by the controller.
            self.changed.notify_waiters();
        }
        Ok(())
    }
}

/// A scoped reporting handle passed only to a dispatched operation attempt.
///
/// Call `confirm_prefix` after the *actual domain or provider owner* has
/// confirmed the cumulative committed prefix. Duplicate equal reports are
/// idempotent. This does not authenticate a remote service, roll back effects
/// or permit retries after a lost acknowledgement.
#[derive(Clone)]
pub struct GraphEffectRecorder {
    state: Arc<Mutex<ObservationState>>,
    operation_id: String,
    attempt: u64,
    // Set by the immutable validated plan, never accepted from caller input.
    delegated_domain: Option<String>,
}

/// Held by the actual graph-owned child future and dropped on completion,
/// panic or cancellation. Its closure and every confirmation use the same
/// observation mutex, so a cloned reporter cannot publish after closure.
pub(crate) struct GraphEffectAttemptGuard {
    state: Arc<Mutex<ObservationState>>,
    operation_id: String,
    attempt: u64,
}

impl Drop for GraphEffectAttemptGuard {
    fn drop(&mut self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.recording_attempts.get(&self.operation_id) == Some(&self.attempt) {
            state.recording_attempts.remove(&self.operation_id);
        }
    }
}

impl GraphEffectRecorder {
    /// Bind an owner-published, typed read-only source to this **actually
    /// dispatched** delegated-frame operation. The exact parent run and
    /// attempt come from the runner, not from the application or viewer.
    /// The returned publisher must be moved to the domain's real owner, which
    /// copies native state into immutable snapshots at its own safe points.
    ///
    /// This is trusted in-process cooperation, not client authentication.
    pub fn attach_nested<T: Send + Sync + 'static>(
        &self,
        registration: &NestedDomainRegistration,
        view_version: u32,
        initial: T,
    ) -> Result<NestedPublisher<T>, NestedObservationError> {
        let expected = self
            .delegated_domain
            .as_ref()
            .ok_or(NestedObservationError::UnsupportedBoundary)?;
        if view_version == 0 {
            return Err(NestedObservationError::InvalidVersion);
        }
        let (domain, generation, owner) = registration.live_owner()?;
        if &domain != expected {
            return Err(NestedObservationError::WrongDomain {
                expected: expected.clone(),
                actual: domain,
            });
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.snapshot.phase != GraphRunPhase::Running
            || state.recording_attempts.get(&self.operation_id) != Some(&self.attempt)
            || !matches!(
                state.snapshot.operations.get(&self.operation_id),
                Some(GraphObservedOperation {
                    state: OperationState::Running,
                    ..
                })
            )
        {
            return Err(NestedObservationError::InactiveAttempt);
        }
        if let Some(previous) = state.nested.get(&self.operation_id) {
            if previous.description.owner_generation == generation {
                return Err(NestedObservationError::AlreadyAttached);
            }
            previous.retire();
        }
        let description = NestedDescription {
            parent: NestedParent {
                run: state.snapshot.identity.clone(),
                operation: self.operation_id.clone(),
                attempt: self.attempt,
            },
            domain,
            owner_generation: generation,
            view_version,
        };
        let (attachment, publisher) = NestedPublisher::attach(description, owner, initial);
        state.nested.insert(self.operation_id.clone(), attachment);
        Ok(publisher)
    }

    pub fn confirm_prefix(&self, stream: &str, cumulative: u64) -> Result<(), GraphEffectError> {
        if stream.is_empty()
            || stream.len() > 128
            || !stream
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(GraphEffectError::InvalidStream);
        }
        if cumulative == 0 {
            return Err(GraphEffectError::ZeroProgress);
        }
        // Callback completion and confirmations serialize through the same
        // authoritative lock, even if the parent coordinator has not yet
        // consumed the operation's JoinSet result.
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.snapshot.phase != GraphRunPhase::Running
            || state.recording_attempts.get(&self.operation_id) != Some(&self.attempt)
        {
            return Err(GraphEffectError::InactiveAttempt);
        }
        let operation = state
            .snapshot
            .operations
            .get_mut(&self.operation_id)
            .expect("validated operation identity");
        if operation.state != OperationState::Running || operation.attempt != self.attempt {
            return Err(GraphEffectError::InactiveAttempt);
        }
        if let Some(&previous) = operation.confirmed_prefixes.get(stream) {
            if cumulative < previous {
                return Err(GraphEffectError::RegressingPrefix {
                    previous,
                    requested: cumulative,
                });
            }
            if cumulative == previous {
                return Ok(());
            }
        } else if operation.confirmed_prefixes.len() >= MAX_GRAPH_EFFECT_STREAMS {
            return Err(GraphEffectError::StreamLimitExceeded {
                limit: MAX_GRAPH_EFFECT_STREAMS,
            });
        }
        operation
            .confirmed_prefixes
            .insert(stream.to_owned(), cumulative);
        state.push(GraphRunChange::ConfirmedPrefix {
            operation_id: self.operation_id.clone(),
            attempt: self.attempt,
            stream: stream.to_owned(),
            cumulative,
        });
        Ok(())
    }
}

/// Single-writer publication boundary owned by the graph runner. Its Drop
/// guard makes abrupt future cancellation observable without inventing an
/// external rollback, completed callback or successful downstream operation.
pub(crate) struct GraphRunPublication {
    observer: GraphRunObserver,
    session: SessionToken,
    nodes: Vec<String>,
    delegated_domains: Vec<Option<String>>,
    dependencies: Vec<Vec<usize>>,
    finished: bool,
}

impl GraphRunPublication {
    pub(crate) fn new(
        identity: GraphRunIdentity,
        nodes: &[OperationNode],
        session: SessionToken,
    ) -> Self {
        let names: Vec<String> = nodes.iter().map(|node| node.id().to_owned()).collect();
        let delegated_domains = nodes
            .iter()
            .map(|node| {
                node.domain_boundary().and_then(|boundary| {
                    (boundary.mode() == DomainAdmissionMode::DelegatedFrame)
                        .then(|| boundary.domain().to_owned())
                })
            })
            .collect();
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
        let mut controls = BTreeMap::from([(GraphControlTarget::Run, GraphControlMode::Running)]);
        let mut domain_owners = BTreeMap::new();
        let mut domain_nodes = BTreeMap::<String, Vec<String>>::new();
        for node in nodes {
            controls.insert(
                GraphControlTarget::Operation(node.id().to_owned()),
                GraphControlMode::Running,
            );
            if let Some(boundary) = node.domain_boundary() {
                domain_owners.insert(boundary.domain().to_owned(), boundary.mode());
                domain_nodes
                    .entry(boundary.domain().to_owned())
                    .or_default()
                    .push(node.id().to_owned());
                if boundary.mode() == DomainAdmissionMode::RieselTick {
                    controls.insert(
                        GraphControlTarget::RieselDomainTicks(boundary.domain().to_owned()),
                        GraphControlMode::Running,
                    );
                }
            }
        }
        let operations = names
            .iter()
            .map(|name| {
                (
                    name.clone(),
                    GraphObservedOperation {
                        state: OperationState::Pending,
                        attempt: 0,
                        confirmed_prefixes: BTreeMap::new(),
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
                        controls,
                        latest_sequence: 0,
                        session_expired: false,
                    },
                    transitions: VecDeque::new(),
                    recording_attempts: BTreeMap::new(),
                    domain_owners,
                    domain_nodes,
                    nested: BTreeMap::new(),
                })),
                session: session.clone(),
            },
            nodes: names,
            delegated_domains,
            dependencies,
            session,
            finished: false,
        }
    }

    pub(crate) fn observer(&self) -> GraphRunObserver {
        self.observer.clone()
    }

    pub(crate) fn controller(&self, changed: Arc<Notify>) -> GraphRunController {
        GraphRunController {
            state: Arc::clone(&self.observer.state),
            session: self.session.clone(),
            changed,
        }
    }

    /// Cheap read avoids momentarily reserving shared resource capacity
    /// while this run is paused. Admission rechecks under the same mutex
    /// after reservation to prevent races with concurrent control commands.
    pub(crate) fn may_admit(&self, index: usize, domain: Option<&DomainBoundary>) -> bool {
        let state = self
            .observer
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        Self::targets(&self.nodes[index], domain)
            .iter()
            .all(|target| {
                matches!(
                    state.snapshot.controls.get(target),
                    Some(GraphControlMode::Running | GraphControlMode::StepArmed)
                )
            })
    }

    /// Atomically consume at most one armed step at every matching boundary
    /// and publish the actual running attempt. Caller must already own its
    /// session-level operation reservation, but not yet have spawned work.
    pub(crate) fn admit(&mut self, index: usize, domain: Option<&DomainBoundary>) -> bool {
        let mut state = self
            .observer
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let name = &self.nodes[index];
        if state.snapshot.phase != GraphRunPhase::Running
            || !matches!(
                state.snapshot.operations[name].state,
                OperationState::Pending
            )
        {
            return false;
        }
        let targets = Self::targets(name, domain);
        if targets.iter().any(|target| {
            !matches!(
                state.snapshot.controls.get(target),
                Some(GraphControlMode::Running | GraphControlMode::StepArmed)
            )
        }) {
            return false;
        }
        for target in targets {
            if state.snapshot.controls.get(&target) == Some(&GraphControlMode::StepArmed) {
                state
                    .snapshot
                    .controls
                    .insert(target.clone(), GraphControlMode::Paused);
                state.push(GraphRunChange::Control {
                    target,
                    mode: GraphControlMode::Paused,
                });
            }
        }
        let op = state
            .snapshot
            .operations
            .get_mut(name)
            .expect("validated operation id");
        op.attempt = op
            .attempt
            .checked_add(1)
            .expect("one operation exhausted u64 attempts");
        op.state = OperationState::Running;
        let attempt = op.attempt;
        state.recording_attempts.insert(name.clone(), attempt);
        state.push(GraphRunChange::Operation {
            operation_id: name.clone(),
            attempt,
            state: OperationState::Running,
        });
        true
    }

    fn targets(name: &str, domain: Option<&DomainBoundary>) -> Vec<GraphControlTarget> {
        let mut targets = vec![
            GraphControlTarget::Run,
            GraphControlTarget::Operation(name.to_owned()),
        ];
        if let Some(boundary) = domain
            && boundary.mode() == DomainAdmissionMode::RieselTick
        {
            targets.push(GraphControlTarget::RieselDomainTicks(
                boundary.domain().to_owned(),
            ));
        }
        targets
    }

    pub(crate) fn recorder(&self, index: usize) -> (GraphEffectRecorder, GraphEffectAttemptGuard) {
        let state = self
            .observer
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let operation_id = self.nodes[index].clone();
        let attempt = state.snapshot.operations[&operation_id].attempt;
        debug_assert!(attempt > 0, "only dispatched attempts may confirm effects");
        (
            GraphEffectRecorder {
                state: Arc::clone(&self.observer.state),
                operation_id: operation_id.clone(),
                attempt,
                delegated_domain: self.delegated_domains[index].clone(),
            },
            GraphEffectAttemptGuard {
                state: Arc::clone(&self.observer.state),
                operation_id,
                attempt,
            },
        )
    }

    pub(crate) fn identity(&self) -> GraphRunIdentity {
        self.observer
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot
            .identity
            .clone()
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
            if matches!(phase, GraphRunPhase::Finished | GraphRunPhase::Abandoned) {
                // A step that never found an eligible boundary cannot remain
                // armed after the owning run has ceased accepting work.
                let armed: Vec<_> = state
                    .snapshot
                    .controls
                    .iter()
                    .filter_map(|(target, mode)| {
                        (*mode == GraphControlMode::StepArmed).then_some(target.clone())
                    })
                    .collect();
                for target in armed {
                    state
                        .snapshot
                        .controls
                        .insert(target.clone(), GraphControlMode::Paused);
                    state.push(GraphRunChange::Control {
                        target,
                        mode: GraphControlMode::Paused,
                    });
                }
            }
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
        if matches!(next, OperationState::Running) {
            state.recording_attempts.insert(name.clone(), attempt);
        } else {
            state.recording_attempts.remove(name);
        }
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
        {
            let mut state = self
                .observer
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state.snapshot.session_expired = !self.session.is_active();
        }
        self.phase(GraphRunPhase::Abandoned);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::OperationKey;

    #[test]
    fn callback_guard_closes_recording_before_coordinator_observes_completion() {
        let nodes = vec![OperationNode::new(
            "tick",
            OperationKey::new("test.tick", 1).unwrap(),
        )];
        let identity = GraphRunIdentity::new(Uuid::new_v4(), 1, "guard-test".into(), 1);
        let mut publication = GraphRunPublication::new(identity, &nodes, SessionToken::new());
        publication.start();
        publication.transition(0, OperationState::Running);
        let (recorder, guard) = publication.recorder(0);
        recorder.confirm_prefix("ticks", 1).unwrap();

        // Simulate the callback future returning before the outer runner has
        // polled the JoinSet. A retained clone must no longer mutate state.
        let stale = recorder.clone();
        drop(guard);
        assert_eq!(
            publication.observer().snapshot().operations["tick"].state,
            OperationState::Running
        );
        assert!(matches!(
            stale.confirm_prefix("ticks", 2),
            Err(GraphEffectError::InactiveAttempt)
        ));
        assert_eq!(
            publication.observer().snapshot().operations["tick"].confirmed_prefixes["ticks"],
            1
        );
        publication.transition(0, OperationState::Completed);
        publication.finish(false);
    }
}
