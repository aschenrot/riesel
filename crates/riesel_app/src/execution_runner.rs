//! Bounded, session-scoped execution of already-validated Rust operation plans.
//!
//! The runner is owned by one awaited application future. Its Tokio JoinSet
//! cannot outlive that future: there is no independent runtime, supervisor,
//! provider resolver or ECS scheduler here. The optional async-tcp feature
//! supplies the existing Tokio session infrastructure.
//!
//! This coordinator admits one-shot operations, explicit signals, declared
//! domain boundaries and resource conflicts. Its session-local observer
//! publishes authoritative run state. An attempt-scoped confirmed-prefix
//! reporter is an incremental #113 facility; cooperative control remains open.
use crate::execution_observation::{
    GraphEffectRecorder, GraphRunController, GraphRunIdentity, GraphRunObserver,
    GraphRunPublication,
};
use crate::execution_plan::{
    AccessMode, BoundExecutionPlan, DomainAdmissionMode, DomainBoundary, OperationTrigger,
    ResourceAccess,
};
use crate::runtime::RunningApp;
use riesel_protocol::SessionToken;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::num::NonZeroUsize;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use thiserror::Error;
use tokio::sync::{Notify, mpsc};
use tokio::task::{Id, JoinSet};
use uuid::Uuid;

const MAX_PARALLEL_OPERATIONS: usize = 32;
const MAX_SESSION_PARALLEL_OPERATIONS: usize = 32;
const MAX_SESSION_RUNS: usize = 16;
const MAX_PENDING_SIGNALS: usize = 64;

/// One shared admission authority belongs to the existing RunningApp session.
/// Separate runs must not each claim their own 32-operation budget or run
/// conflicting declared accesses without coordination.
pub(crate) struct SessionGraphAdmission {
    state: Mutex<SessionGraphState>,
    changed: Arc<Notify>,
    // Runtime-issued, in-process scope identity for every run in this
    // application generation; not an external identity or credential.
    session_generation: Uuid,
}

impl Default for SessionGraphAdmission {
    fn default() -> Self {
        Self {
            state: Mutex::new(SessionGraphState::default()),
            changed: Arc::new(Notify::new()),
            session_generation: Uuid::new_v4(),
        }
    }
}

#[derive(Default)]
struct SessionGraphState {
    live_runs: usize,
    next_run_sequence: u64,
    next_ticket: u64,
    operations: BTreeMap<u64, OperationReservation>,
    // One declared outer admission authority per named domain while any run
    // that uses it is outstanding. Not a credential or an ECS-system owner.
    domains: BTreeMap<String, (DomainAdmissionMode, usize)>,
}

struct OperationReservation {
    accesses: Vec<ResourceAccess>,
    domain: Option<String>,
}

struct GraphRunLease {
    session: Arc<SessionGraphAdmission>,
    domains: BTreeMap<String, DomainAdmissionMode>,
    run_sequence: u64,
}

impl Drop for GraphRunLease {
    fn drop(&mut self) {
        let mut state = self
            .session
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        debug_assert!(state.live_runs > 0);
        state.live_runs -= 1;
        for (domain, mode) in &self.domains {
            let remove = {
                let (registered_mode, count) = state
                    .domains
                    .get_mut(domain)
                    .expect("claimed domain has a retained authority");
                debug_assert_eq!(registered_mode, mode);
                debug_assert!(*count > 0);
                *count -= 1;
                *count == 0
            };
            if remove {
                state.domains.remove(domain);
            }
        }
    }
}

/// A drop guard survives even if the run future is cancelled or an operation
/// panics. Release first, then wake other runs without holding the mutex.
struct GraphOperationLease {
    session: Arc<SessionGraphAdmission>,
    ticket: u64,
}

impl Drop for GraphOperationLease {
    fn drop(&mut self) {
        let mut state = self
            .session
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let removed = state
            .operations
            .remove(&self.ticket)
            .expect("active operation reservation");
        // An abruptly dropped graph runner does not synchronously drain its
        // JoinSet. Preserve the domain mode until the operation future itself
        // has been aborted and actually released this lease.
        if let Some(domain) = removed.domain {
            let remove = {
                let (_, owners) = state
                    .domains
                    .get_mut(&domain)
                    .expect("operation pins its declared domain mode");
                debug_assert!(*owners > 0);
                *owners -= 1;
                *owners == 0
            };
            if remove {
                state.domains.remove(&domain);
            }
        }
        drop(state);
        self.session.changed.notify_waiters();
    }
}

