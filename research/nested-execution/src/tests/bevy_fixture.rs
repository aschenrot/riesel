//! Standalone Bevy ECS 0.20.1 as an independently owned domain, not a
//! replacement for Riesel's generic graph runner or a public Bevy adapter.
//! This deliberately matches the existing real RunenECS fixture's world
//! effects, per-owner-epoch controls, partial effects and uncertain outcomes.

use super::*;
use bevy_ecs::prelude::*;
use bevy_ecs::schedule::{ScheduleLabel, SingleThreadedExecutor, Stepping};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ScheduleLabel)]
struct Simulation;

#[derive(Component)]
struct Position(i32);

#[derive(Resource)]
struct Input(i32);

#[derive(Resource)]
struct Ticks(u32);

// Native Bevy NonSend<T> remains on the thread that owns this Bevy World.
struct LocalOnly(Rc<Cell<u32>>);

fn integrate(mut query: Query<&mut Position>, input: Res<Input>) {
    for mut position in &mut query {
        position.0 += input.0;
    }
}

fn finish(mut ticks: ResMut<Ticks>, local: NonSend<LocalOnly>) {
    ticks.0 += 1;
    local.0.set(local.0.get() + 1);
}

/// This owner is created, used, and dropped inside DomainActor's OS thread.
pub(super) struct Owned {
    world: World,
    schedule: Schedule,
}

impl Owned {
    pub(super) fn new() -> Self {
        let mut world = World::new();
        world.spawn(Position(0));
        world.insert_resource(Input(2));
        world.insert_resource(Ticks(0));
        world.insert_non_send(LocalOnly(Rc::new(Cell::new(0))));

        let mut schedule = Schedule::new(Simulation);
        schedule.set_executor(SingleThreadedExecutor::new());
        schedule.add_systems((integrate, finish).chain());
        Self { world, schedule }
    }

    pub(super) fn tick(&mut self) -> i32 {
        self.schedule.run(&mut self.world);
        let count = self.world.resource::<Ticks>().0;
        let calls = self.world.get_non_send::<LocalOnly>().unwrap().0.get();
        assert_eq!(count, calls, "only the owner thread runs the Bevy system");
        let mut query = self.world.query::<&Position>();
        let positions: Vec<_> = query.iter(&self.world).map(|p| p.0).collect();
        assert_eq!(positions, vec![2 * count as i32]);
        positions[0]
    }
}

#[test]
fn actual_bevy_ecs_delegated_frame_preserves_partial_committed_tick() {
    executor().block_on(async {
        let setup = setup(Kind::ActualBevyEcsTick, 2, false);
        let controller = setup.runner.controller();
        assert!(matches!(
            controller.pause(GraphControlTarget::RieselDomainTicks("domain".into())),
            Err(GraphControlError::UnsupportedDomainAuthority { .. })
        ));
        let task = tokio::spawn(setup.runner.execute());
        until(|| {
            setup.independent.load(Ordering::SeqCst) == 1
                && setup.observer.snapshot().operations["frame"].state == OperationState::Running
        })
        .await;

        // The identical outer graph admission contract cannot step or
        // fabricate one of Bevy's internal systems. Only the domain owner
        // can advertise and act on a supported inner safe point.
        assert_eq!(
            grant(&setup.permits, OWNER_EPOCH, Kind::ActualRunenEcsTick).await,
            Err(Error::UnsupportedSafePoint)
        );
        assert_eq!(
            grant(&setup.permits, OWNER_EPOCH - 1, Kind::ActualBevyEcsTick).await,
            Err(Error::StaleOwnerEpoch)
        );
        assert_eq!(setup.actor.observer.snapshot().cumulative_world, 0);
        grant(&setup.permits, OWNER_EPOCH, Kind::ActualBevyEcsTick)
            .await
            .unwrap();
        let domain = setup.actor.observer.snapshot();
        assert_eq!(domain.kind, Kind::ActualBevyEcsTick);
        assert_eq!(domain.cumulative_world, 2);
        assert_eq!(domain.committed[&("frame".into(), 1)], 1);
        assert_eq!(domain.latest_sequence, 1);
        assert_eq!(
            setup.observer.snapshot().operations["frame"].state,
            OperationState::Running
        );
        cancel(&setup.permits, OWNER_EPOCH).await.unwrap();
        let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
        assert_eq!(report.operations["frame"], OperationState::Cancelled);
        assert_eq!(report.operations["after"], OperationState::Blocked);
        assert_eq!(report.confirmed_prefixes["frame"]["domain.committed"], 1);
        assert!(!report.operations.contains_key("bevy-system"));
        assert_eq!(setup.downstream.load(Ordering::SeqCst), 0);
        assert_eq!(domain.parent_run, report.identity);
        assert_eq!(setup.actor.stop(), 2);
    });
}

