//! Decision probe for #83. Real RunenECS is used ONLY as a pinned research dependency.
//! This does not register RunenECS as a Riesel provider or accept a production API.
use super::*;
use runen_ecs::prelude::*;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::mpsc::{self, SyncSender};
use std::thread::JoinHandle;
use tokio::sync::oneshot;

fn noop() -> Operation {
    operation(|| async { Ok(()) })
}

#[derive(ScheduleLabel)]
struct Simulation;
#[derive(SystemSet)]
struct Spawn;
#[derive(SystemSet)]
struct Movement;

#[derive(Component)]
struct Position(i32);
#[derive(Resource)]
struct Input(i32);
#[derive(Resource)]
struct TickCount(u32);
#[derive(Resource)]
struct LocalOnly(Rc<Cell<u32>>);

fn queue_spawn(mut commands: Commands) {
    commands.spawn(Position(10));
}
fn move_positions(mut positions: Query<&mut Position>, input: Res<Input>) {
    for position in positions.iter() {
        position.0 += input.0;
    }
}
fn finish_tick(mut count: ResMut<TickCount>, local: Res<LocalOnly>) {
    count.0 += 1;
    local.0.set(local.0.get() + 1);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EcsSnapshot {
    tick: u32,
    positions: Vec<i32>,
    invoker_thread_calls: u32,
}

enum DomainPermit {
    Advance { generation: u64, reply: oneshot::Sender<Result<(), &'static str>> },
    Cancel { generation: u64, reply: oneshot::Sender<Result<(), &'static str>> },
}

#[derive(Debug)]
struct SteppedFrameOutcome {
    ticks: Vec<EcsSnapshot>,
    cancelled: bool,
}

enum EcsCommand {
    SteppedFrame {
        generation: u64,
        proposed: usize,
        catch_up_cap: usize,
        input: i32,
        permits: mpsc::Receiver<DomainPermit>,
        observation: tokio::sync::mpsc::Sender<EcsSnapshot>,
        completion: oneshot::Sender<Result<SteppedFrameOutcome, &'static str>>,
    },
    Tick {
        input: i32,
        completion: oneshot::Sender<Result<EcsSnapshot, &'static str>>,
    },
    Frame {
        proposed: usize,
        catch_up_cap: usize,
        input: i32,
        completion: oneshot::Sender<Result<Vec<EcsSnapshot>, &'static str>>,
    },
    Stop,
}

fn perform_ecs_tick(world: &mut World, runtime: &mut Runtime, input: i32)
    -> Result<EcsSnapshot, &'static str>
{
    world.resource_mut::<Input>().unwrap().0 = input;
    runtime.run_schedule::<Simulation>(world)
        .map_err(|_| "RunenECS schedule failed")?;
    let mut positions: Vec<_> = world.query::<&Position>()
        .iter(&*world).map(|position| position.0).collect();
    positions.sort_unstable();
    Ok(EcsSnapshot {
        tick: world.resource::<TickCount>().unwrap().0,
        positions,
        invoker_thread_calls: world.resource::<LocalOnly>().unwrap().0.get(),
    })
}

/// The actual ECS World/Runtime/!Send resource are created and accessed only
/// on this thread. A Send future never receives borrowed world state.
struct EcsOwner {
    sender: SyncSender<EcsCommand>,
    worker: Option<JoinHandle<()>>,
}
impl EcsOwner {
    fn start(owner: TickOwner) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<EcsCommand>(1);
        let worker = std::thread::spawn(move || {
            let mut world = World::new();
            world.spawn(Position(0)).unwrap();
            world.insert_resource(Input(0));
            world.insert_resource(TickCount(0));
            world.insert_resource(LocalOnly(Rc::new(Cell::new(0))));

            let mut runtime = Runtime::new();
            runtime.add_systems(Simulation, (
                queue_spawn.in_set(Spawn),
                move_positions.in_set(Movement).after(Spawn),
                finish_tick.on_invoker_thread().after(Movement),
            )).unwrap();

            for command in receiver {
                match command {
                    EcsCommand::Tick { input, completion } => {
                        let result = if matches!(owner, TickOwner::Kernel) {
                            perform_ecs_tick(&mut world, &mut runtime, input)
                        } else {
                            Err("the domain owns tick admission in this session")
                        };
                        let _ = completion.send(result);
                    }
                    EcsCommand::Frame { proposed, catch_up_cap, input, completion } => {
                        // H: this coordinator, not the outer graph, grants tick admission.
                        // The one frame call is intentionally indivisible from outside.
                        let result = if matches!(owner, TickOwner::Domain) {
                            (0..proposed.min(catch_up_cap))
                                .map(|_| perform_ecs_tick(&mut world, &mut runtime, input))
                                .collect()
                        } else {
                            Err("the kernel owns tick admission in this session")
                        };
                        let _ = completion.send(result);
                    }
                    EcsCommand::SteppedFrame {
                        generation, proposed, catch_up_cap, input, permits, observation, completion,
                    } => {
                        // The domain owns this frame and *each* internal tick safe point.
                        // The outer graph does not invent per-ECS-system pause semantics.
                        let outcome = (|| -> Result<SteppedFrameOutcome, &'static str> {
                            if !matches!(owner, TickOwner::Domain) {
                                return Err("the kernel owns tick admission in this session");
                            }
                            let mut ticks = Vec::new();
                            for _ in 0..proposed.min(catch_up_cap) {
                                loop {
                                    match permits.recv() {
                                        Ok(DomainPermit::Advance { generation: provided, reply }) => {
                                            if provided != generation {
                                                let _ = reply.send(Err("stale session generation"));
                                                continue;
                                            }
                                            let _ = reply.send(Ok(()));
                                            break;
                                        }
                                        Ok(DomainPermit::Cancel { generation: provided, reply }) => {
                                            if provided != generation {
                                                let _ = reply.send(Err("stale session generation"));
                                                continue;
                                            }
                                            let _ = reply.send(Ok(()));
                                            return Ok(SteppedFrameOutcome { ticks, cancelled: true });
                                        }
                                        Err(_) => {
                                            return Ok(SteppedFrameOutcome { ticks, cancelled: true });
                                        }
                                    }
                                }
                                let snapshot = perform_ecs_tick(&mut world, &mut runtime, input)?;
                                // A disconnected or slow observer must not own scheduling.
                                // The authoritative return value retains the completed ticks.
                                let _ = observation.try_send(snapshot.clone());
                                ticks.push(snapshot);
                            }
                            Ok(SteppedFrameOutcome { ticks, cancelled: false })
                        })();
                        let _ = completion.send(outcome);
                    }
                    EcsCommand::Stop => break,
                }
            }
        });
        Self { sender, worker: Some(worker) }
    }
}
impl Drop for EcsOwner {
    fn drop(&mut self) {
        let _ = self.sender.send(EcsCommand::Stop);
        if let Some(worker) = self.worker.take() {
            worker.join().expect("ECS owner thread must stop cleanly");
        }
    }
}
async fn domain_tick(sender: SyncSender<EcsCommand>, input: i32) -> Result<EcsSnapshot, &'static str> {
    let (completion, receiver) = oneshot::channel();
    sender.try_send(EcsCommand::Tick { input, completion })
        .map_err(|_| "bounded domain command queue full or closed")?;
    receiver.await.map_err(|_| "domain owner exited before reply")?
}

