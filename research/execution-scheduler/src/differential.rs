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

enum EcsCommand {
    Tick {
        input: i32,
        completion: oneshot::Sender<Result<EcsSnapshot, &'static str>>,
    },
    Stop,
}

/// The actual ECS World/Runtime/!Send resource are created and accessed only
/// on this thread. A Send future never receives borrowed world state.
struct EcsOwner {
    sender: SyncSender<EcsCommand>,
    worker: Option<JoinHandle<()>>,
}
impl EcsOwner {
    fn start() -> Self {
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
                        world.resource_mut::<Input>().unwrap().0 = input;
                        let outcome = runtime.run_schedule::<Simulation>(&mut world);
                        let result = outcome.map(|()| {
                            let mut positions: Vec<_> = world.query::<&Position>()
                                .iter(&world).map(|position| position.0).collect();
                            positions.sort_unstable(); // ECS iteration order is not an API contract.
                            EcsSnapshot {
                                tick: world.resource::<TickCount>().unwrap().0,
                                positions,
                                invoker_thread_calls: world.resource::<LocalOnly>().unwrap().0.get(),
                            }
                        }).map_err(|_| "RunenECS schedule failed");
                        let _ = completion.send(result);
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

#[derive(Clone, Copy)]
enum TickOwner { Kernel, Domain }

/// One reference scenario: identical world, tick input, frame policies and
/// actual ECS operations. Only admission/control authority differs.
async fn drive_ticks(owner: TickOwner) -> (Vec<EcsSnapshot>, usize) {
    let ecs = EcsOwner::start();
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
        // The same domain frame policy limits catch-up in both cases.
        for _ in 0..proposed_ticks.min(2) {
            run_number += 1;
            match owner {
                TickOwner::Kernel => {
                    let mut run = Run::new(Arc::clone(&plan), 123, run_number, 2, Trace::new(true, 16));
                    run.advance(1).await;
                    assert_eq!(run.state("ecs_tick"), Some(Status::Pending));
                    // One outer operation step cannot advance an internal ECS system.
                    assert_eq!(seen.lock().unwrap().len(), visible_kernel_ticks);
                    run.advance(1).await;
                    assert_eq!(run.state("ecs_tick"), Some(Status::Completed));
                    run.advance(1).await;
                    assert_eq!(run.state("publish_snapshot"), Some(Status::Completed));
                    let authoritative = run.snapshot();
                    assert!(authoritative.iter().any(|(id, state)|
                        *id == "ecs_tick" && *state == Status::Completed));
                    visible_kernel_ticks += 1;
                }
                TickOwner::Domain => {
                    // Domain policy owns admission; no second graph can also
                    // claim to schedule or stop this tick.
                    let snapshot = domain_tick(ecs.sender.clone(), 2).await.unwrap();
                    seen.lock().unwrap().push(snapshot);
                    // A shared inspector needs an explicit domain adapter:
                    // this domain tick has no entry in the generic Run state.
                }
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