impl SessionGraphAdmission {
    fn claim_run(
        self: &Arc<Self>,
        plan: &BoundExecutionPlan<AsyncOperation>,
    ) -> Result<GraphRunLease, GraphRunError> {
        // A validated plan has exactly one mode for every declared domain.
        // Claim the complete plan's authorities atomically before allowing
        // any operation to run, not one prefix at a time.
        let domains: BTreeMap<_, _> = plan
            .plan()
            .description()
            .operations()
            .iter()
            .filter_map(|node| {
                node.domain_boundary()
                    .map(|boundary| (boundary.domain().to_owned(), boundary.mode()))
            })
            .collect();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.live_runs >= MAX_SESSION_RUNS {
            return Err(GraphRunError::SessionRunLimitExceeded {
                limit: MAX_SESSION_RUNS,
            });
        }
        for (domain, requested) in &domains {
            if let Some((existing, _)) = state.domains.get(domain)
                && existing != requested
            {
                return Err(GraphRunError::ConflictingDomainAuthority {
                    domain: domain.clone(),
                    existing: *existing,
                    requested: *requested,
                });
            }
        }
        let next_sequence = state
            .next_run_sequence
            .checked_add(1)
            .ok_or(GraphRunError::RunIdentityExhausted)?;
        for (domain, mode) in &domains {
            let (_, owners) = state.domains.entry(domain.clone()).or_insert((*mode, 0));
            *owners += 1;
        }
        state.next_run_sequence = next_sequence;
        state.live_runs += 1;
        Ok(GraphRunLease {
            session: Arc::clone(self),
            domains,
            run_sequence: next_sequence,
        })
    }

    fn reserve(
        self: &Arc<Self>,
        accesses: &[ResourceAccess],
        domain: Option<&DomainBoundary>,
    ) -> Option<GraphOperationLease> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.operations.len() >= MAX_SESSION_PARALLEL_OPERATIONS
            || state.operations.values().any(|active| {
                conflicts(&active.accesses, accesses)
                    || domain
                        .is_some_and(|boundary| active.domain.as_deref() == Some(boundary.domain()))
            })
        {
            return None;
        }
        // Wraparound must not reuse the identity of a live reservation.
        while state.operations.contains_key(&state.next_ticket) {
            state.next_ticket = state.next_ticket.wrapping_add(1);
        }
        let ticket = state.next_ticket;
        state.next_ticket = ticket.wrapping_add(1);
        state.operations.insert(
            ticket,
            OperationReservation {
                accesses: accesses.to_vec(),
                domain: domain.map(|boundary| boundary.domain().to_owned()),
            },
        );
        if let Some(boundary) = domain {
            let (registered_mode, count) = state
                .domains
                .get_mut(boundary.domain())
                .expect("run must claim domain authority before operation admission");
            debug_assert_eq!(*registered_mode, boundary.mode());
            *count += 1;
        }
        Some(GraphOperationLease {
            session: Arc::clone(self),
            ticket,
        })
    }
}

/// A registered operation does not own a Tokio runtime or an ECS world.
/// Delegated thread-affine work must remain in its domain-owned executor.
pub type OperationFuture = Pin<Box<dyn Future<Output = OperationOutcome> + Send + 'static>>;
pub type AsyncOperation = dyn Fn(GraphEffectRecorder) -> OperationFuture + Send + Sync + 'static;

/// Adapt a Rust async closure to the exact-version operation registry.
/// The closure is not invoked by validation or binding.
pub fn async_operation<F, Fut>(operation: F) -> Arc<AsyncOperation>
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = OperationOutcome> + Send + 'static,
{
    Arc::new(move |_effects| Box::pin(operation()) as OperationFuture)
}

/// Adapt an async operation that explicitly reports an owner-confirmed
/// cumulative effect prefix during its own dispatched attempt. Legacy
/// `async_operation` callbacks remain valid and record no confirmed effects.
/// A completed outcome alone is *not* proof of any external effect.
pub fn async_operation_with_effects<F, Fut>(operation: F) -> Arc<AsyncOperation>
where
    F: Fn(GraphEffectRecorder) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = OperationOutcome> + Send + 'static,
{
    Arc::new(move |effects| Box::pin(operation(effects)) as OperationFuture)
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
    #[error("the application session already has {limit} outstanding graph runs")]
    SessionRunLimitExceeded { limit: usize },
    #[error("application session exhausted its graph run identity sequence")]
    RunIdentityExhausted,
    #[error(
        "domain {domain} already has {existing:?} admission authority; cannot also claim {requested:?}"
    )]
    ConflictingDomainAuthority {
        domain: String,
        existing: DomainAdmissionMode,
        requested: DomainAdmissionMode,
    },
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

