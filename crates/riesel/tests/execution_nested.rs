//! Issue #153: a real RunningApp graph with a thread-confined non-ECS owner.
//! This exercises the *production* read-only nested API, not the older
//! research-only actor's independent observation implementation.
#![cfg(feature = "async-tcp")]

use riesel::{
    App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionGraphRunner,
    ExecutionPlanDraft, GraphControlError, GraphControlTarget, GraphEffectRecorder, GraphRunObserver,
    GraphRunPhase,
    MAX_NESTED_TRANSITIONS, NestedCursor, NestedDomainOwner, NestedDomainRegistration,
    NestedObservationError, NestedPublisher, NestedStatus, OperationKey, OperationNode,
    OperationOutcome, OperationRegistry, OperationState, RunningApp, async_operation,
    async_operation_with_effects,
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
    .start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
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
                        let publisher =
                            match effects.attach_nested::<StageSnapshot>(&registration, 1) {
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
                                if cancelled {
                                    OperationOutcome::Cancelled
                                } else {
                                    OperationOutcome::Completed
                                }
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
    let graph = app
        .execution_graph(plan, NonZeroUsize::new(3).unwrap())
        .unwrap();
    let observer = graph.observer();
    Fixture {
        app,
        domain,
        graph,
        observer,
        permits,
        independent,
    }
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
        assert!(
            setup
                .observer
                .nested::<StageSnapshot>("frame", 1)
                .unwrap()
                .is_none()
        );
        let controller = setup.graph.controller();
        assert!(matches!(
            controller.pause(GraphControlTarget::RieselDomainTicks("world.owner".into())),
            Err(GraphControlError::UnsupportedDomainAuthority { .. })
        ));
        let task = tokio::spawn(setup.graph.execute());
        until(|| setup.observer.nested_description("frame").is_some()).await;
        until(|| setup.independent.load(Ordering::SeqCst) == 1).await;
        let view = setup
            .observer
            .nested::<StageSnapshot>("frame", 1)
            .unwrap()
            .unwrap();
        let initial = view.snapshot();
        assert_eq!(initial.status, NestedStatus::Unpublished);
        assert!(initial.payload.is_none(), "no invented initial owner state");
        assert_eq!(
            initial.description.parent.run,
            setup.observer.snapshot().identity
        );
        assert_eq!(initial.description.parent.operation, "frame");
        assert_eq!(
            initial.description.parent.attempt,
            setup.observer.snapshot().operations["frame"].attempt
        );
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
        assert_eq!(
            *first.payload.unwrap(),
            StageSnapshot {
                stage: 1,
                position: 2
            }
        );
        assert_eq!(first.sequence, 1);
        assert_eq!(
            setup.observer.snapshot().operations["frame"].state,
            OperationState::Running
        );
        cancel(&setup.permits).await;
        let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
        assert_eq!(report.operations["frame"], OperationState::Cancelled);
        assert_eq!(report.operations["dependent"], OperationState::Blocked);
        assert_eq!(report.operations["independent"], OperationState::Completed);
        assert_eq!(report.confirmed_prefixes["frame"]["owner.stages"], 1);
        assert!(!report.operations.contains_key("domain.system"));
        assert_eq!(view.snapshot().description.parent.run, report.identity);
        assert_eq!(
            setup.domain.stop(),
            StageSnapshot {
                stage: 1,
                position: 2
            }
        );
        assert_eq!(view.snapshot().status, NestedStatus::OwnerClosed);
        assert_eq!(
            *view.snapshot().payload.unwrap(),
            StageSnapshot {
                stage: 1,
                position: 2
            }
        );
        assert_eq!(
            setup.observer.snapshot().operations["frame"].state,
            OperationState::Cancelled
        );
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
        let view = setup
            .observer
            .nested::<StageSnapshot>("frame", 1)
            .unwrap()
            .unwrap();
        step(&setup.permits).await;
        step(&setup.permits).await;
        let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
        assert_eq!(report.operations["frame"], OperationState::OutcomeUnknown);
        assert_eq!(report.operations["dependent"], OperationState::Blocked);
        assert!(!report.confirmed_prefixes.contains_key("frame"));
        assert_eq!(view.snapshot().sequence, 2);
        assert_eq!(
            *view.snapshot().payload.unwrap(),
            StageSnapshot {
                stage: 2,
                position: 4
            }
        );
        assert_eq!(
            setup.domain.stop(),
            StageSnapshot {
                stage: 2,
                position: 4
            }
        );
        assert_eq!(
            setup.observer.snapshot().operations["frame"].state,
            OperationState::OutcomeUnknown
        );
    });
}

