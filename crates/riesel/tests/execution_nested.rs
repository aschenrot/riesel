//! Issue #153: a real RunningApp graph with a thread-confined non-ECS owner.
//! This exercises the *production* read-only nested API, not the older
//! research-only actor's independent observation implementation.
#![cfg(feature = "async-tcp")]

use riesel::{
    App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionGraphRunner,
    ExecutionPlanDraft, GraphControlError, GraphControlTarget, GraphRunObserver, NestedCursor,
    NestedDomainOwner, NestedDomainRegistration, NestedObservationError, NestedPublisher,
    NestedStatus, OperationKey, OperationNode, OperationOutcome, OperationRegistry, OperationState,
    RunningApp, MAX_NESTED_TRANSITIONS, async_operation, async_operation_with_effects,
};
use std::{
    cell::Cell,
    num::NonZeroUsize,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tokio::sync::oneshot;

const WAIT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct StageSnapshot {
    stage: u64,
    position: i32,
}

enum Permit {
    Step(oneshot::Sender<()>),
    Cancel(oneshot::Sender<()>),
}

enum NativeCommand {
    Frame {
        publisher: NestedPublisher<StageSnapshot>,
        steps: u64,
        permits: Receiver<Permit>,
        lost_reply: bool,
        response: oneshot::Sender<(u64, bool)>,
    },
    Stop,
}

struct NativeDomain {
    commands: SyncSender<NativeCommand>,
    registration: NestedDomainRegistration,
    worker: JoinHandle<StageSnapshot>,
}

impl NativeDomain {
    fn start(domain: &str) -> Self {
        let (commands, incoming) = mpsc::sync_channel(4);
        let (registration_tx, registration_rx) = mpsc::sync_channel(1);
        let name = domain.to_owned();
        let worker = thread::spawn(move || {
            let owner = NestedDomainOwner::new(name);
            registration_tx.send(owner.registration()).unwrap();
            let native_world = Rc::new(Cell::new(StageSnapshot::default()));
            while let Ok(message) = incoming.recv() {
                match message {
                    NativeCommand::Stop => break,
                    NativeCommand::Frame {
                        publisher,
                        steps,
                        permits,
                        lost_reply,
                        response,
                    } => {
                        let mut completed = 0;
                        let mut cancelled = false;
                        while completed < steps {
                            match permits.recv_timeout(WAIT) {
                                Ok(Permit::Step(ack)) => {
                                    let last = native_world.get();
                                    let next = StageSnapshot {
                                        stage: last.stage + 1,
                                        position: last.position + 2,
                                    };
                                    native_world.set(next);
                                    completed += 1;
                                    // The actual native owner publishes the
                                    // copied value; Tokio never owns its Rc.
                                    publisher.publish(next).unwrap();
                                    let _ = ack.send(());
                                }
                                Ok(Permit::Cancel(ack)) => {
                                    cancelled = true;
                                    let _ = ack.send(());
                                    break;
                                }
                                Err(error) => panic!("missing test domain permit: {error}"),
                            }
                        }
                        if !lost_reply {
                            let _ = response.send((completed, cancelled));
                        }
                    }
                }
            }
            native_world.get()
        });
        let registration = registration_rx.recv().unwrap();
        Self {
            commands,
            registration,
            worker,
        }
    }

    fn stop(self) -> StageSnapshot {
        self.commands.send(NativeCommand::Stop).unwrap();
        self.worker.join().unwrap()
    }
}

fn executor() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn session() -> RunningApp {
    App::from_schema(
        r#"{"version":1,"app":{"name":"NestedOwnerFixture"},"models":{},"requires":[]}"#,
    )
    .unwrap()
    .start(&DeploymentConfig::new(Default::default()), &ConnectorRegistry::new())
    .unwrap()
}

fn key(name: &str) -> OperationKey {
    OperationKey::new(name, 1).unwrap()
}

struct Fixture {
    app: RunningApp,
    domain: NativeDomain,
    graph: ExecutionGraphRunner,
    observer: GraphRunObserver,
    permits: SyncSender<Permit>,
    independent: Arc<AtomicUsize>,
}

fn fixture(steps: u64, lost_reply: bool) -> Fixture {
    let domain = NativeDomain::start("world.owner");
    let (permits, receiver) = mpsc::sync_channel(8);
    let permit_receiver = Arc::new(Mutex::new(Some(receiver)));
    let independent = Arc::new(AtomicUsize::new(0));
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    registry
        .register(
            key("frame"),
            async_operation_with_effects({
                let commands = domain.commands.clone();
                let registration = domain.registration.clone();
                let permit_receiver = Arc::clone(&permit_receiver);
                move |effects| {
                    let commands = commands.clone();
                    let registration = registration.clone();
                    let permits = permit_receiver.lock().unwrap().take().unwrap();
                    async move {
                        let publisher = match effects.attach_nested::<StageSnapshot>(&registration, 1) {
                            Ok(publisher) => publisher,
                            Err(error) => return OperationOutcome::Failed(error.to_string()),
                        };
                        let (response, reply) = oneshot::channel();
                        if commands
                            .send(NativeCommand::Frame {
                                publisher,
                                steps,
                                permits,
                                lost_reply,
                                response,
                            })
                            .is_err()
                        {
                            return OperationOutcome::OutcomeUnknown;
                        }
                        match reply.await {
                            Ok((confirmed, cancelled)) => {
                                if confirmed != 0 {
                                    effects.confirm_prefix("owner.stages", confirmed).unwrap();
                                }
                                if cancelled { OperationOutcome::Cancelled } else { OperationOutcome::Completed }
                            }
                            Err(_) => OperationOutcome::OutcomeUnknown,
                        }
                    }
                }
            }),
        )
        .unwrap();
    registry
        .register(
            key("independent"),
            async_operation({
                let completed = Arc::clone(&independent);
                move || {
                    let completed = Arc::clone(&completed);
                    async move {
                        completed.fetch_add(1, Ordering::SeqCst);
                        OperationOutcome::Completed
                    }
                }
            }),
        )
        .unwrap();
    registry
        .register(
            key("dependent"),
            async_operation(|| async { OperationOutcome::Completed }),
        )
        .unwrap();

    let plan = ExecutionPlanDraft::new("nested.owner.fixture", 3)
        .operation(OperationNode::new("frame", key("frame")).delegated_frame("world.owner"))
        .operation(OperationNode::new("independent", key("independent")))
        .operation(OperationNode::new("dependent", key("dependent")).after("frame"))
        .validate()
        .unwrap()
        .bind(&registry)
        .unwrap();
    let app = session();
    let graph = app.execution_graph(plan, NonZeroUsize::new(3).unwrap()).unwrap();
    let observer = graph.observer();
    Fixture { app, domain, graph, observer, permits, independent }
}

async fn until<F: Fn() -> bool>(f: F) {
    tokio::time::timeout(WAIT, async {
        while !f() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn step(permits: &SyncSender<Permit>) {
    let (tx, rx) = oneshot::channel();
    permits.send(Permit::Step(tx)).unwrap();
    tokio::time::timeout(WAIT, rx).await.unwrap().unwrap();
}

async fn cancel(permits: &SyncSender<Permit>) {
    let (tx, rx) = oneshot::channel();
    permits.send(Permit::Cancel(tx)).unwrap();
    tokio::time::timeout(WAIT, rx).await.unwrap().unwrap();
}

#[test]
fn actual_domain_owner_publication_cancellation_and_read_only_discovery() {
    executor().block_on(async {
        let setup = fixture(2, false);
        assert!(setup.observer.nested_description("frame").is_none());
        assert!(setup.observer.nested::<StageSnapshot>("frame", 1).unwrap().is_none());
        let controller = setup.graph.controller();
        assert!(matches!(
            controller.pause(GraphControlTarget::RieselDomainTicks("world.owner".into())),
            Err(GraphControlError::UnsupportedDomainAuthority { .. })
        ));
        let task = tokio::spawn(setup.graph.execute());
        until(|| setup.observer.nested_description("frame").is_some()).await;
        until(|| setup.independent.load(Ordering::SeqCst) == 1).await;
        let view = setup.observer.nested::<StageSnapshot>("frame", 1).unwrap().unwrap();
        let initial = view.snapshot();
        assert_eq!(initial.status, NestedStatus::Unpublished);
        assert!(initial.payload.is_none(), "no invented initial owner state");
        assert_eq!(initial.description.parent.run, setup.observer.snapshot().identity);
        assert_eq!(initial.description.parent.operation, "frame");
        assert_eq!(initial.description.parent.attempt, setup.observer.snapshot().operations["frame"].attempt);
        assert_eq!(initial.description.parent.run.revision(), 3);
        assert_eq!(initial.description.domain, "world.owner");
        assert_eq!(initial.description.view_version, 1);
        assert!(matches!(
            setup.observer.nested::<StageSnapshot>("frame", 2),
            Err(NestedObservationError::UnsupportedVersion { .. })
        ));
        assert!(matches!(
            setup.observer.nested::<usize>("frame", 1),
            Err(NestedObservationError::WrongPayloadType)
        ));

        step(&setup.permits).await;
        let first = view.snapshot();
        assert_eq!(first.status, NestedStatus::Current);
        assert_eq!(*first.payload.unwrap(), StageSnapshot { stage: 1, position: 2 });
        assert_eq!(first.sequence, 1);
        assert_eq!(setup.observer.snapshot().operations["frame"].state, OperationState::Running);
        cancel(&setup.permits).await;
        let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
        assert_eq!(report.operations["frame"], OperationState::Cancelled);
        assert_eq!(report.operations["dependent"], OperationState::Blocked);
        assert_eq!(report.operations["independent"], OperationState::Completed);
        assert_eq!(report.confirmed_prefixes["frame"]["owner.stages"], 1);
        assert!(!report.operations.contains_key("domain.system"));
        assert_eq!(view.snapshot().description.parent.run, report.identity);
        assert_eq!(setup.domain.stop(), StageSnapshot { stage: 1, position: 2 });
        assert_eq!(view.snapshot().status, NestedStatus::OwnerClosed);
        assert_eq!(*view.snapshot().payload.unwrap(), StageSnapshot { stage: 1, position: 2 });
        assert_eq!(setup.observer.snapshot().operations["frame"].state, OperationState::Cancelled);
        drop(setup.app);
        assert_eq!(view.snapshot().status, NestedStatus::SessionExpired);
    });
}

#[test]
fn lost_outer_reply_does_not_rewrite_inner_owner_snapshots_or_outer_unknown() {
    executor().block_on(async {
        let setup = fixture(2, true);
        let task = tokio::spawn(setup.graph.execute());
        until(|| setup.observer.nested_description("frame").is_some()).await;
        let view = setup.observer.nested::<StageSnapshot>("frame", 1).unwrap().unwrap();
        step(&setup.permits).await;
        step(&setup.permits).await;
        let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
        assert_eq!(report.operations["frame"], OperationState::OutcomeUnknown);
        assert_eq!(report.operations["dependent"], OperationState::Blocked);
        assert!(report.confirmed_prefixes.get("frame").is_none());
        assert_eq!(view.snapshot().sequence, 2);
        assert_eq!(*view.snapshot().payload.unwrap(), StageSnapshot { stage: 2, position: 4 });
        assert_eq!(setup.domain.stop(), StageSnapshot { stage: 2, position: 4 });
        assert_eq!(setup.observer.snapshot().operations["frame"].state, OperationState::OutcomeUnknown);
    });
}

#[test]
fn bounded_snapshot_cursor_overflow_and_owner_generation_fencing() {
    executor().block_on(async {
        let steps = MAX_NESTED_TRANSITIONS as u64 + 9;
        let setup = fixture(steps, false);
        let task = tokio::spawn(setup.graph.execute());
        until(|| setup.observer.nested_description("frame").is_some()).await;
        let view = setup.observer.nested::<StageSnapshot>("frame", 1).unwrap().unwrap();
        let id = view.snapshot().description.owner_generation;
        let cursor = NestedCursor { owner_generation: id, sequence: 0 };
        assert!(matches!(
            view.changes_since(NestedCursor { owner_generation: id, sequence: 1 }),
            Err(NestedObservationError::FutureCursor { .. })
        ));
        assert!(matches!(
            view.changes_since(NestedCursor { owner_generation: uuid::Uuid::nil(), sequence: 0 }),
            Err(NestedObservationError::StaleCursor)
        ));
        for _ in 0..steps {
            step(&setup.permits).await;
        }
        let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
        assert!(report.succeeded());
        assert_eq!(report.confirmed_prefixes["frame"]["owner.stages"], steps);
        let changes = view.changes_since(cursor).unwrap();
        assert!(changes.truncated);
        assert_eq!(changes.sequences.len(), MAX_NESTED_TRANSITIONS);
        assert_eq!(changes.latest.sequence, steps);
        assert_eq!(view.snapshot().sequence, steps);
        assert_eq!(*view.snapshot().payload.unwrap(), StageSnapshot { stage: steps, position: 2 * steps as i32 });
        assert_eq!(setup.domain.stop(), StageSnapshot { stage: steps, position: 2 * steps as i32 });
        assert_eq!(view.snapshot().status, NestedStatus::OwnerClosed);
    });
}