async fn domain_frame(sender: SyncSender<EcsCommand>, proposed: usize, catch_up_cap: usize, input: i32)
    -> Result<Vec<EcsSnapshot>, &'static str>
{
    let (completion, receiver) = oneshot::channel();
    sender.try_send(EcsCommand::Frame { proposed, catch_up_cap, input, completion })
        .map_err(|_| "bounded domain frame queue full or closed")?;
    receiver.await.map_err(|_| "domain owner exited before frame reply")?
}

#[derive(Clone, Copy)]
enum TickOwner { Kernel, Domain }

/// One reference scenario: identical world, tick input, frame policies and
/// actual ECS operations. Only admission/control authority differs.
async fn drive_ticks(owner: TickOwner) -> (Vec<EcsSnapshot>, usize) {
    let ecs = EcsOwner::start(owner);
    let seen = Arc::new(Mutex::new(Vec::<EcsSnapshot>::new()));
    let sender = ecs.sender.clone();
    let observed = Arc::clone(&seen);
    let plan = Arc::new(Plan::validate(vec![
        Node::new("read_input", &[], vec![], operation(|| async { Ok(()) })),
        Node::new("ecs_tick", &["read_input"], vec![Access::Write("ecs-world")], operation(move || {
            let sender = sender.clone();
            let observed = Arc::clone(&observed);
            async move {
                let snapshot = domain_tick(sender, 2).await?;
                observed.lock().unwrap().push(snapshot);
                Ok(())
            }
        })),
        Node::new("publish_snapshot", &["ecs_tick"], vec![Access::Read("ecs-world")], noop()),
    ]).unwrap());

    let mut run_number = 0;
    let mut visible_kernel_ticks = 0;
    for proposed_ticks in [0_usize, 1, 4] {
        match owner {
            TickOwner::Kernel => {
                // C: the outer policy grants each separate simulation-tick run.
                for _ in 0..proposed_ticks.min(2) {
                    run_number += 1;
                    let mut run = Run::new(Arc::clone(&plan), 123, run_number, 2, Trace::new(true, 16));
                    run.advance(1).await;
                    assert_eq!(run.state("ecs_tick"), Some(Status::Pending));
                    assert_eq!(seen.lock().unwrap().len(), visible_kernel_ticks);
                    run.advance(1).await;
                    assert_eq!(run.state("ecs_tick"), Some(Status::Completed));
                    run.advance(1).await;
                    assert_eq!(run.state("publish_snapshot"), Some(Status::Completed));
                    assert!(run.snapshot().iter().any(|(id, state)|
                        *id == "ecs_tick" && *state == Status::Completed));
                    visible_kernel_ticks += 1;
                }
            }
            TickOwner::Domain => {
                // H: the domain grants 0/N ticks inside one frame command.
                // Riesel's generic Run does not own these tick nodes or safe points.
                let snapshots = domain_frame(ecs.sender.clone(), proposed_ticks, 2, 2)
                    .await.unwrap();
                seen.lock().unwrap().extend(snapshots);
            }
        }
    }
    let result = seen.lock().unwrap().clone();
    (result, visible_kernel_ticks)
}

