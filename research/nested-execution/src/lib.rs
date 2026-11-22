//! Research-only nested owner projection and control comparison for #146.
//!
//! Real Riesel graph runner and real thread-affine !Send domain actor are
//! exercised. These types are NOT published contracts or implementation code.
//! Actual RunenECS differential evidence is retained separately in PR #84.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use riesel::{
        App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionGraphRunner,
        ExecutionPlanDraft, GraphControlError, GraphControlTarget, GraphRunIdentity,
        GraphRunObserver, OperationKey, OperationNode, OperationOutcome, OperationRegistry,
        OperationState, RunningApp, async_operation, async_operation_with_effects,
    };
    use std::cell::Cell;
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    use std::num::NonZeroUsize;
    use std::rc::Rc;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    };
    use std::thread::{self, JoinHandle};
    use std::time::Duration;
    use tokio::sync::oneshot;

    const OWNER_EPOCH: u64 = 71;
    const RETAINED: usize = 4;
    const WAIT: Duration = Duration::from_secs(3);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        SimulationTick,
        PipelineStage,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Parent {
        run: GraphRunIdentity,
        operation: String,
        attempt: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Error {
        StaleParent,
        DuplicateAttempt,
        StaleOwnerEpoch,
        UnsupportedSafePoint,
        NoPermit,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct InnerEvent {
        sequence: u64,
        parent: Parent,
        owner_epoch: u64,
        kind: Kind,
        completed: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct DomainSnapshot {
        parent_run: GraphRunIdentity,
        owner_epoch: u64,
        kind: Kind,
        cumulative_world: i32,
        // The authoritative owner accounts *per outer operation attempt*.
        committed: BTreeMap<(String, u64), u64>,
        last_parent: Option<Parent>,
        latest_sequence: u64,
    }

    struct Store {
        snapshot: DomainSnapshot,
        events: VecDeque<InnerEvent>,
    }

    impl Store {
        fn record(&mut self, parent: &Parent, value: i32, prefix: u64) {
            self.snapshot.cumulative_world = value;
            self.snapshot.last_parent = Some(parent.clone());
            self.snapshot
                .committed
                .insert((parent.operation.clone(), parent.attempt), prefix);
            self.snapshot.latest_sequence += 1;
            self.events.push_back(InnerEvent {
                sequence: self.snapshot.latest_sequence,
                parent: parent.clone(),
                owner_epoch: self.snapshot.owner_epoch,
                kind: self.snapshot.kind,
                completed: prefix,
            });
            if self.events.len() > RETAINED {
                self.events.pop_front();
            }
        }
    }

    #[derive(Clone)]
    struct DomainObserver(Arc<Mutex<Store>>);

    #[derive(Debug)]
    struct DomainChanges {
        truncated: bool,
        latest: u64,
        events: Vec<InnerEvent>,
    }

    impl DomainObserver {
        fn snapshot(&self) -> DomainSnapshot {
            self.0.lock().unwrap().snapshot.clone()
        }

        fn changes_since(&self, cursor: u64) -> DomainChanges {
            let store = self.0.lock().unwrap();
            let latest = store.snapshot.latest_sequence;
            let first = store.events.front().map_or(latest + 1, |e| e.sequence);
            DomainChanges {
                truncated: cursor < first.saturating_sub(1),
                latest,
                events: store
                    .events
                    .iter()
                    .filter(|e| e.sequence > cursor)
                    .cloned()
                    .collect(),
            }
        }
    }

    enum Permit {
        Advance {
            epoch: u64,
            kind: Kind,
            accepted: oneshot::Sender<Result<(), Error>>,
        },
        Cancel {
            epoch: u64,
            accepted: oneshot::Sender<Result<(), Error>>,
        },
    }

    #[derive(Debug, PartialEq, Eq)]
    struct Reply {
        confirmed: u64,
        cancelled: bool,
    }

    enum DomainCommand {
        Frame {
            parent: Parent,
            proposed: u64,
            permits: Receiver<Permit>,
            lost_ack: bool,
            reply: oneshot::Sender<Result<Reply, Error>>,
        },
        Stop,
    }

    struct DomainActor {
        commands: SyncSender<DomainCommand>,
        observer: DomainObserver,
        worker: JoinHandle<i32>,
    }

    impl DomainActor {
        fn start(expected: GraphRunIdentity, kind: Kind, allowed_operations: &[&str]) -> Self {
            let allowed = allowed_operations
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<BTreeSet<String>>();
            let observer = DomainObserver(Arc::new(Mutex::new(Store {
                snapshot: DomainSnapshot {
                    parent_run: expected.clone(),
                    owner_epoch: OWNER_EPOCH,
                    kind,
                    cumulative_world: 0,
                    committed: BTreeMap::new(),
                    last_parent: None,
                    latest_sequence: 0,
                },
                events: VecDeque::new(),
            })));
            let (commands, incoming) = mpsc::sync_channel::<DomainCommand>(4);
            let published = observer.clone();
            let worker = thread::spawn(move || {
                // Only this actor thread ever holds this !Send world. The
                // projection contains copied values, not Rc or &mut World.
                let world = Rc::new(Cell::new(0_i32));
                // Only a local, single-owner in-memory duplicate fence for
                // this experiment. NOT durable idempotency or remote trust.
                let mut seen_attempts = BTreeSet::<(String, u64)>::new();
                while let Ok(command) = incoming.recv() {
                    match command {
                        DomainCommand::Stop => break,
                        DomainCommand::Frame {
                            parent,
                            proposed,
                            permits,
                            lost_ack,
                            reply,
                        } => {
                            if parent.run != expected
                                || !allowed.contains(&parent.operation)
                                || parent.attempt != 1
                            {
                                let _ = reply.send(Err(Error::StaleParent));
                                continue;
                            }
                            if !seen_attempts.insert((parent.operation.clone(), parent.attempt)) {
                                let _ = reply.send(Err(Error::DuplicateAttempt));
                                continue;
                            }
                            let mut confirmed = 0_u64;
                            let mut cancelled = false;
                            let mut error = None;
                            while confirmed < proposed {
                                match permits.recv_timeout(WAIT) {
                                    Ok(Permit::Advance {
                                        epoch,
                                        kind: requested,
                                        accepted,
                                    }) => {
                                        if epoch != OWNER_EPOCH {
                                            let _ = accepted.send(Err(Error::StaleOwnerEpoch));
                                            continue;
                                        }
                                        if requested != kind {
                                            let _ = accepted.send(Err(Error::UnsupportedSafePoint));
                                            continue;
                                        }
                                        let new_value = world.get()
                                            + match kind {
                                                Kind::SimulationTick => 2,
                                                Kind::PipelineStage => 5,
                                            };
                                        world.set(new_value);
                                        confirmed += 1;
                                        published
                                            .0
                                            .lock()
                                            .unwrap()
                                            .record(&parent, new_value, confirmed);
                                        let _ = accepted.send(Ok(()));
                                    }
                                    Ok(Permit::Cancel { epoch, accepted }) => {
                                        if epoch != OWNER_EPOCH {
                                            let _ = accepted.send(Err(Error::StaleOwnerEpoch));
                                            continue;
                                        }
                                        cancelled = true;
                                        let _ = accepted.send(Ok(()));
                                        break;
                                    }
                                    Err(_) => {
                                        error = Some(Error::NoPermit);
                                        break;
                                    }
                                }
                            }
                            if lost_ack {
                                // A real mutation can commit even when the
                                // async graph callback receives no reply.
                                drop(reply);
                            } else if let Some(error) = error {
                                let _ = reply.send(Err(error));
                            } else {
                                let _ = reply.send(Ok(Reply {
                                    confirmed,
                                    cancelled,
                                }));
                            }
                        }
                    }
                }
                world.get()
            });
            Self {
                commands,
                observer,
                worker,
            }
        }

        fn stop(self) -> i32 {
            self.commands.send(DomainCommand::Stop).unwrap();
            self.worker.join().unwrap()
        }
    }

    fn key(name: &str) -> OperationKey {
        OperationKey::new(name, 1).unwrap()
    }

    fn session() -> RunningApp {
        App::from_schema(
            r#"{"version":1,"app":{"name":"NestedContractResearch"},"models":{},"requires":[]}"#,
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

    struct Setup {
        _owner: RunningApp,
        runner: ExecutionGraphRunner,
        observer: GraphRunObserver,
        actor: DomainActor,
        permits: SyncSender<Permit>,
        independent: Arc<AtomicUsize>,
        downstream: Arc<AtomicUsize>,
    }

    /// Fixture-only adapter that binds one real graph operation and one
    /// domain-owned frame attempt; it is not the proposed public API.
    fn frame_handler(
        operation: &'static str,
        commands: Arc<Mutex<Option<SyncSender<DomainCommand>>>>,
        parent_run: Arc<Mutex<Option<GraphRunIdentity>>>,
        permit_rx: Receiver<Permit>,
        proposed: u64,
        lost_ack: bool,
    ) -> Arc<AsyncOperation> {
        let incoming_permits = Arc::new(Mutex::new(Some(permit_rx)));
        async_operation_with_effects(move |effects| {
            let commands = commands.lock().unwrap().clone().unwrap();
            let parent = Parent {
                run: parent_run.lock().unwrap().clone().unwrap(),
                operation: operation.to_owned(),
                attempt: 1,
            };
            let permits = incoming_permits.lock().unwrap().take().unwrap();
            async move {
                let (reply, answer) = oneshot::channel();
                if commands
                    .try_send(DomainCommand::Frame {
                        parent,
                        proposed,
                        permits,
                        lost_ack,
                        reply,
                    })
                    .is_err()
                {
                    return OperationOutcome::Failed("domain closed".into());
                }
                match answer.await {
                    Ok(Ok(result)) => {
                        if result.confirmed > 0 {
                            effects
                                .confirm_prefix("domain.committed", result.confirmed)
                                .unwrap();
                        }
                        if result.cancelled {
                            OperationOutcome::Cancelled
                        } else if result.confirmed == proposed {
                            OperationOutcome::Completed
                        } else {
                            OperationOutcome::Failed("incomplete frame".into())
                        }
                    }
                    Ok(Err(error)) => OperationOutcome::Failed(format!("{error:?}")),
                    Err(_) => OperationOutcome::OutcomeUnknown,
                }
            }
        })
    }

    fn setup(kind: Kind, proposed: u64, lost_ack: bool) -> Setup {
        let owner = session();
        let commands = Arc::new(Mutex::new(None::<SyncSender<DomainCommand>>));
        let parent_run = Arc::new(Mutex::new(None::<GraphRunIdentity>));
        let (permits, permit_rx) = mpsc::sync_channel::<Permit>(4);
        let independent = Arc::new(AtomicUsize::new(0));
        let downstream = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("frame"),
                frame_handler(
                    "frame",
                    Arc::clone(&commands),
                    Arc::clone(&parent_run),
                    permit_rx,
                    proposed,
                    lost_ack,
                ),
            )
            .unwrap();
        registry
            .register(
                key("independent"),
                async_operation({
                    let independent = Arc::clone(&independent);
                    move || {
                        let independent = Arc::clone(&independent);
                        async move {
                            independent.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        registry
            .register(
                key("after"),
                async_operation({
                    let downstream = Arc::clone(&downstream);
                    move || {
                        let downstream = Arc::clone(&downstream);
                        async move {
                            downstream.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("research.nested", 7)
            .operation(OperationNode::new("frame", key("frame")).delegated_frame("domain"))
            .operation(OperationNode::new("independent", key("independent")))
            .operation(OperationNode::new("after", key("after")).after("frame"))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(3).unwrap())
            .unwrap();
        let observer = runner.observer();
        let identity = observer.snapshot().identity;
        let actor = DomainActor::start(identity.clone(), kind, &["frame"]);
        *commands.lock().unwrap() = Some(actor.commands.clone());
        *parent_run.lock().unwrap() = Some(identity);
        Setup {
            _owner: owner,
            runner,
            observer,
            actor,
            permits,
            independent,
            downstream,
        }
    }

    async fn until(mut condition: impl FnMut() -> bool) {
        tokio::time::timeout(WAIT, async {
            while !condition() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn grant(tx: &SyncSender<Permit>, epoch: u64, kind: Kind) -> Result<(), Error> {
        let (accepted, reply) = oneshot::channel();
        tx.send(Permit::Advance {
            epoch,
            kind,
            accepted,
        })
        .unwrap();
        tokio::time::timeout(WAIT, reply).await.unwrap().unwrap()
    }

    async fn cancel(tx: &SyncSender<Permit>, epoch: u64) -> Result<(), Error> {
        let (accepted, reply) = oneshot::channel();
        tx.send(Permit::Cancel { epoch, accepted }).unwrap();
        tokio::time::timeout(WAIT, reply).await.unwrap().unwrap()
    }

    #[test]
    fn domain_owned_partial_frame_preserves_confirmed_tick_without_fake_graph_nodes() {
        executor().block_on(async {
            let setup = setup(Kind::SimulationTick, 2, false);
            let controller = setup.runner.controller();
            assert!(matches!(
                controller.pause(GraphControlTarget::RieselDomainTicks("domain".into())),
                Err(GraphControlError::UnsupportedDomainAuthority { .. })
            ));
            let task = tokio::spawn(setup.runner.execute());
            until(|| {
                setup.independent.load(Ordering::SeqCst) == 1
                    && setup.observer.snapshot().operations["frame"].state
                        == OperationState::Running
            })
            .await;
            assert_eq!(setup.observer.snapshot().operations["frame"].attempt, 1);
            assert_eq!(setup.actor.observer.snapshot().latest_sequence, 0);
            assert_eq!(
                grant(&setup.permits, OWNER_EPOCH - 1, Kind::SimulationTick).await,
                Err(Error::StaleOwnerEpoch)
            );
            assert_eq!(
                grant(&setup.permits, OWNER_EPOCH, Kind::PipelineStage).await,
                Err(Error::UnsupportedSafePoint)
            );
            assert_eq!(setup.actor.observer.snapshot().cumulative_world, 0);
            grant(&setup.permits, OWNER_EPOCH, Kind::SimulationTick)
                .await
                .unwrap();
            until(|| setup.actor.observer.snapshot().latest_sequence == 1).await;
            let snapshot = setup.actor.observer.snapshot();
            assert_eq!(snapshot.committed[&("frame".into(), 1)], 1);
            assert_eq!(snapshot.cumulative_world, 2);
            assert_eq!(
                snapshot.last_parent.as_ref().unwrap().run,
                setup.observer.snapshot().identity
            );
            assert_eq!(
                setup.observer.snapshot().operations["frame"].state,
                OperationState::Running
            );
            cancel(&setup.permits, OWNER_EPOCH).await.unwrap();
            let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
            assert_eq!(report.operations["frame"], OperationState::Cancelled);
            assert_eq!(report.operations["after"], OperationState::Blocked);
            assert_eq!(report.confirmed_prefixes["frame"]["domain.committed"], 1);
            assert_eq!(setup.downstream.load(Ordering::SeqCst), 0);
            assert!(!report.operations.contains_key("tick-1"));
            assert_eq!(setup.actor.stop(), 2);
        });
    }

    #[test]
    fn lost_reply_is_unknown_even_if_domain_owner_resnapshot_proves_commits() {
        executor().block_on(async {
            let setup = setup(Kind::SimulationTick, 2, true);
            let task = tokio::spawn(setup.runner.execute());
            until(|| {
                setup.observer.snapshot().operations["frame"].state == OperationState::Running
            })
            .await;
            grant(&setup.permits, OWNER_EPOCH, Kind::SimulationTick)
                .await
                .unwrap();
            grant(&setup.permits, OWNER_EPOCH, Kind::SimulationTick)
                .await
                .unwrap();
            let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
            assert_eq!(report.operations["frame"], OperationState::OutcomeUnknown);
            assert_eq!(report.operations["after"], OperationState::Blocked);
            assert!(
                report.confirmed_prefixes.get("frame").is_none(),
                "a lost acknowledgement must not create a graph effect confirmation"
            );
            let owner_view = setup.actor.observer.snapshot();
            assert_eq!(owner_view.committed[&("frame".into(), 1)], 2);
            assert_eq!(owner_view.cumulative_world, 4);
            assert_eq!(owner_view.parent_run, report.identity);
            assert_eq!(setup.downstream.load(Ordering::SeqCst), 0);
            assert_eq!(setup.actor.stop(), 4);
            // A future reconciliation contract could use this independent
            // authoritative owner snapshot, never the missing graph trace.
        });
    }

    #[test]
    fn non_ecs_stages_share_parent_correlation_and_bounded_observer_resync() {
        executor().block_on(async {
            let setup = setup(Kind::PipelineStage, 12, false);
            let task = tokio::spawn(setup.runner.execute());
            until(|| setup.independent.load(Ordering::SeqCst) == 1).await;
            for _ in 0..12 {
                grant(&setup.permits, OWNER_EPOCH, Kind::PipelineStage)
                    .await
                    .unwrap();
            }
            let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
            assert!(report.succeeded());
            assert_eq!(report.confirmed_prefixes["frame"]["domain.committed"], 12);
            assert_eq!(setup.downstream.load(Ordering::SeqCst), 1);
            let batches = setup.actor.observer.changes_since(0);
            assert!(batches.truncated);
            assert_eq!(batches.events.len(), RETAINED);
            assert_eq!(batches.latest, 12);
            assert_eq!(batches.events[0].sequence, 9);
            let snapshot = setup.actor.observer.snapshot();
            assert_eq!(snapshot.cumulative_world, 60);
            assert_eq!(snapshot.committed[&("frame".into(), 1)], 12);
            assert_eq!(snapshot.last_parent.unwrap().run, report.identity);
            assert!(
                setup
                    .actor
                    .observer
                    .changes_since(snapshot.latest_sequence)
                    .events
                    .is_empty()
            );
            assert_eq!(setup.actor.stop(), 60);
        });
    }

    #[test]
    fn new_session_and_wrong_parent_reject_before_nested_world_mutation() {
        let first = setup(Kind::SimulationTick, 1, false);
        let second = setup(Kind::SimulationTick, 1, false);
        assert_ne!(
            first.observer.snapshot().identity.session_generation(),
            second.observer.snapshot().identity.session_generation()
        );
        let (tx, rx) = mpsc::sync_channel(1);
        let (reply, response) = oneshot::channel();
        first
            .actor
            .commands
            .send(DomainCommand::Frame {
                parent: Parent {
                    run: second.observer.snapshot().identity,
                    operation: "frame".into(),
                    attempt: 1,
                },
                proposed: 1,
                permits: rx,
                lost_ack: false,
                reply,
            })
            .unwrap();
        assert_eq!(
            executor().block_on(response).unwrap(),
            Err(Error::StaleParent)
        );
        drop(tx);
        assert_eq!(first.actor.observer.snapshot().cumulative_world, 0);
        assert_eq!(first.actor.stop(), 0);
        assert_eq!(second.actor.stop(), 0);
    }
    #[test]
    fn multiple_real_outer_frames_have_distinct_authoritative_nested_parents() {
        executor().block_on(async {
            let owner = session();
            let commands = Arc::new(Mutex::new(None::<SyncSender<DomainCommand>>));
            let parent_run = Arc::new(Mutex::new(None::<GraphRunIdentity>));
            let (first_tx, first_rx) = mpsc::sync_channel::<Permit>(4);
            let (second_tx, second_rx) = mpsc::sync_channel::<Permit>(4);
            let mut registry = OperationRegistry::<AsyncOperation>::new();
            registry
                .register(
                    key("frame-first"),
                    frame_handler(
                        "frame-first",
                        Arc::clone(&commands),
                        Arc::clone(&parent_run),
                        first_rx,
                        1,
                        false,
                    ),
                )
                .unwrap();
            registry
                .register(
                    key("frame-second"),
                    frame_handler(
                        "frame-second",
                        Arc::clone(&commands),
                        Arc::clone(&parent_run),
                        second_rx,
                        1,
                        false,
                    ),
                )
                .unwrap();
            let plan = ExecutionPlanDraft::new("research.nested.multiframe", 8)
                .operation(
                    OperationNode::new("frame-first", key("frame-first")).delegated_frame("domain"),
                )
                .operation(
                    OperationNode::new("frame-second", key("frame-second"))
                        .after("frame-first")
                        .delegated_frame("domain"),
                )
                .validate()
                .unwrap()
                .bind(&registry)
                .unwrap();
            let runner = owner
                .execution_graph(plan, NonZeroUsize::new(1).unwrap())
                .unwrap();
            let observer = runner.observer();
            let identity = observer.snapshot().identity;
            let actor = DomainActor::start(
                identity.clone(),
                Kind::SimulationTick,
                &["frame-first", "frame-second"],
            );
            *commands.lock().unwrap() = Some(actor.commands.clone());
            *parent_run.lock().unwrap() = Some(identity.clone());
            let task = tokio::spawn(runner.execute());
            until(|| {
                observer.snapshot().operations["frame-first"].state == OperationState::Running
            })
            .await;
            grant(&first_tx, OWNER_EPOCH, Kind::SimulationTick)
                .await
                .unwrap();
            until(|| {
                observer.snapshot().operations["frame-first"].state == OperationState::Completed
                    && observer.snapshot().operations["frame-second"].state
                        == OperationState::Running
            })
            .await;
            grant(&second_tx, OWNER_EPOCH, Kind::SimulationTick)
                .await
                .unwrap();
            let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
            assert!(report.succeeded());
            assert_eq!(
                report.confirmed_prefixes["frame-first"]["domain.committed"],
                1
            );
            assert_eq!(
                report.confirmed_prefixes["frame-second"]["domain.committed"],
                1
            );
            let domain_view = actor.observer.snapshot();
            assert_eq!(domain_view.parent_run, report.identity);
            assert_eq!(domain_view.cumulative_world, 4);
            assert_eq!(domain_view.latest_sequence, 2);
            assert_eq!(domain_view.committed[&("frame-first".into(), 1)], 1);
            assert_eq!(domain_view.committed[&("frame-second".into(), 1)], 1);
            let events = actor.observer.changes_since(0).events;
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].parent.operation, "frame-first");
            assert_eq!(events[1].parent.operation, "frame-second");
            assert_eq!(events[0].parent.run, events[1].parent.run);

            // Replaying a command with the identical parent/attempt in the
            // same domain-owner lifetime is rejected before any new mutation.
            // This is NOT a durable exactly-once guarantee across restart.
            let (unused_tx, unused_rx) = mpsc::sync_channel::<Permit>(1);
            let (reply, answer) = oneshot::channel();
            actor
                .commands
                .send(DomainCommand::Frame {
                    parent: Parent {
                        run: identity,
                        operation: "frame-first".into(),
                        attempt: 1,
                    },
                    proposed: 1,
                    permits: unused_rx,
                    lost_ack: false,
                    reply,
                })
                .unwrap();
            assert_eq!(answer.await.unwrap(), Err(Error::DuplicateAttempt));
            drop(unused_tx);
            assert_eq!(actor.observer.snapshot().latest_sequence, 2);
            assert_eq!(actor.stop(), 4);
        });
    }
}
