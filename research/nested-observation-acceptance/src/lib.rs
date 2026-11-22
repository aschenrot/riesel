//! Issue #153 — real Bevy ECS and pinned RunenECS using the SAME
//! production Riesel nested observation facility (no research-only observer).
//! This independent workspace is test-only; no engine enters Riesel's SDK.
#![forbid(unsafe_code)]

#[cfg(test)]
mod tests {
    use riesel::{
        App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft,
        GraphControlError, GraphControlTarget, NestedDomainOwner, NestedDomainRegistration,
        NestedObservationError, NestedPublisher, NestedStatus, OperationKey, OperationNode,
        OperationOutcome, OperationRegistry, OperationState, RunningApp, async_operation,
        async_operation_with_effects,
    };
    use std::{
        cell::Cell,
        fmt::Debug,
        num::NonZeroUsize,
        rc::Rc,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc::{self, Receiver, SyncSender},
        },
        thread::{self, JoinHandle},
        time::Duration,
    };
    use tokio::sync::oneshot;

    const WAIT: Duration = Duration::from_secs(4);

    trait DomainView: Clone + Debug + PartialEq + Send + Sync + 'static {
        fn ticks(&self) -> u32;
        fn position(&self) -> i32;
        fn owner_thread_calls(&self) -> u32;
    }

    trait DomainImpl: 'static {
        type View: DomainView;
        fn new() -> Self;
        fn tick(&mut self) -> Self::View;
        fn last(&mut self) -> Self::View;
    }

    mod bevy_domain {
        use super::{Cell, DomainImpl, DomainView, Rc};
        use bevy_ecs::prelude::*;
        use bevy_ecs::schedule::{ScheduleLabel, SingleThreadedExecutor};

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ScheduleLabel)]
        struct Simulation;
        #[derive(Component)]
        struct Position(i32);
        #[derive(Resource)]
        struct Input(i32);
        #[derive(Resource)]
        struct Ticks(u32);
        struct LocalCalls(Rc<Cell<u32>>);

        fn integrate(mut positions: Query<&mut Position>, input: Res<Input>) {
            for mut position in &mut positions {
                position.0 += input.0;
            }
        }
        fn finish(mut ticks: ResMut<Ticks>, local: NonSend<LocalCalls>) {
            ticks.0 += 1;
            local.0.set(local.0.get() + 1);
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(super) struct View {
            pub position: i32,
            pub tick: u32,
            pub non_send_calls: u32,
        }
        impl DomainView for View {
            fn ticks(&self) -> u32 {
                self.tick
            }
            fn position(&self) -> i32 {
                self.position
            }
            fn owner_thread_calls(&self) -> u32 {
                self.non_send_calls
            }
        }

        pub(super) struct Bevy {
            world: World,
            schedule: Schedule,
        }
        impl DomainImpl for Bevy {
            type View = View;

            fn new() -> Self {
                let mut world = World::new();
                world.spawn(Position(0));
                world.insert_resource(Input(2));
                world.insert_resource(Ticks(0));
                world.insert_non_send(LocalCalls(Rc::new(Cell::new(0))));
                let mut schedule = Schedule::new(Simulation);
                schedule.set_executor(SingleThreadedExecutor::new());
                schedule.add_systems((integrate, finish).chain());
                Self { world, schedule }
            }

            fn tick(&mut self) -> View {
                self.schedule.run(&mut self.world);
                self.last()
            }

            fn last(&mut self) -> View {
                let ticks = self.world.resource::<Ticks>().0;
                let mut positions = self.world.query::<&Position>();
                let position = positions.iter(&self.world).next().unwrap().0;
                let calls = self.world.get_non_send::<LocalCalls>().unwrap().0.get();
                assert_eq!(calls, ticks);
                assert_eq!(position, ticks as i32 * 2);
                View {
                    position,
                    tick: ticks,
                    non_send_calls: calls,
                }
            }
        }
    }

    mod runen_domain {
        use super::{Cell, DomainImpl, DomainView, Rc};
        use runen_ecs::prelude::*;

        #[derive(ScheduleLabel)]
        struct Simulation;
        #[derive(SystemSet)]
        struct Movement;
        #[derive(Component)]
        struct Position(i32);
        #[derive(Resource)]
        struct Input(i32);
        #[derive(Resource)]
        struct Ticks(u32);
        #[derive(Resource)]
        struct LocalCalls(Rc<Cell<u32>>);

        fn integrate(mut positions: Query<&mut Position>, input: Res<Input>) {
            for position in positions.iter() {
                position.0 += input.0;
            }
        }
        fn finish(mut ticks: ResMut<Ticks>, local: Res<LocalCalls>) {
            ticks.0 += 1;
            local.0.set(local.0.get() + 1);
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub(super) struct View {
            pub x: i32,
            pub simulation_ticks: u32,
            pub invoker_calls: u32,
        }
        impl DomainView for View {
            fn ticks(&self) -> u32 {
                self.simulation_ticks
            }
            fn position(&self) -> i32 {
                self.x
            }
            fn owner_thread_calls(&self) -> u32 {
                self.invoker_calls
            }
        }

        pub(super) struct Runen {
            world: World,
            runtime: Runtime,
        }
        impl DomainImpl for Runen {
            type View = View;

            fn new() -> Self {
                let mut world = World::new();
                world.spawn(Position(0)).unwrap();
                world.insert_resource(Input(2));
                world.insert_resource(Ticks(0));
                world.insert_resource(LocalCalls(Rc::new(Cell::new(0))));
                let mut runtime = Runtime::new();
                runtime
                    .add_systems(
                        Simulation,
                        (
                            integrate.in_set(Movement),
                            finish.on_invoker_thread().after(Movement),
                        ),
                    )
                    .unwrap();
                Self { world, runtime }
            }

            fn tick(&mut self) -> View {
                self.runtime
                    .run_schedule::<Simulation>(&mut self.world)
                    .unwrap();
                self.last()
            }

            fn last(&mut self) -> View {
                let ticks = self.world.resource::<Ticks>().unwrap().0;
                let position = self
                    .world
                    .query::<&Position>()
                    .iter(&self.world)
                    .next()
                    .unwrap()
                    .0;
                let calls = self.world.resource::<LocalCalls>().unwrap().0.get();
                assert_eq!(calls, ticks);
                assert_eq!(position, ticks as i32 * 2);
                View {
                    x: position,
                    simulation_ticks: ticks,
                    invoker_calls: calls,
                }
            }
        }
    }

    enum Permit {
        Advance(oneshot::Sender<()>),
        Cancel(oneshot::Sender<()>),
    }

    enum Command<T> {
        Frame {
            source: NestedPublisher<T>,
            permits: Receiver<Permit>,
            lost_ack: bool,
            reply: oneshot::Sender<(u64, bool)>,
        },
        Stop,
    }

    struct Actor<T> {
        commands: SyncSender<Command<T>>,
        registration: NestedDomainRegistration,
        worker: JoinHandle<T>,
    }

    fn actor<D: DomainImpl>() -> Actor<D::View> {
        let (commands, incoming) = mpsc::sync_channel::<Command<D::View>>(4);
        let (register_tx, register_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let owner = NestedDomainOwner::new("simulation.domain");
            register_tx.send(owner.registration()).unwrap();
            // A native Bevy/Runen World with Rc<Cell> is CONSTRUCTED INSIDE
            // this OS thread, never sent through a Tokio operation future.
            let mut ecs = D::new();
            while let Ok(command) = incoming.recv() {
                match command {
                    Command::Stop => break,
                    Command::Frame {
                        source,
                        permits,
                        lost_ack,
                        reply,
                    } => {
                        let mut confirmed = 0;
                        let mut cancelled = false;
                        while confirmed < 2 {
                            match permits.recv_timeout(WAIT) {
                                Ok(Permit::Advance(ack)) => {
                                    let snapshot = ecs.tick();
                                    source.publish(snapshot).unwrap();
                                    confirmed += 1;
                                    let _ = ack.send(());
                                }
                                Ok(Permit::Cancel(ack)) => {
                                    cancelled = true;
                                    let _ = ack.send(());
                                    break;
                                }
                                Err(error) => panic!("missing ECS permit: {error}"),
                            }
                        }
                        if !lost_ack {
                            let _ = reply.send((confirmed, cancelled));
                        }
                    }
                }
            }
            ecs.last()
        });
        Actor {
            commands,
            registration: register_rx.recv().unwrap(),
            worker,
        }
    }

    fn key(value: &str) -> OperationKey {
        OperationKey::new(value, 1).unwrap()
    }

    fn session() -> RunningApp {
        App::from_schema(
            r#"{"version":1,"app":{"name":"EcsNestedAcceptance"},"models":{},"requires":[]}"#,
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

    async fn until<F: Fn() -> bool>(condition: F) {
        tokio::time::timeout(WAIT, async {
            while !condition() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    async fn permit(steps: &SyncSender<Permit>, cancel: bool) {
        let (send, ack) = oneshot::channel();
        if cancel {
            steps.send(Permit::Cancel(send)).unwrap();
        } else {
            steps.send(Permit::Advance(send)).unwrap();
        }
        tokio::time::timeout(WAIT, ack).await.unwrap().unwrap();
    }

    fn run_once<D: DomainImpl>(lost_ack: bool) {
        executor().block_on(async {
            let actor = actor::<D>();
            let (steps, step_rx) = mpsc::sync_channel::<Permit>(4);
            let step_rx = Arc::new(std::sync::Mutex::new(Some(step_rx)));
            let completed = Arc::new(AtomicUsize::new(0));
            let mut registry = OperationRegistry::<AsyncOperation>::new();
            registry
                .register(
                    key("frame"),
                    async_operation_with_effects({
                        let commands = actor.commands.clone();
                        let registration = actor.registration.clone();
                        let step_rx = Arc::clone(&step_rx);
                        move |effects| {
                            let commands = commands.clone();
                            let registration = registration.clone();
                            let permits = step_rx.lock().unwrap().take().unwrap();
                            async move {
                                let source =
                                    effects.attach_nested::<D::View>(&registration, 1).unwrap();
                                let (reply, ack) = oneshot::channel();
                                commands
                                    .send(Command::Frame {
                                        source,
                                        permits,
                                        lost_ack,
                                        reply,
                                    })
                                    .unwrap();
                                match ack.await {
                                    Ok((count, cancelled)) => {
                                        if count != 0 {
                                            effects.confirm_prefix("ecs.committed", count).unwrap();
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
                    key("async"),
                    async_operation({
                        let completed = Arc::clone(&completed);
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
            let plan = ExecutionPlanDraft::new("ecs.nested.real", 2)
                .operation(
                    OperationNode::new("frame", key("frame")).delegated_frame("simulation.domain"),
                )
                .operation(OperationNode::new("async", key("async")))
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
            assert!(observer.nested_description("frame").is_none());
            let controls = graph.controller();
            assert!(matches!(
                controls.pause(GraphControlTarget::RieselDomainTicks(
                    "simulation.domain".into()
                )),
                Err(GraphControlError::UnsupportedDomainAuthority { .. })
            ));
            let run = tokio::spawn(graph.execute());
            until(|| observer.nested_description("frame").is_some()).await;
            until(|| completed.load(Ordering::SeqCst) == 1).await;
            let view = observer.nested::<D::View>("frame", 1).unwrap().unwrap();
            let before = view.snapshot();
            assert_eq!(before.status, NestedStatus::Unpublished);
            assert_eq!(before.description.parent.run, observer.snapshot().identity);
            assert_eq!(
                before.description.parent.attempt,
                observer.snapshot().operations["frame"].attempt
            );
            assert_eq!(before.description.domain, "simulation.domain");
            assert!(matches!(
                observer.nested::<String>("frame", 1),
                Err(NestedObservationError::WrongPayloadType)
            ));
            permit(&steps, false).await;
            let tick1 = view.snapshot();
            assert_eq!(tick1.status, NestedStatus::Current);
            let payload = tick1.payload.unwrap();
            assert_eq!(payload.ticks(), 1);
            assert_eq!(payload.position(), 2);
            assert_eq!(payload.owner_thread_calls(), 1);
            assert_eq!(
                observer.snapshot().operations["frame"].state,
                OperationState::Running
            );

            if !lost_ack {
                permit(&steps, true).await;
            } else {
                permit(&steps, false).await;
            }
            let report = tokio::time::timeout(WAIT, run).await.unwrap().unwrap();
            assert_eq!(report.operations["dependent"], OperationState::Blocked);
            assert_eq!(report.operations["async"], OperationState::Completed);
            assert_eq!(
                report.operations["frame"],
                if lost_ack {
                    OperationState::OutcomeUnknown
                } else {
                    OperationState::Cancelled
                }
            );
            if lost_ack {
                assert!(!report.confirmed_prefixes.contains_key("frame"));
                assert_eq!(view.snapshot().sequence, 2);
                assert_eq!(view.snapshot().payload.as_ref().unwrap().ticks(), 2);
            } else {
                assert_eq!(report.confirmed_prefixes["frame"]["ecs.committed"], 1);
                assert_eq!(view.snapshot().sequence, 1);
            }
            assert_eq!(view.snapshot().description.parent.run, report.identity);
            actor.commands.send(Command::Stop).unwrap();
            let native = actor.worker.join().unwrap();
            assert_eq!(native.ticks(), if lost_ack { 2 } else { 1 });
            assert_eq!(view.snapshot().status, NestedStatus::OwnerClosed);
            assert_eq!(
                observer.snapshot().operations["frame"].state,
                report.operations["frame"]
            );
            drop(app);
            assert_eq!(view.snapshot().status, NestedStatus::SessionExpired);
        });
    }

    #[test]
    fn real_bevy_ecs_domain_partial_cancellation() {
        run_once::<bevy_domain::Bevy>(false);
    }
    #[test]
    fn real_bevy_ecs_domain_unknown_after_two_commits() {
        run_once::<bevy_domain::Bevy>(true);
    }
    #[test]
    fn real_runen_ecs_domain_partial_cancellation() {
        run_once::<runen_domain::Runen>(false);
    }
    #[test]
    fn real_runen_ecs_domain_unknown_after_two_commits() {
        run_once::<runen_domain::Runen>(true);
    }
}