async fn event_driven_async_workflow() -> usize {
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let a = Arc::clone(&barrier);
    let b = Arc::clone(&barrier);
    let plan = Arc::new(Plan::validate(vec![
        Node::new("on_message", &[], vec![], noop()).triggered("message"),
        Node::new("io_a", &["on_message"], vec![], operation(move || {
            let b = Arc::clone(&a);
            async move { b.wait().await; Ok(()) }
        })),
        Node::new("io_b", &["on_message"], vec![], operation(move || {
            let b = Arc::clone(&b);
            async move { b.wait().await; Ok(()) }
        })),
        Node::new("join", &["io_a", "io_b"], vec![], noop()),
    ]).unwrap());
    let mut run = Run::new(plan, 123, 99, 2, Trace::new(true, 16));
    run.advance(usize::MAX).await;
    assert_eq!(run.state("join"), Some(Status::Pending));
    run.fire("message");
    tokio::time::timeout(std::time::Duration::from_secs(2), run.advance(usize::MAX))
        .await.expect("independent IO branches must finish");
    assert_eq!(run.state("join"), Some(Status::Completed));
    assert_eq!(run.peak_in_flight, 2);
    run.launched
}

#[tokio::test(flavor = "current_thread")]
async fn actual_runen_ecs_c_and_h_authority_probe_with_simultaneous_async_workflow() {
    let ((c, c_visible), c_io) = tokio::join!(
        drive_ticks(TickOwner::Kernel), event_driven_async_workflow()
    );
    let ((h, h_visible), h_io) = tokio::join!(
        drive_ticks(TickOwner::Domain), event_driven_async_workflow()
    );
    assert_eq!(c, h, "changing outer ownership must not change ECS semantics");
    assert_eq!(c_io, h_io);
    assert_eq!(c_io, 4);
    assert_eq!(c_visible, 3, "C exposes three graph-owned tick nodes");
    assert_eq!(h_visible, 0, "H needs a domain adapter for unified graph inspection");
    assert_eq!(c, vec![
        EcsSnapshot { tick: 1, positions: vec![2, 12], invoker_thread_calls: 1 },
        EcsSnapshot { tick: 2, positions: vec![4, 12, 14], invoker_thread_calls: 2 },
        EcsSnapshot { tick: 3, positions: vec![6, 12, 14, 16], invoker_thread_calls: 3 },
    ]);
}