#[test]
fn bounded_snapshot_cursor_overflow_and_owner_generation_fencing() {
    executor().block_on(async {
        let steps = MAX_NESTED_TRANSITIONS as u64 + 9;
        let setup = fixture(steps, false);
        let task = tokio::spawn(setup.graph.execute());
        until(|| setup.observer.nested_description("frame").is_some()).await;
        let view = setup
            .observer
            .nested::<StageSnapshot>("frame", 1)
            .unwrap()
            .unwrap();
        let id = view.snapshot().description.owner_generation;
        let cursor = NestedCursor {
            owner_generation: id,
            sequence: 0,
        };
        assert!(matches!(
            view.changes_since(NestedCursor {
                owner_generation: id,
                sequence: 1
            }),
            Err(NestedObservationError::FutureCursor { .. })
        ));
        assert!(matches!(
            view.changes_since(NestedCursor {
                owner_generation: uuid::Uuid::nil(),
                sequence: 0
            }),
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
        assert_eq!(
            *view.snapshot().payload.unwrap(),
            StageSnapshot {
                stage: steps,
                position: 2 * steps as i32
            }
        );
        assert_eq!(
            setup.domain.stop(),
            StageSnapshot {
                stage: steps,
                position: 2 * steps as i32
            }
        );
        assert_eq!(view.snapshot().status, NestedStatus::OwnerClosed);
    });
}


#[test]
fn abandoned_outer_future_does_not_erase_late_domain_owned_effects() {
    executor().block_on(async {
        let setup = fixture(2, false);
        let task = tokio::spawn(setup.graph.execute());
        until(|| setup.observer.nested_description("frame").is_some()).await;
        let view = setup
            .observer
            .nested::<StageSnapshot>("frame", 1)
            .unwrap()
            .unwrap();
        // Prove the native Frame command reached its owner before abruptly
        // abandoning the Tokio graph coordinator.
        step(&setup.permits).await;
        assert_eq!(view.snapshot().sequence, 1);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        until(|| setup.observer.snapshot().phase == GraphRunPhase::Abandoned).await;
        assert_eq!(
            setup.observer.snapshot().operations["frame"].state,
            OperationState::OutcomeUnknown
        );
        // The already-dispatched native domain continues independently.
        step(&setup.permits).await;
        assert_eq!(view.snapshot().sequence, 2);
        assert_eq!(view.snapshot().payload.as_ref().unwrap().stage, 2);
        assert_eq!(
            setup.observer.snapshot().operations["frame"].state,
            OperationState::OutcomeUnknown
        );
        assert_eq!(
            setup.domain.stop(),
            StageSnapshot { stage: 2, position: 4 }
        );
        assert_eq!(view.snapshot().status, NestedStatus::OwnerClosed);
    });
}

#[test]
fn replacing_owner_generation_retires_old_readers_and_publishers() {
    executor().block_on(async {
        let first = NestedDomainOwner::new("world.owner");
        let second = NestedDomainOwner::new("world.owner");
        let wrong = NestedDomainOwner::new("other.domain");
        assert_ne!(first.generation(), second.generation());
        let (attached_tx, attached_rx) = oneshot::channel::<NestedPublisher<i32>>();
        let attached_tx = Arc::new(Mutex::new(Some(attached_tx)));
        let (continue_tx, continue_rx) = oneshot::channel::<()>();
        let continue_rx = Arc::new(Mutex::new(Some(continue_rx)));
        let reporter = Arc::new(Mutex::new(None::<GraphEffectRecorder>));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("frame"),
                async_operation_with_effects({
                    let first = first.registration();
                    let second = second.registration();
                    let wrong = wrong.registration();
                    let attached_tx = Arc::clone(&attached_tx);
                    let continue_rx = Arc::clone(&continue_rx);
                    let reporter = Arc::clone(&reporter);
                    move |effects| {
                        let first = first.clone();
                        let second = second.clone();
                        let wrong = wrong.clone();
                        let attached_tx = attached_tx.lock().unwrap().take().unwrap();
                        let continue_rx = continue_rx.lock().unwrap().take().unwrap();
                        *reporter.lock().unwrap() = Some(effects.clone());
                        async move {
                            assert!(matches!(
                                effects.attach_nested::<i32>(&wrong, 1),
                                Err(NestedObservationError::WrongDomain { .. })
                            ));
                            assert!(matches!(
                                effects.attach_nested::<i32>(&first, 0),
                                Err(NestedObservationError::InvalidVersion)
                            ));
                            let original = effects.attach_nested::<i32>(&first, 1).unwrap();
                            assert!(matches!(
                                effects.attach_nested::<i32>(&first, 1),
                                Err(NestedObservationError::AlreadyAttached)
                            ));
                            attached_tx.send(original).unwrap();
                            continue_rx.await.unwrap();
                            let replacement = effects.attach_nested::<i32>(&second, 2).unwrap();
                            replacement.publish(42).unwrap();
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("nested.replace", 4)
            .operation(OperationNode::new("frame", key("frame")).delegated_frame("world.owner"))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let app = session();
        let runner = app.execution_graph(plan, NonZeroUsize::new(1).unwrap()).unwrap();
        let observer = runner.observer();
        let task = tokio::spawn(runner.execute());
        let original = tokio::time::timeout(WAIT, attached_rx).await.unwrap().unwrap();
        original.publish(7).unwrap();
        let old = observer.nested::<i32>("frame", 1).unwrap().unwrap();
        assert_eq!(*old.snapshot().payload.unwrap(), 7);
        assert_eq!(old.snapshot().description.owner_generation, first.generation());
        let old_cursor = NestedCursor {
            owner_generation: first.generation(),
            sequence: 1,
        };
        continue_tx.send(()).unwrap();
        until(|| observer.nested_description("frame").unwrap().owner_generation == second.generation()).await;
        assert_eq!(old.snapshot().status, NestedStatus::Superseded);
        assert!(matches!(
            old.changes_since(old_cursor),
            Err(NestedObservationError::Superseded)
        ));
        assert!(matches!(
            original.publish(8),
            Err(NestedObservationError::Superseded)
        ));
        assert!(matches!(
            observer.nested::<i32>("frame", 1),
            Err(NestedObservationError::UnsupportedVersion { .. })
        ));
        let new = observer.nested::<i32>("frame", 2).unwrap().unwrap();
        assert_eq!(new.snapshot().status, NestedStatus::Current);
        assert_eq!(*new.snapshot().payload.unwrap(), 42);
        assert_ne!(new.snapshot().description.owner_generation, old_cursor.owner_generation);
        assert!(matches!(
            new.changes_since(old_cursor),
            Err(NestedObservationError::StaleCursor)
        ));
        let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
        assert_eq!(report.operations["frame"], OperationState::Completed);
        assert_eq!(new.snapshot().description.parent.run, report.identity);
        assert_eq!(new.snapshot().description.parent.attempt, 1);
        assert!(matches!(
            reporter.lock().unwrap().as_ref().unwrap().attach_nested::<i32>(&second.registration(), 2),
            Err(NestedObservationError::InactiveAttempt)
        ));
        drop(second);
        assert_eq!(new.snapshot().status, NestedStatus::OwnerClosed);
        drop(app);
        assert_eq!(new.snapshot().status, NestedStatus::SessionExpired);
    });
}

#[test]
fn ordinary_operations_cannot_create_domain_owned_nested_views() {
    executor().block_on(async {
        let domain = NestedDomainOwner::new("world.owner");
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("ordinary"),
                async_operation_with_effects({
                    let registration = domain.registration();
                    move |effects| {
                        let registration = registration.clone();
                        async move {
                            assert!(matches!(
                                effects.attach_nested::<usize>(&registration, 1),
                                Err(NestedObservationError::UnsupportedBoundary)
                            ));
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("nested.unsupported", 1)
            .operation(OperationNode::new("ordinary", key("ordinary")))
            .validate().unwrap().bind(&registry).unwrap();
        let app = session();
        let runner = app.execution_graph(plan, NonZeroUsize::new(1).unwrap()).unwrap();
        let observer = runner.observer();
        let report = runner.execute().await;
        assert!(report.succeeded());
        assert!(observer.nested_description("ordinary").is_none());
        assert!(observer.nested::<usize>("ordinary", 1).unwrap().is_none());
    });
}
