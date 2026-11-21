//! Fixed-tick domain ownership acceptance fixture for Issue #112.
//!
//! The two execution policies drive the same non-Send world and same bounded
//! two-tick result: centrally admitted *outer* ticks versus a domain-owned
//! frame with cooperative permits. No ECS systems are ever rescheduled here.
//! This fixture does not define a published clock/control protocol.
#![cfg(feature = "async-tcp")]
use riesel::{
    AccessMode, App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft,
    GraphRunReport, OperationKey, OperationNode, OperationOutcome, OperationRegistry,
    OperationState, RunningApp, async_operation,
};
use std::cell::Cell;
use std::num::NonZeroUsize;
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, Receiver, SyncSender},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio::sync::{Barrier, mpsc as async_mpsc, oneshot};

const SESSION_EPOCH: u64 = 71;
const PROPOSED_TICKS: usize = 4;
const CATCH_UP_CAP: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TickOwner {
    Central,
    Domain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Snapshot {
    tick: usize,
    position: i32,
}

impl Snapshot {
    fn initial() -> Self {
        Self { tick: 0, position: 0 }
    }

    fn next(self) -> Self {
        Self {
            tick: self.tick + 1,
            position: self.position + 2,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct FrameOutcome {
    last: Snapshot,
    committed_ticks: usize,
    cancelled: bool,
}

enum Permit {
    Advance { epoch: u64, response: oneshot::Sender<bool> },
    Cancel { epoch: u64, response: oneshot::Sender<bool> },
}

enum DomainCommand {
    Tick {
        requested: TickOwner,
        response: oneshot::Sender<Result<Snapshot, &'static str>>,
    },
    Frame {
        requested: TickOwner,
        permits: Receiver<Permit>,
        snapshots: async_mpsc::Sender<Snapshot>,
        response: oneshot::Sender<Result<FrameOutcome, &'static str>>,
    },
    Stop,
}

struct DomainActor {
    commands: SyncSender<DomainCommand>,
    worker: JoinHandle<Snapshot>,
}

impl DomainActor {
    fn start(owner: TickOwner) -> Self {
        let (commands, receiver) = mpsc::sync_channel::<DomainCommand>(4);
        let worker = thread::spawn(move || {
            // Construct the non-Send world on its own thread. No Riesel async
            // callback can move Rc<Cell<...>> into a Tokio worker.
            let world = Rc::new(Cell::new(Snapshot::initial()));
            while let Ok(command) = receiver.recv() {
                match command {
                    DomainCommand::Tick { requested, response } => {
                        let result = if owner == TickOwner::Central && requested == owner {
                            let next = world.get().next();
                            world.set(next);
                            Ok(next)
                        } else {
                            Err("tick admission authority mismatch")
                        };
                        let _ = response.send(result);
                    }
                    DomainCommand::Frame {
                        requested,
                        permits,
                        snapshots,
                        response,
                    } => {
                        if owner != TickOwner::Domain || requested != owner {
                            let _ = response.send(Err("frame admission authority mismatch"));
                            continue;
                        }
                        let mut committed_ticks = 0;
                        let mut cancelled = false;
                        let mut failed = None;
                        while committed_ticks < PROPOSED_TICKS.min(CATCH_UP_CAP) {
                            match permits.recv_timeout(Duration::from_secs(3)) {
                                Ok(Permit::Advance { epoch, response }) => {
                                    if epoch != SESSION_EPOCH {
                                        let _ = response.send(false);
                                        continue;
                                    }
                                    let next = world.get().next();
                                    world.set(next);
                                    committed_ticks += 1;
                                    if snapshots.try_send(next).is_err() {
                                        failed = Some("domain snapshot channel unavailable");
                                        let _ = response.send(false);
                                        break;
                                    }
                                    let _ = response.send(true);
                                }
                                Ok(Permit::Cancel { epoch, response }) => {
                                    if epoch != SESSION_EPOCH {
                                        let _ = response.send(false);
                                        continue;
                                    }
                                    cancelled = true;
                                    let _ = response.send(true);
                                    break;
                                }
                                Err(_) => {
                                    failed = Some("no domain-owned safe-point permit");
                                    break;
                                }
                            }
                        }
                        let outcome = if let Some(message) = failed {
                            Err(message)
                        } else {
                            Ok(FrameOutcome {
                                last: world.get(),
                                committed_ticks,
                                cancelled,
                            })
                        };
                        let _ = response.send(outcome);
                    }
                    DomainCommand::Stop => break,
                }
            }
            world.get()
        });
        Self { commands, worker }
    }

    fn stop(self) -> Snapshot {
        self.commands.try_send(DomainCommand::Stop).unwrap();
        self.worker.join().unwrap()
    }
}

fn key(name: &str) -> OperationKey {
    OperationKey::new(name, 1).unwrap()
}

fn application_session() -> RunningApp {
    App::from_schema(
        r#"{"version":1,"app":{"name":"FixedDomainFixture"},"models":{},"requires":[]}"#,
    )
    .unwrap()
    .start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
    .unwrap()
}

fn executor() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn register_workflow(
    registry: &mut OperationRegistry<AsyncOperation>,
    completed: Arc<AtomicUsize>,
) {
    let barrier = Arc::new(Barrier::new(2));
    for side in ["left", "right"] {
        let barrier = Arc::clone(&barrier);
        let completed = Arc::clone(&completed);
        registry
            .register(
                key(side),
                async_operation(move || {
                    let barrier = Arc::clone(&barrier);
                    let completed = Arc::clone(&completed);
                    async move {
                        barrier.wait().await;
                        completed.fetch_add(1, Ordering::SeqCst);
                        OperationOutcome::Completed
                    }
                }),
            )
            .unwrap();
    }
    for name in ["input", "join", "final"] {
        registry
            .register(
                key(name),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
    }
}

fn with_async_branches(mut plan: ExecutionPlanDraft, terminal: &str) -> ExecutionPlanDraft {
    plan = plan.operation(OperationNode::new("input", key("input")));
    plan = plan.operation(OperationNode::new("left", key("left")).after("input"));
    plan = plan.operation(OperationNode::new("right", key("right")).after("input"));
    plan = plan.operation(
        OperationNode::new("join", key("join"))
            .after("left")
            .after("right"),
    );
    plan.operation(
        OperationNode::new("final", key("final"))
            .after("join")
            .after(terminal),
    )
}

async fn wait_for_count(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while counter.load(Ordering::SeqCst) < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn central_policy() -> (Vec<Snapshot>, GraphRunReport) {
    let domain = DomainActor::start(TickOwner::Central);
    let snapshots = Arc::new(Mutex::new(Vec::<Snapshot>::new()));
    let joined = Arc::new(AtomicUsize::new(0));
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    register_workflow(&mut registry, Arc::clone(&joined));
    let tick_handler = async_operation({
        let commands = domain.commands.clone();
        let snapshots = Arc::clone(&snapshots);
        move || {
            let commands = commands.clone();
            let snapshots = Arc::clone(&snapshots);
            async move {
                let (response, reply) = oneshot::channel();
                if commands.try_send(DomainCommand::Tick {
                    requested: TickOwner::Central,
                    response,
                }).is_err() {
                    return OperationOutcome::Failed("domain queue unavailable".into());
                }
                match reply.await {
                    Ok(Ok(snapshot)) => {
                        snapshots.lock().unwrap().push(snapshot);
                        OperationOutcome::Completed
                    }
                    Ok(Err(error)) => OperationOutcome::Failed(error.into()),
                    Err(_) => OperationOutcome::OutcomeUnknown,
                }
            }
        }
    });
    registry.register(key("tick"), tick_handler).unwrap();
    let plan = with_async_branches(
        ExecutionPlanDraft::new("fixed.central", 1)
            .operation(
                OperationNode::new("tick-one", key("tick"))
                    .after("input")
                    .on_signal("tick.one")
                    .access("ecs.world", AccessMode::Write),
            )
            .operation(
                OperationNode::new("tick-two", key("tick"))
                    .after("tick-one")
                    .on_signal("tick.two")
                    .access("ecs.world", AccessMode::Write),
            ),
        "tick-two",
    )
    .validate()
    .unwrap()
    .bind(&registry)
    .unwrap();
    let owner = application_session();
    let runner = owner.execution_graph(plan, NonZeroUsize::new(3).unwrap()).unwrap();
    let signals = runner.signal_sender();
    let task = tokio::spawn(runner.execute());
    wait_for_count(&joined, 2).await;
    assert!(snapshots.lock().unwrap().is_empty());
    assert!(!task.is_finished());

    signals.fire("tick.one").unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        while snapshots.lock().unwrap().len() < 1 {
            tokio::task::yield_now().await;
        }
    }).await.unwrap();
    assert_eq!(snapshots.lock().unwrap()[0].tick, 1);
    assert!(!task.is_finished());

    signals.fire("tick.two").unwrap();
    let report = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    assert!(report.succeeded());
    assert_eq!(report.operations["tick-one"], OperationState::Completed);
    assert_eq!(report.operations["tick-two"], OperationState::Completed);
    assert_eq!(report.operations["final"], OperationState::Completed);
    assert_eq!(domain.stop(), Snapshot { tick: 2, position: 4 });
    let snapshots = snapshots.lock().unwrap().clone();
    (snapshots, report)
}

async fn delegated_policy(cancel_after_first: bool) -> (Vec<Snapshot>, GraphRunReport, FrameOutcome) {
    let domain = DomainActor::start(TickOwner::Domain);
    let joined = Arc::new(AtomicUsize::new(0));
    let (permit_tx, permit_rx) = mpsc::sync_channel::<Permit>(2);
    let permits = Arc::new(Mutex::new(Some(permit_rx)));
    let (observation_tx, mut observation_rx) = async_mpsc::channel::<Snapshot>(4);
    let final_frame = Arc::new(Mutex::new(None::<FrameOutcome>));
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    register_workflow(&mut registry, Arc::clone(&joined));
    registry
        .register(
            key("frame"),
            async_operation({
                let commands = domain.commands.clone();
                let permits = Arc::clone(&permits);
                let final_frame = Arc::clone(&final_frame);
                move || {
                    let commands = commands.clone();
                    let permits = permits.lock().unwrap().take().unwrap();
                    let observation_tx = observation_tx.clone();
                    let final_frame = Arc::clone(&final_frame);
                    async move {
                        let (response, reply) = oneshot::channel();
                        if commands.try_send(DomainCommand::Frame {
                            requested: TickOwner::Domain,
                            permits,
                            snapshots: observation_tx,
                            response,
                        }).is_err() {
                            return OperationOutcome::Failed("domain queue unavailable".into());
                        }
                        match reply.await {
                            Ok(Ok(frame)) => {
                                *final_frame.lock().unwrap() = Some(frame);
                                if frame.cancelled {
                                    OperationOutcome::Cancelled
                                } else if frame.committed_ticks == CATCH_UP_CAP {
                                    OperationOutcome::Completed
                                } else {
                                    OperationOutcome::Failed("incomplete frame".into())
                                }
                            }
                            Ok(Err(error)) => OperationOutcome::Failed(error.into()),
                            Err(_) => OperationOutcome::OutcomeUnknown,
                        }
                    }
                }
            }),
        )
        .unwrap();
    let plan = with_async_branches(
        ExecutionPlanDraft::new("fixed.delegated", 1)
            .operation(
                OperationNode::new("frame", key("frame"))
                    .after("input")
                    .access("ecs.world", AccessMode::Write),
            ),
        "frame",
    )
    .validate()
    .unwrap()
    .bind(&registry)
    .unwrap();
    let owner = application_session();
    let runner = owner.execution_graph(plan, NonZeroUsize::new(3).unwrap()).unwrap();
    let task = tokio::spawn(runner.execute());
    wait_for_count(&joined, 2).await;
    assert!(!task.is_finished(), "domain controls its own frame admission");

    let (response, reply) = oneshot::channel();
    permit_tx.send(Permit::Advance { epoch: SESSION_EPOCH - 1, response }).unwrap();
    assert!(!tokio::time::timeout(Duration::from_secs(3), reply).await.unwrap().unwrap());
    assert!(observation_rx.try_recv().is_err(), "stale permit cannot commit a tick");

    let (response, reply) = oneshot::channel();
    permit_tx.send(Permit::Advance { epoch: SESSION_EPOCH, response }).unwrap();
    assert!(reply.await.unwrap());
    let first = tokio::time::timeout(Duration::from_secs(3), observation_rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first, Snapshot { tick: 1, position: 2 });
    assert!(!task.is_finished(), "one permit cannot finish a two-tick frame");

    let (response, reply) = oneshot::channel();
    if cancel_after_first {
        permit_tx.send(Permit::Cancel { epoch: SESSION_EPOCH, response }).unwrap();
    } else {
        permit_tx.send(Permit::Advance { epoch: SESSION_EPOCH, response }).unwrap();
    }
    assert!(reply.await.unwrap());
    let mut observed = vec![first];
    if !cancel_after_first {
        observed.push(
            tokio::time::timeout(Duration::from_secs(3), observation_rx.recv())
                .await
                .unwrap()
                .unwrap(),
        );
    }
    let report = tokio::time::timeout(Duration::from_secs(3), task)
        .await
        .unwrap()
        .unwrap();
    let frame = final_frame.lock().unwrap().unwrap();
    assert_eq!(frame.committed_ticks, if cancel_after_first { 1 } else { 2 });
    assert_eq!(frame.last, domain.stop());
    assert!(!report.operations.contains_key("tick-one"));
    assert!(!report.operations.contains_key("tick-two"));
    (observed, report, frame)
}

#[test]
fn fixed_tick_modes_preserve_world_semantics_with_independent_async_work() {
    executor().block_on(async {
        assert_eq!(PROPOSED_TICKS.min(CATCH_UP_CAP), 2);
        let (central, central_report) = central_policy().await;
        let (delegated, delegated_report, frame) = delegated_policy(false).await;
        assert_eq!(central, delegated);
        assert_eq!(
            central,
            vec![
                Snapshot { tick: 1, position: 2 },
                Snapshot { tick: 2, position: 4 },
            ]
        );
        assert!(central_report.succeeded());
        assert!(delegated_report.succeeded());
        assert_eq!(frame.committed_ticks, 2);
        assert_eq!(delegated_report.operations["frame"], OperationState::Completed);
        assert_eq!(delegated_report.operations["final"], OperationState::Completed);
        assert!(central_report.peak_in_flight >= 2);
        assert!(delegated_report.peak_in_flight >= 2);
    });
}

#[test]
fn partial_domain_frame_cancellation_preserves_committed_tick_and_blocks_final_join() {
    executor().block_on(async {
        let (observed, report, frame) = delegated_policy(true).await;
        assert_eq!(observed, vec![Snapshot { tick: 1, position: 2 }]);
        assert_eq!(frame.committed_ticks, 1);
        assert!(frame.cancelled);
        assert_eq!(frame.last, Snapshot { tick: 1, position: 2 });
        assert_eq!(report.operations["frame"], OperationState::Cancelled);
        assert_eq!(report.operations["final"], OperationState::Blocked);
        assert!(!report.succeeded());
    });
}

#[test]
fn mismatched_domain_admission_owner_rejects_before_world_mutation() {
    executor().block_on(async {
        let domain = DomainActor::start(TickOwner::Domain);
        let (response, reply) = oneshot::channel();
        domain.commands.try_send(DomainCommand::Tick {
            requested: TickOwner::Central,
            response,
        }).unwrap();
        assert_eq!(
            reply.await.unwrap(),
            Err("tick admission authority mismatch")
        );
        assert_eq!(domain.stop(), Snapshot::initial());

        let central = DomainActor::start(TickOwner::Central);
        let (_, permits) = mpsc::sync_channel::<Permit>(1);
        let (observed, _) = async_mpsc::channel::<Snapshot>(2);
        let (response, reply) = oneshot::channel();
        central.commands.try_send(DomainCommand::Frame {
            requested: TickOwner::Domain,
            permits,
            snapshots: observed,
            response,
        }).unwrap();
        assert!(matches!(
            reply.await.unwrap(),
            Err("frame admission authority mismatch")
        ));
        assert_eq!(central.stop(), Snapshot::initial());
    });
}