#[test]
fn actual_bevy_ecs_lost_reply_preserves_owner_snapshot_not_graph_confirmation() {
    executor().block_on(async {
        let setup = setup(Kind::ActualBevyEcsTick, 2, true);
        let task = tokio::spawn(setup.runner.execute());
        until(|| setup.observer.snapshot().operations["frame"].state == OperationState::Running)
            .await;
        for _ in 0..2 {
            grant(&setup.permits, OWNER_EPOCH, Kind::ActualBevyEcsTick)
                .await
                .unwrap();
        }
        let report = tokio::time::timeout(WAIT, task).await.unwrap().unwrap();
        assert_eq!(report.operations["frame"], OperationState::OutcomeUnknown);
        assert_eq!(report.operations["after"], OperationState::Blocked);
        assert!(!report.confirmed_prefixes.contains_key("frame"));
        let domain = setup.actor.observer.snapshot();
        assert_eq!(domain.parent_run, report.identity);
        assert_eq!(domain.latest_sequence, 2);
        assert_eq!(domain.committed[&("frame".into(), 1)], 2);
        assert_eq!(domain.cumulative_world, 4);
        assert_eq!(setup.downstream.load(Ordering::SeqCst), 0);
        assert_eq!(setup.actor.stop(), 4);
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ScheduleLabel)]
struct StepLab;

#[derive(Resource, Default)]
struct StepLog(Vec<&'static str>);

fn mark_a(mut log: ResMut<StepLog>) {
    log.0.push("a");
}
fn mark_b(mut log: ResMut<StepLog>) {
    log.0.push("b");
}
fn mark_c(mut log: ResMut<StepLog>) {
    log.0.push("c");
}

#[test]
fn bevy_native_system_stepping_remains_domain_owned_and_feature_gated() {
    // This is Bevy's OWN stepping API inside its scheduler, not a Riesel
    // controller/registered graph-system operation. #84 independently
    // demonstrated breakpoint and schedule WorldId lifecycle semantics.
    let mut world = World::new();
    world.insert_resource(StepLog::default());
    world.insert_resource(Stepping::new());

    let mut begin_frame = Schedule::default();
    begin_frame.add_systems(Stepping::begin_frame);
    let mut simulation = Schedule::new(StepLab);
    simulation.set_executor(SingleThreadedExecutor::new());
    simulation.add_systems((mark_a, mark_b, mark_c).chain());

    world
        .resource_mut::<Stepping>()
        .add_schedule(StepLab)
        .enable();
    begin_frame.run(&mut world);
    simulation.run(&mut world);
    assert!(world.resource::<StepLog>().0.is_empty());

    for expected in [
        ["a"].as_slice(),
        ["a", "b"].as_slice(),
        ["a", "b", "c"].as_slice(),
    ] {
        world.resource_mut::<Stepping>().step_frame();
        begin_frame.run(&mut world);
        simulation.run(&mut world);
        assert_eq!(world.resource::<StepLog>().0, expected);
    }
}
