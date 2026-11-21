//! Bounded, session-scoped execution of already-validated Rust operation plans.
//!
//! The runner is owned by one awaited application future. Its Tokio JoinSet
//! cannot outlive that future: there is no independent runtime, supervisor,
//! provider resolver or ECS scheduler here. The optional async-tcp feature
//! supplies the existing Tokio session infrastructure.
//!
//! This first increment admits one-shot operations, explicit signals and
//! declarative resource conflicts. Authoritative run identity, committed-effect
//! ledgers and cooperative step controls belong to Issue #113.
use crate::execution_plan::{AccessMode, BoundExecutionPlan, OperationTrigger, ResourceAccess};
use crate::runtime::RunningApp;
use riesel_protocol::SessionToken;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::{Id, JoinSet};

const MAX_PARALLEL_OPERATIONS: usize = 32;
const MAX_PENDING_SIGNALS: usize = 64;

/// A registered operation does not own a Tokio runtime or an ECS world.
/// Delegated thread-affine work must remain in its domain-owned executor.
pub type OperationFuture = Pin<Box<dyn Future<Output = OperationOutcome> + Send + 'static>>;
pub type AsyncOperation = dyn Fn() -> OperationFuture + Send + Sync + 'static;

/// Adapt a Rust async closure to the exact-version operation registry.
/// The closure is not invoked by validation or binding.
pub fn async_operation<F, Fut>(operation: F) -> Arc<AsyncOperation>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = OperationOutcome> + Send + 'static,
{
    Arc::new(move || Box::pin(operation()) as OperationFuture)
}

/// An operation's terminal classification. Cancellation does not roll back
/// previously committed effects. A lost external reply must be Unknown, not
/// success or proven cancellation. Detailed effects are tracked by #113.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOutcome {
    Completed,
    Failed(String),
    Cancelled,
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationState {
    Pending,
    Running,
    Completed,
    Failed(String),
    Cancelled,
    OutcomeUnknown,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum GraphRunError {
    #[error("the application session expired")]
    SessionExpired,
    #[error("requested {requested} concurrent operations exceeds limit {maximum}")]
    InvalidParallelism { requested: usize, maximum: usize },
    #[error("plan does not declare signal {name:?}")]
    UnknownSignal { name: String },
    #[error("the bounded signal queue is full")]
    SignalQueueFull,
    #[error("the graph is no longer accepting signals")]
    SignalQueueClosed,
}

/// A local, bounded admission handle; not an authorized remote control API.
#[derive(Clone)]
pub struct GraphSignalSender {
    sender: mpsc::Sender<String>,
    allowed: Arc<BTreeSet<String>>,
    session: SessionToken,
}

impl GraphSignalSender {
    pub fn fire(&self, name: &str) -> Result<(), GraphRunError> {
        if !self.session.is_active() {
            return Err(GraphRunError::SessionExpired);
        }
        if !self.allowed.contains(name) {
            return Err(GraphRunError::UnknownSignal {
                name: name.to_owned(),
            });
        }
        self.sender
            .try_send(name.to_owned())
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => GraphRunError::SignalQueueFull,
                mpsc::error::TrySendError::Closed(_) => GraphRunError::SignalQueueClosed,
            })
    }
}

/// A completed local execution report, NOT a globally unique run identity or
/// the authoritative live/synchronized run snapshot planned by #113.
#[derive(Debug, Clone)]
pub struct GraphRunReport {
    pub plan_id: String,
    pub revision: u64,
    pub operations: BTreeMap<String, OperationState>,
    pub started: usize,
    pub peak_in_flight: usize,
    pub session_expired: bool,
}

impl GraphRunReport {
    pub fn succeeded(&self) -> bool {
        !self.session_expired
            && self
                .operations
                .values()
                .all(|state| matches!(state, OperationState::Completed))
    }
}

/// Consumed by execute(): aborting/dropping its future cannot leave a
/// reusable runner containing stale Running nodes. In-flight operations are
/// aborted on session invalidation, but effects already dispatched may be
/// irreversible; they are conservatively reported as OutcomeUnknown.
pub struct ExecutionGraphRunner {
    plan: BoundExecutionPlan<AsyncOperation>,
    session: SessionToken,
    index: BTreeMap<String, usize>,
    states: Vec<OperationState>,
    fired: BTreeSet<String>,
    signal_rx: mpsc::Receiver<String>,
    signal_tx: GraphSignalSender,
    max_in_flight: usize,
    peak_in_flight: usize,
    started: usize,
}