/// The adapter forbids mixed authorities even though its internal message type
/// supports both commands. A second dispatcher cannot silently double-step.
#[tokio::test(flavor = "current_thread")]
async fn domain_actor_rejects_conflicting_tick_admission_modes() {
    let kernel_owned = EcsOwner::start(TickOwner::Kernel);
    assert_eq!(
        domain_frame(kernel_owned.sender.clone(), 1, 2, 2).await,
        Err("the kernel owns tick admission in this session")
    );
    assert_eq!(domain_tick(kernel_owned.sender.clone(), 2).await.unwrap().tick, 1);

    let domain_owned = EcsOwner::start(TickOwner::Domain);
    assert_eq!(
        domain_tick(domain_owned.sender.clone(), 2).await,
        Err("the domain owns tick admission in this session")
    );
    assert_eq!(
        domain_frame(domain_owned.sender.clone(), 1, 2, 2).await.unwrap()[0].tick,
        1
    );
}

/// C's strongest claim is ONE outer graph that can admit both an ECS tick and
/// triggered async work, while never attempting to schedule internal systems.
#[tokio::test(flavor = "current_thread")]
async fn single_outer_graph_coordinates_ecs_tick_and_async_join() {
    let ecs = EcsOwner::start(TickOwner::Kernel);
    let sender = ecs.sender.clone();
    let state = Arc::new(Mutex::new(None::<EcsSnapshot>));
    let result = Arc::clone(&state);
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let a = Arc::clone(&barrier);
    let b = Arc::clone(&barrier);
    let plan = Arc::new(Plan::validate(vec![
        Node::new("ecs_tick", &[], vec![Access::Write("ecs-world")], operation(move || {
            let sender = sender.clone();
            let result = Arc::clone(&result);
            async move {
                let snapshot = domain_tick(sender, 2).await?;
                *result.lock().unwrap() = Some(snapshot);
                Ok(())
            }
        })),
        Node::new("io_event", &[], vec![], noop()).triggered("message"),
        Node::new("io_a", &["io_event"], vec![], operation(move || {
            let a = Arc::clone(&a);
            async move { a.wait().await; Ok(()) }
        })),
        Node::new("io_b", &["io_event"], vec![], operation(move || {
            let b = Arc::clone(&b);
            async move { b.wait().await; Ok(()) }
        })),
        Node::new("combined_join", &["ecs_tick", "io_a", "io_b"], vec![], noop()),
    ]).unwrap());
    let mut run = Run::new(plan, 123, 100, 3, Trace::new(true, 32));
    run.fire("message");
    tokio::time::timeout(std::time::Duration::from_secs(2), run.advance(usize::MAX))
        .await.expect("combined ECS+IO graph must complete");
    assert_eq!(run.state("combined_join"), Some(Status::Completed));
    assert!(run.peak_in_flight >= 2, "independent IO branches must overlap");
    assert_eq!(state.lock().unwrap().as_ref().unwrap().positions, vec![2, 12]);
    assert_eq!(run.snapshot().len(), 5);
    assert!(run.trace.retained.iter().any(|e|
        e.node == "ecs_tick" && e.status == Status::Completed));
}


