//! Issue #83 research-only generic execution-graph experiment.
//! This is NOT a production scheduler, stable execution format or Riesel API.
//! Uses one Tokio session executor, never an independent long-lived runtime.
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc::SyncSender};

pub type OperationFuture = Pin<Box<dyn Future<Output = Result<(), &'static str>> + Send>>;
pub type Operation = Arc<dyn Fn() -> OperationFuture + Send + Sync>;

pub fn operation<F, Fut>(f: F) -> Operation
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<(), &'static str>> + Send + 'static,
{
    Arc::new(move || Box::pin(f()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read(&'static str),
    Write(&'static str),
    // Serial-admission illustration, NOT an OS-thread-affinity guarantee.
    ExclusiveThread,
}

fn conflicts(left: &[Access], right: &[Access]) -> bool {
    left.iter().any(|a| {
        right.iter().any(|b| match (a, b) {
            (Access::ExclusiveThread, Access::ExclusiveThread) => true,
            (Access::Write(x), Access::Read(y) | Access::Write(y))
            | (Access::Read(x), Access::Write(y)) => x == y,
            _ => false,
        })
    })
}

pub struct Node {
    pub id: &'static str,
    pub depends_on: Vec<&'static str>,
    pub access: Vec<Access>,
    pub signal: Option<&'static str>,
    pub invoke: Operation,
}

impl Node {
    pub fn new(id: &'static str, deps: &[&'static str], access: Vec<Access>, invoke: Operation) -> Self {
        Self { id, depends_on: deps.to_vec(), access, signal: None, invoke }
    }

    pub fn triggered(mut self, signal: &'static str) -> Self {
        self.signal = Some(signal);
        self
    }
}

pub struct Plan {
    nodes: Vec<Node>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InvalidPlan {
    EmptyId,
    DuplicateId(&'static str),
    MissingDependency(&'static str),
    DuplicateDependency(&'static str),
    Cycle,
}

impl Plan {
    pub fn validate(nodes: Vec<Node>) -> Result<Self, InvalidPlan> {
        let mut ids = HashSet::new();
        for node in &nodes {
            if node.id.is_empty() {
                return Err(InvalidPlan::EmptyId);
            }
            if !ids.insert(node.id) {
                return Err(InvalidPlan::DuplicateId(node.id));
            }
        }
        for node in &nodes {
            let mut used = HashSet::new();
            for dep in &node.depends_on {
                if !ids.contains(dep) {
                    return Err(InvalidPlan::MissingDependency(dep));
                }
                if !used.insert(dep) {
                    return Err(InvalidPlan::DuplicateDependency(dep));
                }
            }
        }
        let mut seen = HashSet::new();
        // Topological validation is independent of closure invocation.
        loop {
            let before = seen.len();
            for node in &nodes {
                if node.depends_on.iter().all(|dep| seen.contains(dep)) {
                    seen.insert(node.id);
                }
            }
            if seen.len() == nodes.len() {
                return Ok(Self { nodes });
            }
            if seen.len() == before {
                return Err(InvalidPlan::Cycle);
            }
        }
    }

    pub fn describe(&self) -> Vec<(&'static str, Vec<&'static str>, Option<&'static str>)> {
        self.nodes.iter().map(|n| (n.id, n.depends_on.clone(), n.signal)).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
    Blocked,
}

#[derive(Debug, Clone)]
pub struct Event {
    pub sequence: u64,
    pub session: u64,
    pub run: u64,
    pub node: &'static str,
    pub attempt: u64,
    pub status: Status,
}

/// Loss-aware bounded diagnostic sink. Neither trace nor subscriber is authoritative.
pub struct Trace {
    pub enabled: bool,
    pub capacity: usize,
    pub retained: VecDeque<Event>,
    pub evicted: u64,
    pub dropped_on_send: u64,
    pub subscriber_disconnected: u64,
    next_sequence: u64,
    subscriber: Option<SyncSender<Event>>,
}

impl Trace {
    pub fn new(enabled: bool, capacity: usize) -> Self {
        Self {
            enabled, capacity, retained: VecDeque::new(), evicted: 0,
            dropped_on_send: 0, subscriber_disconnected: 0,
            next_sequence: 0, subscriber: None,
        }
    }

    pub fn subscribe(&mut self, sender: SyncSender<Event>) {
        self.subscriber = Some(sender);
    }

    fn transition(&mut self, session: u64, run: u64, node: &'static str, attempt: u64, status: Status) {
        if !self.enabled {
            return;
        }
        self.next_sequence += 1;
        let event = Event { sequence: self.next_sequence, session, run, node, attempt, status };
        if self.capacity == 0 {
            self.evicted += 1;
        } else {
            if self.retained.len() == self.capacity {
                self.retained.pop_front();
                self.evicted += 1;
            }
            self.retained.push_back(event.clone());
        }
        if let Some(sender) = &self.subscriber {
            match sender.try_send(event) {
                Ok(()) => {},
                Err(std::sync::mpsc::TrySendError::Full(_)) => self.dropped_on_send += 1,
                Err(std::sync::mpsc::TrySendError::Disconnected(_)) => {
                    self.subscriber_disconnected += 1;
                    self.subscriber = None;
                }
            }
        }
    }
}

/// A run is local to a Riesel application session; caller owns and closes it.
/// Step advances only at operation boundaries. In-flight calls are never preempted.
pub struct Run {
    plan: Arc<Plan>,
    state: Vec<Status>,
    attempts: Vec<u64>,
    signals: BTreeSet<&'static str>,
    session: u64,
    run: u64,
    attempt: u64,
    max_in_flight: usize,
    pub trace: Trace,
    cancellation: Arc<AtomicBool>,
    pub peak_in_flight: usize,
    pub launched: usize,
}

impl Run {
    pub fn new(plan: Arc<Plan>, session: u64, run: u64, max_in_flight: usize, trace: Trace) -> Self {
        let n = plan.nodes.len();
        Self {
            plan, state: vec![Status::Pending; n], attempts: vec![0; n], signals: BTreeSet::new(),
            session, run, attempt: 0, max_in_flight: max_in_flight.max(1), trace,
            cancellation: Arc::new(AtomicBool::new(false)), peak_in_flight: 0, launched: 0,
        }
    }

    pub fn state(&self, id: &str) -> Option<Status> {
        self.plan.nodes.iter().position(|n| n.id == id).map(|idx| self.state[idx])
    }

    pub fn snapshot(&self) -> Vec<(&'static str, Status)> {
        self.plan.nodes.iter().zip(&self.state).map(|(n, s)| (n.id, *s)).collect()
    }

    pub fn fire(&mut self, signal: &'static str) {
        self.signals.insert(signal);
    }

    pub fn cancel(&self) {
        self.cancellation.store(true, Ordering::Release);
    }

    pub async fn advance(&mut self, budget: usize) {
        let mut jobs = tokio::task::JoinSet::<(usize, Result<(), &'static str>)>::new();
        let mut active = Vec::<usize>::new();
        let mut task_indices = HashMap::<tokio::task::Id, usize>::new();
        let mut remaining = budget;
        let mut failed = false;
        loop {
            if !failed && !self.cancellation.load(Ordering::Acquire) {
                for idx in 0..self.state.len() {
                    if remaining == 0 || active.len() >= self.max_in_flight {
                        break;
                    }
                    let node = &self.plan.nodes[idx];
                    if self.state[idx] != Status::Pending
                        || node.signal.is_some_and(|s| !self.signals.contains(s))
                        || !node.depends_on.iter().all(|d| self.state[
                            self.plan.nodes.iter().position(|other| other.id == *d)
                                .expect("validated dependency")
                        ] == Status::Completed)
                        || active.iter().any(|&i| conflicts(&node.access, &self.plan.nodes[i].access))
                    {
                        continue;
                    }
                    self.state[idx] = Status::Running;
                    self.attempt += 1;
                    self.attempts[idx] = self.attempt;
                    self.launched += 1;
                    remaining -= 1;
                    self.trace.transition(self.session, self.run, node.id, self.attempt, Status::Running);
                    let invoke = Arc::clone(&node.invoke);
                    let handle = jobs.spawn(async move { (idx, (invoke)().await) });
                    task_indices.insert(handle.id(), idx);
                    active.push(idx);
                    self.peak_in_flight = self.peak_in_flight.max(active.len());
                }
            }
            if active.is_empty() {
                break;
            }
            // Report failed/panicking workers as failed operations, rather than
            // allowing an unwound task to destroy the inspector itself.
            let joined = jobs.join_next_with_id().await.expect("tracked job");
            let (idx, status) = match joined {
                Ok((task_id, (idx, outcome))) => {
                    task_indices.remove(&task_id);
                    (idx, if outcome.is_ok() { Status::Completed } else { Status::Failed })
                }
                Err(error) => {
                    let idx = task_indices.remove(&error.id()).expect("tracked panic identity");
                    (idx, Status::Failed)
                }
            };
            active.retain(|i| *i != idx);
            if status == Status::Failed {
                failed = true;
            }
            self.state[idx] = status;
            self.trace.transition(
                self.session, self.run, self.plan.nodes[idx].id, self.attempts[idx], status
            );
        }
        if failed || self.cancellation.load(Ordering::Acquire) {
            for idx in 0..self.state.len() {
                if self.state[idx] == Status::Pending {
                    self.state[idx] = if failed { Status::Blocked } else { Status::Cancelled };
                    self.trace.transition(self.session, self.run, self.plan.nodes[idx].id, 0, self.state[idx]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod differential;
#[cfg(test)]
mod live_control;