/// A completed local execution report; its identity also identifies the
/// session-owned authoritative in-process snapshot retained by an observer.
#[derive(Debug, Clone)]
pub struct GraphRunReport {
    pub identity: GraphRunIdentity,
    pub plan_id: String,
    pub revision: u64,
    pub operations: BTreeMap<String, OperationState>,
    /// Owner-confirmed cumulative prefixes by operation and named stream.
    /// A failed, cancelled or outcome-unknown attempt may still have effects.
    /// These values are not a durable record or proof of unconfirmed effects.
    pub confirmed_prefixes: BTreeMap<String, BTreeMap<String, u64>>,
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

/// Failed structured background graphs retain their local post-run result
/// under the existing named application-task error.
#[derive(Debug, Error)]
#[error("background execution graph did not complete successfully")]
pub struct GraphExecutionFailure {
    pub report: GraphRunReport,
}

/// Consumed by execute(): aborting/dropping its future cannot leave a
/// reusable runner containing stale Running nodes. In-flight operations are
/// aborted on session invalidation, but effects already dispatched may be
/// irreversible; they are conservatively reported as OutcomeUnknown.
// RESEARCH-ONLY instrumentation for #226. This branch must not merge these
// counters as an unapproved public scheduler API or production feature.
#[derive(Default)]
struct ResearchScanCounts {
    admission_passes: u64,
    admission_visits: u64,
    blocked_passes: u64,
    blocked_visits: u64,
}

pub struct ExecutionGraphRunner {
    research_scans: ResearchScanCounts,
    plan: BoundExecutionPlan<AsyncOperation>,
    session: SessionToken,
    admission: Arc<SessionGraphAdmission>,
    // Drop this before the domain lease: an abruptly abandoned coordinator
    // must publish conservative terminal states before another owner enters.
    publication: GraphRunPublication,
    _run_lease: GraphRunLease,
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
    pub(crate) fn new(
        plan: BoundExecutionPlan<AsyncOperation>,
        session: SessionToken,
        admission: Arc<SessionGraphAdmission>,
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
        let run_lease = admission.claim_run(&plan)?;
        let description = plan.plan().description();
        let operations = description.operations();
        let identity = GraphRunIdentity::new(
            admission.session_generation,
            run_lease.run_sequence,
            description.id().to_owned(),
            description.revision(),
        );
        let publication = GraphRunPublication::new(identity, operations, session.clone());
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
            research_scans: ResearchScanCounts::default(),
            plan,
            session: session.clone(),
            admission,
            publication,
            _run_lease: run_lease,
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

    /// Read authoritative live state and the bounded transition ring for this
    /// exact run. The handle cannot admit work, control safe points or
    /// authenticate another process. It may outlive the completed runner.
    pub fn observer(&self) -> GraphRunObserver {
        self.publication.observer()
    }

    /// Cooperatively gate future graph admission at this run's advertised
    /// outer operation and Riesel-owned domain-tick boundaries. The returned
    /// handle is in-process, exact-run/session-bound and may be cloned before
    /// the runner is moved to an application-owned async task. It cannot
    /// suspend already dispatched callbacks or step delegated ECS internals.
    pub fn controller(&self) -> GraphRunController {
        self.publication
            .controller(Arc::clone(&self.admission.changed))
    }

    pub(crate) fn belongs_to_session(&self, session: &SessionToken) -> bool {
        self.session.shares_session_with(session)
    }

    /// Execute a single immutable plan revision once. Waiting for an external
    /// signal is intentional; the owning application may cancel its session.
    /// Calls are never retried, and other branches remain eligible while a
    /// signal-gated domain tick is waiting.
    pub async fn execute(self) -> GraphRunReport {
        self.execute_until(std::future::pending()).await.0
    }

    /// A structured owner can request cooperative shutdown and await all
    /// graph-owned Tokio child futures before application provider teardown.
    pub(crate) async fn execute_until<F: Future<Output = ()>>(
        mut self,
        stop: F,
    ) -> (GraphRunReport, bool) {
        let mut jobs = JoinSet::<(usize, OperationOutcome)>::new();
        let mut active = HashMap::<Id, usize>::new();
        let mut session_expired = false;
        let mut stopped_by_owner = false;
        let mut stop = std::pin::pin!(stop);
        self.publication.start();

        loop {
            if std::future::poll_fn(|cx| std::task::Poll::Ready(stop.as_mut().poll(cx).is_ready()))
                .await
            {
                stopped_by_owner = true;
                break;
            }
            // Register for a release notification before examining capacity.
            // No other run can free a slot between our failed admission and
            // our waiter becoming visible.
            let admission = Arc::clone(&self.admission);
            let mut admission_changed = std::pin::pin!(admission.changed.notified());
            admission_changed.as_mut().enable();
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
                admission_changed,
                stop.as_mut(),
            )
            .await
            {
                Wake::Stopping => {
                    stopped_by_owner = true;
                    break;
                }
                Wake::Expired => {
                    session_expired = true;
                    break;
                }
                Wake::Signal(signal) => {
                    self.fired.insert(signal);
                }
                Wake::AdmissionReleased => {}
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
                        self.publication
                            .transition(index, self.states[index].clone());
                    }
                    Err(error) => {
                        let index = active
                            .remove(&error.id())
                            .expect("tracked operation task identity");
                        self.states[index] = OperationState::Failed(error.to_string());
                        self.publication
                            .transition(index, self.states[index].clone());
                    }
                },
            }
        }

