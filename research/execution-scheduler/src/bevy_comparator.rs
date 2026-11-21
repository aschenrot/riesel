//! Issue #83: research-only Bevy ECS / RunenECS comparator.
//! Run identical *observable* fixed-tick work in two independent domain runtimes.
//! Do not interpret successful examples as performance or universal determinism proof.
use std::cell::Cell;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReferenceSnapshot {
    tick: u32,
    positions: Vec<i32>,
    invoker_only_calls: u32,
}

mod bevy_fixture {
    use super::*;
    use bevy_ecs::prelude::*;
    use bevy_ecs::schedule::{ScheduleLabel, SingleThreadedExecutor, Stepping};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ScheduleLabel)]
    pub struct Simulation;

    #[derive(Component)]
    struct Position(i32);
    #[derive(Resource)]
    struct Input(i32);
    #[derive(Resource)]
    struct Ticks(u32);
    struct LocalOnly(Rc<Cell<u32>>);

    fn queue_spawn(mut commands: Commands) {
        commands.spawn(Position(10));
    }
    fn integrate(mut positions: Query<&mut Position>, input: Res<Input>) {
        for mut position in &mut positions {
            position.0 += input.0;
        }
    }
    fn finish(mut ticks: ResMut<Ticks>, local: NonSend<LocalOnly>) {
        ticks.0 += 1;
        local.0.set(local.0.get() + 1);
    }

    pub fn execute() -> Vec<ReferenceSnapshot> {
        let mut world = World::new();
        world.spawn(Position(0));
        world.insert_resource(Input(2));
        world.insert_resource(Ticks(0));
        world.insert_non_send(LocalOnly(Rc::new(Cell::new(0))));

        // Deliberately serial: same comparator class as RunenECS serial reference.
        let mut schedule = Schedule::new(Simulation);
        schedule.set_executor(SingleThreadedExecutor::new());
        schedule.add_systems((queue_spawn, integrate, finish).chain());
        let mut snapshots = Vec::new();
        for proposed in [0_usize, 1, 4] {
            for _ in 0..proposed.min(2) {
                schedule.run(&mut world);
                let mut query = world.query::<&Position>();
                let mut positions: Vec<i32> = query.iter(&world).map(|v| v.0).collect();
                positions.sort_unstable(); // Query iteration order is not the parity oracle.
                snapshots.push(ReferenceSnapshot {
                    tick: world.resource::<Ticks>().0,
                    positions,
                    invoker_only_calls: world.get_non_send::<LocalOnly>().unwrap().0.get(),
                });
            }
        }
        assert!(schedule.systems().unwrap().count() >= 3,
            "schedule system inspection should expose the registered functions");
        snapshots
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ScheduleLabel)]
    struct StepLab;
    #[derive(Resource, Default)]
    struct StepLog(Vec<&'static str>);

    fn mark_a(mut log: ResMut<StepLog>) { log.0.push("a"); }
    fn mark_b(mut log: ResMut<StepLog>) { log.0.push("b"); }
    fn mark_c(mut log: ResMut<StepLog>) { log.0.push("c"); }

    /// Bevy's stepping requires the feature flag and a frame boundary. This is
    /// *not* interrupting a currently running system, and is not a Riesel API.
    pub fn stepping_probe() {
        let mut world = World::new();
        world.insert_resource(StepLog::default());
        world.insert_resource(Stepping::new());

        let mut begin_frame = Schedule::default();
        begin_frame.add_systems(Stepping::begin_frame);

        let mut simulation = Schedule::new(StepLab);
        simulation.set_executor(SingleThreadedExecutor::new());
        simulation.add_systems((mark_a, mark_b, mark_c).chain());

        world.resource_mut::<Stepping>().add_schedule(StepLab).enable();
        begin_frame.run(&mut world);
        simulation.run(&mut world);
        assert!(world.resource::<StepLog>().0.is_empty(),
            "configured stepping should begin in waiting state");

        for expected in [["a"].as_slice(), ["a", "b"].as_slice(), ["a", "b", "c"].as_slice()] {
            world.resource_mut::<Stepping>().step_frame();
            begin_frame.run(&mut world);
            simulation.run(&mut world);
            assert_eq!(world.resource::<StepLog>().0, expected);
        }

        // Breakpoints are separate from pause requests on a running system.
        let mut world = World::new();
        world.insert_resource(StepLog::default());
        world.insert_resource(Stepping::new());
        world.resource_mut::<Stepping>()
            .add_schedule(StepLab).enable()
            .set_breakpoint(StepLab, mark_b)
            .continue_frame();
        begin_frame.run(&mut world);
        let mut simulation = Schedule::new(StepLab);
        simulation.set_executor(SingleThreadedExecutor::new());
        simulation.add_systems((mark_a, mark_b, mark_c).chain());
        simulation.run(&mut world);
        assert_eq!(world.resource::<StepLog>().0, vec!["a"],
            "continue should halt before the configured system breakpoint");
    }
}