/// Tests the H adapter's *real* per-tick safe points, not an arbitrary Rust
/// system debugger. A paused domain frame is independently controlled and
/// observed while the ECS World and !Send resource remain on their owner thread.
#[tokio::test(flavor = "current_thread")]
async fn domain_owned_frame_supports_fenced_cooperative_tick_stepping_and_partial_cancel() {
    let ecs = EcsOwner::start(TickOwner::Domain);
    let (permits, permit_rx) = mpsc::sync_channel::<DomainPermit>(1);
    let (observation_tx, mut observations) = tokio::sync::mpsc::channel::<EcsSnapshot>(4);
    let (completion_tx, completion_rx) = oneshot::channel();
    ecs.sender.try_send(EcsCommand::SteppedFrame {
        generation: 712,
        proposed: 4,
        catch_up_cap: 3,
        input: 2,
        permits: permit_rx,
        observation: observation_tx,
        completion: completion_tx,
    }).map_err(|_| "bounded ECS owner queue unavailable").unwrap();

    let (stale_tx, stale_rx) = oneshot::channel();
    permits.try_send(DomainPermit::Advance { generation: 711, reply: stale_tx }).map_err(|_| "permit queue unavailable").unwrap();
    assert_eq!(stale_rx.await.unwrap(), Err("stale session generation"));
    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(20), observations.recv()
    ).await.is_err(), "stale step may not advance a tick");

    let (first_tx, first_rx) = oneshot::channel();
    permits.try_send(DomainPermit::Advance { generation: 712, reply: first_tx }).map_err(|_| "permit queue unavailable").unwrap();
    assert_eq!(first_rx.await.unwrap(), Ok(()));
    let first = tokio::time::timeout(std::time::Duration::from_secs(2), observations.recv())
        .await.unwrap().unwrap();
    assert_eq!(first.tick, 1);
    assert_eq!(first.positions, vec![2, 12]);
    assert_eq!(first.invoker_thread_calls, 1);

    assert!(tokio::time::timeout(
        std::time::Duration::from_millis(20), observations.recv()
    ).await.is_err(), "one permit must not execute two simulation ticks");

    let (second_tx, second_rx) = oneshot::channel();
    permits.try_send(DomainPermit::Advance { generation: 712, reply: second_tx }).map_err(|_| "permit queue unavailable").unwrap();
    assert_eq!(second_rx.await.unwrap(), Ok(()));
    let second = tokio::time::timeout(std::time::Duration::from_secs(2), observations.recv())
        .await.unwrap().unwrap();
    assert_eq!(second.tick, 2);
    assert_eq!(second.positions, vec![4, 12, 14]);

    let (cancel_tx, cancel_rx) = oneshot::channel();
    permits.try_send(DomainPermit::Cancel { generation: 712, reply: cancel_tx }).map_err(|_| "permit queue unavailable").unwrap();
    assert_eq!(cancel_rx.await.unwrap(), Ok(()));
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), completion_rx)
        .await.unwrap().unwrap().unwrap();
    assert!(result.cancelled);
    assert_eq!(result.ticks, vec![first, second], "completed ticks cannot be rolled back");
    assert_eq!(domain_frame(ecs.sender.clone(), 1, 2, 2).await.unwrap()[0].tick, 3,
        "cancelling a frame cannot erase already published ECS state");
}

/// Diagnostics are bounded and lossy. Dropped ECS snapshots are not an
/// authoritative domain-state transition nor permission to retry a tick.
#[tokio::test(flavor = "current_thread")]
async fn slow_domain_observer_cannot_stall_tick_execution() {
    let ecs = EcsOwner::start(TickOwner::Domain);
    let (permits, permit_rx) = mpsc::sync_channel::<DomainPermit>(1);
    let (observation_tx, _unread) = tokio::sync::mpsc::channel::<EcsSnapshot>(1);
    let (completion_tx, completion_rx) = oneshot::channel();
    ecs.sender.try_send(EcsCommand::SteppedFrame {
        generation: 812,
        proposed: 3,
        catch_up_cap: 3,
        input: 2,
        permits: permit_rx,
        observation: observation_tx,
        completion: completion_tx,
    }).map_err(|_| "bounded ECS owner queue unavailable").unwrap();
    for _ in 0..3 {
        let (response_tx, response_rx) = oneshot::channel();
        permits.send(DomainPermit::Advance { generation: 812, reply: response_tx })
            .map_err(|_| "permit queue unavailable").unwrap();
        assert_eq!(response_rx.await.unwrap(), Ok(()));
    }
    let result = tokio::time::timeout(std::time::Duration::from_secs(2), completion_rx)
        .await.unwrap().unwrap().unwrap();
    assert!(!result.cancelled);
    assert_eq!(result.ticks.len(), 3);
    assert_eq!(result.ticks.last().unwrap().tick, 3);
}