        if session_expired || stopped_by_owner {
            jobs.abort_all();
            while jobs.join_next().await.is_some() {}
            for index in active.into_values() {
                // The body may have dispatched external effects before abort.
                self.states[index] = OperationState::OutcomeUnknown;
                self.publication
                    .transition(index, OperationState::OutcomeUnknown);
            }
            self.block_dependents();
            for (index, state) in self.states.iter_mut().enumerate() {
                if matches!(state, OperationState::Pending) {
                    *state = OperationState::Cancelled;
                    self.publication
                        .transition(index, OperationState::Cancelled);
                }
            }
        }

        let identity = self.publication.identity();
        self.publication.finish(session_expired);
        if std::env::var_os("RIESEL_SCHEDULER_RESEARCH_SCAN_COUNTS").is_some() {
            eprintln!(
                "research #226 scan plan={} nodes={} admit_passes={} admit_visits={} block_passes={} block_visits={} started={}",
                self.plan.plan().description().id(),
                self.states.len(),
                self.research_scans.admission_passes,
                self.research_scans.admission_visits,
                self.research_scans.blocked_passes,
                self.research_scans.blocked_visits,
                self.started,
            );
        }
        let confirmed_prefixes = self
            .publication
            .observer()
            .snapshot()
            .operations
            .into_iter()
            .filter_map(|(name, operation)| {
                (!operation.confirmed_prefixes.is_empty())
                    .then_some((name, operation.confirmed_prefixes))
            })
            .collect();
        let description = self.plan.plan().description();
        let operations = description
            .operations()
            .iter()
            .zip(self.states)
            .map(|(node, state)| (node.id().to_owned(), state))
            .collect();
        (
            GraphRunReport {
                identity,
                plan_id: description.id().to_owned(),
                revision: description.revision(),
                operations,
                confirmed_prefixes,
                started: self.started,
                peak_in_flight: self.peak_in_flight,
                session_expired,
            },
            stopped_by_owner,
        )
    }

    fn admit(
        &mut self,
        jobs: &mut JoinSet<(usize, OperationOutcome)>,
        active: &mut HashMap<Id, usize>,
    ) {
        let operations = self.plan.plan().description().operations();
        self.research_scans.admission_passes += 1;
        for (index, node) in operations.iter().enumerate() {
            if active.len() >= self.max_in_flight {
                break;
            }
            self.research_scans.admission_visits += 1;
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
            // Check the local safe-point gate before claiming scarce
            // session capacity. The final admission rechecks atomically
            // against concurrent pause/step commands after reservation.
            if !self.publication.may_admit(index, node.domain_boundary()) {
                continue;
            }
            // The one session-level admission table enforces conflicts for
            // both this run and every other run in the same application.
            let Some(lease) = self
                .admission
                .reserve(node.resources(), node.domain_boundary())
            else {
                continue;
            };
            let handler = Arc::clone(
                self.plan
                    .handler(node.id())
                    .expect("validated exact-version operation binding"),
            );
            // One authoritative lock atomically consumes the admitted
            // safe-point step and marks the callback Running, so a pause
            // cannot retroactively claim to stop already dispatched work.
            if !self.publication.admit(index, node.domain_boundary()) {
                drop(lease);
                continue;
            }
            self.states[index] = OperationState::Running;
            let (effects, effect_scope) = self.publication.recorder(index);
            let session = self.session.clone();
            let handle = jobs.spawn(async move {
                let _lease = lease;
                // Drop before the operation lease; no leaked reporter can
                // publish after this child future finishes or is aborted.
                let _effect_scope = effect_scope;
                if !session.is_active() {
                    (index, OperationOutcome::Cancelled)
                } else {
                    (index, handler(effects).await)
                }
            });
            active.insert(handle.id(), index);
            self.started += 1;
            self.peak_in_flight = self.peak_in_flight.max(active.len());
        }
    }

    fn block_dependents(&mut self) {
        // Every prerequisite precedes its dependents in the validated plan.
        // A single pass therefore propagates terminal blocking through even
        // a reverse-declared chain; repeated whole-plan scans are unnecessary.
        self.research_scans.blocked_passes += 1;
        for node in self.plan.plan().dependency_order() {
            self.research_scans.blocked_visits += 1;
            let index = *self.index.get(node.id()).expect("validated operation id");
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
                self.publication.transition(index, OperationState::Blocked);
            }
        }
    }
}