impl ExecutionGraphRunner {
    /// Bind an existing session to an already-validated and registered plan.
    /// No extra Tokio runtime or provider instance is created.
    pub fn new(
        plan: BoundExecutionPlan<AsyncOperation>,
        session: SessionToken,
        max_in_flight: NonZeroUsize,
    ) -> Result<Self, GraphRunError> {
        if !session.is_active() {
            return Err(GraphRunError::SessionExpired);
        }
        if max_in_flight.get() > MAX_PARALLEL_OPERATIONS {
            return Err(GraphRunError::InvalidParallelism {
                requested: max_in_flight.get(),
                maximum: MAX_PARALLEL_OPERATIONS,
            });
        }
        let operations = plan.plan().description().operations();
        let index = operations
            .iter()
            .enumerate()
            .map(|(i, node)| (node.id().to_owned(), i))
            .collect();
        let allowed = operations
            .iter()
            .filter_map(|node| match node.trigger() {
                OperationTrigger::Immediate => None,
                OperationTrigger::Signal(signal) => Some(signal.clone()),
            })
            .collect();
        let (sender, signal_rx) = mpsc::channel(MAX_PENDING_SIGNALS);
        Ok(Self {
            states: vec![OperationState::Pending; operations.len()],
            plan,
            session: session.clone(),
            index,
            fired: BTreeSet::new(),
            signal_rx,
            signal_tx: GraphSignalSender {
                sender,
                allowed: Arc::new(allowed),
                session,
            },
            max_in_flight: max_in_flight.get(),
            peak_in_flight: 0,
            started: 0,
        })
    }

    pub fn signal_sender(&self) -> GraphSignalSender {
        self.signal_tx.clone()
    }

    /// Execute a single immutable plan revision once. Waiting for an external
    /// signal is intentional; the owning application may cancel its session.
    /// Calls are never retried, and other branches remain eligible while a
    /// signal-gated domain tick is waiting.
    pub async fn execute(mut self) -> GraphRunReport {
        let mut jobs = JoinSet::<(usize, OperationOutcome)>::new();
        let mut active = HashMap::<Id, usize>::new();
        let mut session_expired = false;

        loop {
            while let Ok(signal) = self.signal_rx.try_recv() {
                self.fired.insert(signal);
            }
            self.block_dependents();
            if !self.session.is_active() {
                session_expired = true;
                break;
            }
            self.admit(&mut jobs, &mut active);

            if active.is_empty()
                && !self
                    .states
                    .iter()
                    .any(|state| matches!(state, OperationState::Pending))
            {
                break;
            }

            match wait_for_work(
                &self.session,
                &mut self.signal_rx,
                &mut jobs,
                !active.is_empty(),
            )
            .await
            {
                Wake::Expired => {
                    session_expired = true;
                    break;
                }
                Wake::Signal(signal) => {
                    self.fired.insert(signal);
                }
                Wake::Completed(result) => match result {
                    Ok((id, (index, outcome))) => {
                        let owned = active.remove(&id);
                        debug_assert_eq!(owned, Some(index));
                        self.states[index] = match outcome {
                            OperationOutcome::Completed => OperationState::Completed,
                            OperationOutcome::Failed(message) => OperationState::Failed(message),
                            OperationOutcome::Cancelled => OperationState::Cancelled,
                            OperationOutcome::OutcomeUnknown => OperationState::OutcomeUnknown,
                        };
                    }
                    Err(error) => {
                        let index = active
                            .remove(&error.id())
                            .expect("tracked operation task identity");
                        self.states[index] = OperationState::Failed(error.to_string());
                    }
                },
            }
        }

        if session_expired {
            jobs.abort_all();
            while jobs.join_next().await.is_some() {}
            for index in active.into_values() {
                // The body may have dispatched external effects before abort.
                self.states[index] = OperationState::OutcomeUnknown;
            }
            self.block_dependents();
            for state in &mut self.states {
                if matches!(state, OperationState::Pending) {
                    *state = OperationState::Cancelled;
                }
            }
        }

        let description = self.plan.plan().description();
        let operations = description
            .operations()
            .iter()
            .zip(self.states)
            .map(|(node, state)| (node.id().to_owned(), state))
            .collect();
        GraphRunReport {
            plan_id: description.id().to_owned(),
            revision: description.revision(),
            operations,
            started: self.started,
            peak_in_flight: self.peak_in_flight,
            session_expired,
        }
    }