mod runen_fixture {
    use super::*;
    use runen_ecs::prelude::*;

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
    struct Ticks(u32);
    #[derive(Resource)]
    struct LocalOnly(Rc<Cell<u32>>);

    fn queue_spawn(mut commands: Commands) {
        commands.spawn(Position(10));
    }
    fn integrate(mut positions: Query<&mut Position>, input: Res<Input>) {
        for position in positions.iter() {
            position.0 += input.0;
        }
    }
    fn finish(mut ticks: ResMut<Ticks>, local: Res<LocalOnly>) {
        ticks.0 += 1;
        local.0.set(local.0.get() + 1);
    }

    pub fn execute() -> Vec<ReferenceSnapshot> {
        let mut world = World::new();
        world.spawn(Position(0)).unwrap();
        world.insert_resource(Input(2));
        world.insert_resource(Ticks(0));
        world.insert_resource(LocalOnly(Rc::new(Cell::new(0))));

        let mut runtime = Runtime::new();
        runtime.add_systems(Simulation, (
            queue_spawn.in_set(Spawn),
            integrate.in_set(Movement).after(Spawn),
            finish.on_invoker_thread().after(Movement),
        )).unwrap();

        let description = runtime.inspect_schedule::<Simulation>().unwrap().unwrap();
        assert_eq!(description.systems().len(), 3);
        let mut snapshots = Vec::new();
        for proposed in [0_usize, 1, 4] {
            for _ in 0..proposed.min(2) {
                runtime.run_schedule::<Simulation>(&mut world).unwrap();
                let mut positions: Vec<i32> = world.query::<&Position>()
                    .iter(&world).map(|v| v.0).collect();
                positions.sort_unstable();
                snapshots.push(ReferenceSnapshot {
                    tick: world.resource::<Ticks>().unwrap().0,
                    positions,
                    invoker_only_calls: world.resource::<LocalOnly>().unwrap().0.get(),
                });
            }
        }
        snapshots
    }
}

#[test]
fn same_fixed_tick_workload_and_non_send_state_in_two_ecs_engines() {
    let bevy = bevy_fixture::execute();
    let runen = runen_fixture::execute();
    assert_eq!(bevy, runen,
        "different ECS engines should agree on the explicit serial workload");
    assert_eq!(bevy, vec![
        ReferenceSnapshot { tick: 1, positions: vec![2, 12], invoker_only_calls: 1 },
        ReferenceSnapshot { tick: 2, positions: vec![4, 12, 14], invoker_only_calls: 2 },
        ReferenceSnapshot { tick: 3, positions: vec![6, 12, 14, 16], invoker_only_calls: 3 },
    ]);
}

#[test]
fn bevy_stepping_and_breakpoint_api_has_actual_effect() {
    bevy_fixture::stepping_probe();
}