enum Wake {
    Stopping,
    Expired,
    Signal(String),
    AdmissionReleased,
    Completed(Result<(Id, (usize, OperationOutcome)), tokio::task::JoinError>),
}

/// Mirror the existing async-task poll_fn cancellation pattern; this does
/// not require Tokio's optional macros feature. Recreate neither Notify
/// subscription nor the join future on every poll. Finished operations are
/// polled before the signal queue, so a busy producer cannot starve cleanup.
async fn wait_for_work<F: Future<Output = ()>, S: Future<Output = ()>>(
    session: &SessionToken,
    signals: &mut mpsc::Receiver<String>,
    jobs: &mut JoinSet<(usize, OperationOutcome)>,
    has_jobs: bool,
    changed: F,
    mut stop: Pin<&mut S>,
) -> Wake {
    use std::task::Poll;

    let mut expired = std::pin::pin!(session.cancelled());
    let mut incoming = std::pin::pin!(signals.recv());
    let mut completed = std::pin::pin!(jobs.join_next_with_id());
    let mut changed = std::pin::pin!(changed);
    std::future::poll_fn(|cx| {
        if stop.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Wake::Stopping);
        }
        if expired.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Wake::Expired);
        }
        if has_jobs && let Poll::Ready(Some(result)) = completed.as_mut().poll(cx) {
            return Poll::Ready(Wake::Completed(result));
        }
        if let Poll::Ready(Some(signal)) = incoming.as_mut().poll(cx) {
            return Poll::Ready(Wake::Signal(signal));
        }
        if changed.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Wake::AdmissionReleased);
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
        ExecutionGraphRunner::new(
            plan,
            self.session_token(),
            Arc::clone(&self.graph_admission),
            max_in_flight,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_plan::{
        ExecutionPlanDraft, OperationKey, OperationNode, OperationRegistry,
    };

    fn plan(mode: DomainAdmissionMode) -> BoundExecutionPlan<AsyncOperation> {
        let key = OperationKey::new("test.tick", 1).unwrap();
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key.clone(),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        let node = OperationNode::new("operation", key);
        let (id, node) = match mode {
            DomainAdmissionMode::RieselTick => ("central", node.domain_tick("world")),
            DomainAdmissionMode::DelegatedFrame => ("delegated", node.delegated_frame("world")),
        };
        ExecutionPlanDraft::new(id, 1)
            .operation(node)
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap()
    }

    #[test]
    fn abandoned_runner_does_not_release_domain_mode_while_child_lease_is_active() {
        let admission = Arc::new(SessionGraphAdmission::default());
        let central = plan(DomainAdmissionMode::RieselTick);
        let delegated = plan(DomainAdmissionMode::DelegatedFrame);

        let claimed_run = admission.claim_run(&central).unwrap();
        let tick = central.plan().description().operations()[0]
            .domain_boundary()
            .unwrap();
        let child_operation = admission.reserve(&[], Some(tick)).unwrap();

        // Dropping the outer runner (for instance an abruptly cancelled
        // Tokio future) is not proof all spawned operation futures have
        // dropped. A mode change is unsafe until the last child lease goes.
        drop(claimed_run);
        assert!(matches!(
            admission.claim_run(&delegated),
            Err(GraphRunError::ConflictingDomainAuthority {
                existing: DomainAdmissionMode::RieselTick,
                requested: DomainAdmissionMode::DelegatedFrame,
                ..
            })
        ));
        drop(child_operation);
        let delegated_run = admission.claim_run(&delegated).unwrap();
        drop(delegated_run);
        assert!(admission.state.lock().unwrap().domains.is_empty());
    }
}