    fn admit(
        &mut self,
        jobs: &mut JoinSet<(usize, OperationOutcome)>,
        active: &mut HashMap<Id, usize>,
    ) {
        let operations = self.plan.plan().description().operations();
        for (index, node) in operations.iter().enumerate() {
            if active.len() >= self.max_in_flight {
                break;
            }
            if !matches!(self.states[index], OperationState::Pending) {
                continue;
            }
            if node.dependencies().iter().any(|dep| {
                !matches!(
                    self.states[*self.index.get(dep).expect("validated dependency")],
                    OperationState::Completed
                )
            }) {
                continue;
            }
            if let OperationTrigger::Signal(signal) = node.trigger()
                && !self.fired.contains(signal)
            {
                continue;
            }
            if active
                .values()
                .any(|&other| conflicts(node.resources(), operations[other].resources()))
            {
                continue;
            }
            let handler = Arc::clone(
                self.plan
                    .handler(node.id())
                    .expect("validated exact-version operation binding"),
            );
            let session = self.session.clone();
            let handle = jobs.spawn(async move {
                if !session.is_active() {
                    (index, OperationOutcome::Cancelled)
                } else {
                    (index, handler().await)
                }
            });
            active.insert(handle.id(), index);
            self.states[index] = OperationState::Running;
            self.started += 1;
            self.peak_in_flight = self.peak_in_flight.max(active.len());
        }
    }

    fn block_dependents(&mut self) {
        let operations = self.plan.plan().description().operations();
        loop {
            let mut changed = false;
            for (index, node) in operations.iter().enumerate() {
                if !matches!(self.states[index], OperationState::Pending) {
                    continue;
                }
                let failed_prerequisite = node.dependencies().iter().any(|dep| {
                    matches!(
                        self.states[*self.index.get(dep).expect("validated dependency")],
                        OperationState::Failed(_)
                            | OperationState::Cancelled
                            | OperationState::OutcomeUnknown
                            | OperationState::Blocked
                    )
                });
                if failed_prerequisite {
                    self.states[index] = OperationState::Blocked;
                    changed = true;
                }
            }
            if !changed {
                return;
            }
        }
    }
}


enum Wake {
    Expired,
    Signal(String),
    Completed(Result<(Id, (usize, OperationOutcome)), tokio::task::JoinError>),
}

/// Mirror the existing async-task poll_fn cancellation pattern; this does
/// not require Tokio's optional macros feature. Recreate neither Notify
/// subscription nor the join future on every poll. Finished operations are
/// polled before the signal queue, so a busy producer cannot starve cleanup.
async fn wait_for_work(
    session: &SessionToken,
    signals: &mut mpsc::Receiver<String>,
    jobs: &mut JoinSet<(usize, OperationOutcome)>,
    has_jobs: bool,
) -> Wake {
    use std::task::Poll;

    let mut expired = std::pin::pin!(session.cancelled());
    let mut incoming = std::pin::pin!(signals.recv());
    let mut completed = std::pin::pin!(jobs.join_next_with_id());
    std::future::poll_fn(|cx| {
        if expired.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Wake::Expired);
        }
        if has_jobs
            && let Poll::Ready(Some(result)) = completed.as_mut().poll(cx)
        {
            return Poll::Ready(Wake::Completed(result));
        }
        if let Poll::Ready(Some(signal)) = incoming.as_mut().poll(cx) {
            return Poll::Ready(Wake::Signal(signal));
        }
        Poll::Pending
    })
    .await
}

fn conflicts(left: &[ResourceAccess], right: &[ResourceAccess]) -> bool {
    left.iter().any(|a| {
        right.iter().any(|b| {
            a.resource() == b.resource()
                && (a.mode() == AccessMode::Write || b.mode() == AccessMode::Write)
        })
    })
}

impl RunningApp {
    /// Create a graph within the existing application session and its owned
    /// async handler scope. The returned runner must be awaited, not detached.
    pub fn execution_graph(
        &self,
        plan: BoundExecutionPlan<AsyncOperation>,
        max_in_flight: NonZeroUsize,
    ) -> Result<ExecutionGraphRunner, GraphRunError> {
        ExecutionGraphRunner::new(plan, self.session_token(), max_in_flight)
    }
}
