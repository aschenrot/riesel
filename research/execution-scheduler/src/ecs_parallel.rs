//! Issue #83: comparable Bevy ECS / RunenECS public-API query and scheduling workloads.
//! Research only. Deliberately excludes rendering, Riesel transport and provider ownership.
//! A serial, unordered or parallel *commutative* result is comparable; system order is not.
use std::hint::black_box;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine { Bevy, Runen }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workload { Empty, Reads, IndependentWrites, ConflictingWrites }

impl Workload {
    pub const ALL: [Self; 4] = [
        Self::Empty, Self::Reads, Self::IndependentWrites, Self::ConflictingWrites
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Reads => "reads",
            Self::IndependentWrites => "independent_writes",
            Self::ConflictingWrites => "conflicting_writes",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Case {
    pub workload: Workload,
    pub entities: usize,
    pub parallel: bool,
}

pub enum Fixture {
    Bevy(bevy::Fixture),
    Runen(runen::Fixture),
}

impl Fixture {
    pub fn new(engine: Engine, case: Case) -> Self {
        match engine {
            Engine::Bevy => Self::Bevy(bevy::Fixture::new(case)),
            Engine::Runen => Self::Runen(runen::Fixture::new(case)),
        }
    }
    pub fn run(&mut self) {
        match self {
            Self::Bevy(f) => f.run(),
            Self::Runen(f) => f.run(),
        }
    }
    pub fn sums(&mut self) -> [u64; 4] {
        match self {
            Self::Bevy(f) => f.sums(),
            Self::Runen(f) => f.sums(),
        }
    }
}

mod bevy {
    use super::*;
    use bevy_ecs::prelude::*;
    use bevy_ecs::schedule::{MultiThreadedExecutor, ScheduleLabel, SingleThreadedExecutor};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ScheduleLabel)]
    struct Measure;

    macro_rules! lane {
        ($component:ident, $reader:ident, $writer:ident) => {
            #[derive(Component)]
            struct $component(u64);

            fn $reader(query: Query<&$component>) {
                let checksum = query.iter().fold(0u64, |sum, v| sum.wrapping_add(v.0));
                black_box(checksum);
            }
            fn $writer(mut query: Query<&mut $component>) {
                let mut checksum = 0u64;
                for mut value in &mut query {
                    value.0 = value.0.wrapping_add(1);
                    checksum = checksum.wrapping_add(value.0);
                }
                black_box(checksum);
            }
        };
    }

    lane!(C0, read_0, write_0);
    lane!(C1, read_1, write_1);
    lane!(C2, read_2, write_2);
    lane!(C3, read_3, write_3);

    macro_rules! conflict {
        ($name:ident) => {
            fn $name(mut query: Query<&mut C0>) {
                let mut checksum = 0u64;
                for mut value in &mut query {
                    value.0 = value.0.wrapping_add(1);
                    checksum = checksum.wrapping_add(value.0);
                }
                black_box(checksum);
            }
        };
    }
    conflict!(conflict_1);
    conflict!(conflict_2);
    conflict!(conflict_3);

    fn empty_0() {}
    fn empty_1() {}
    fn empty_2() {}
    fn empty_3() {}

    pub struct Fixture {
        world: World,
        schedule: Schedule,
    }

    impl Fixture {
        pub fn new(case: Case) -> Self {
            let mut world = World::new();
            for index in 0..case.entities {
                world.spawn((C0(index as u64), C1(1), C2(2), C3(3)));
            }

            let mut schedule = Schedule::new(Measure);
            if case.parallel {
                // Bevy has a globally configured CPU pool, unlike Runen's per-call
                // worker-capacity argument. Both measured modes use four workers.
                bevy_tasks::ComputeTaskPool::get_or_init(|| {
                    bevy_tasks::TaskPoolBuilder::new().num_threads(4).build()
                });
                schedule.set_executor(MultiThreadedExecutor::new());
            } else {
                schedule.set_executor(SingleThreadedExecutor::new());
            }
            match case.workload {
                Workload::Empty => schedule.add_systems((empty_0, empty_1, empty_2, empty_3)),
                Workload::Reads => schedule.add_systems((read_0, read_1, read_2, read_3)),
                Workload::IndependentWrites => schedule.add_systems((write_0, write_1, write_2, write_3)),
                Workload::ConflictingWrites =>
                    schedule.add_systems((write_0, conflict_1, conflict_2, conflict_3)),
            };
            // Force schedule compilation outside the timed region.
            schedule.initialize(&mut world).unwrap();
            Self { world, schedule }
        }

        pub fn run(&mut self) {
            self.schedule.run(&mut self.world);
        }

        pub fn sums(&mut self) -> [u64; 4] {
            let mut q0 = self.world.query::<&C0>();
            let a = q0.iter(&self.world).map(|v| v.0).sum();
            let mut q1 = self.world.query::<&C1>();
            let b = q1.iter(&self.world).map(|v| v.0).sum();
            let mut q2 = self.world.query::<&C2>();
            let c = q2.iter(&self.world).map(|v| v.0).sum();
            let mut q3 = self.world.query::<&C3>();
            let d = q3.iter(&self.world).map(|v| v.0).sum();
            [a, b, c, d]
        }
    }
}

mod runen {
    use super::*;
    use runen_ecs::prelude::*;

    #[derive(ScheduleLabel)]
    struct Measure;

    macro_rules! lane {
        ($component:ident, $reader:ident, $writer:ident) => {
            #[derive(Component)]
            struct $component(u64);

            fn $reader(mut query: Query<&$component>) {
                let checksum = query.iter().fold(0u64, |sum, v| sum.wrapping_add(v.0));
                black_box(checksum);
            }
            fn $writer(mut query: Query<&mut $component>) {
                let mut checksum = 0u64;
                for value in query.iter() {
                    value.0 = value.0.wrapping_add(1);
                    checksum = checksum.wrapping_add(value.0);
                }
                black_box(checksum);
            }
        };
    }
    lane!(C0, read_0, write_0);
    lane!(C1, read_1, write_1);
    lane!(C2, read_2, write_2);
    lane!(C3, read_3, write_3);

    macro_rules! conflict {
        ($name:ident) => {
            fn $name(mut query: Query<&mut C0>) {
                let mut checksum = 0u64;
                for value in query.iter() {
                    value.0 = value.0.wrapping_add(1);
                    checksum = checksum.wrapping_add(value.0);
                }
                black_box(checksum);
            }
        };
    }
    conflict!(conflict_1);
    conflict!(conflict_2);
    conflict!(conflict_3);

    fn empty_0() {}
    fn empty_1() {}
    fn empty_2() {}
    fn empty_3() {}

    pub struct Fixture {
        world: World,
        runtime: Runtime,
        parallel: bool,
    }

    impl Fixture {
        pub fn new(case: Case) -> Self {
            let mut world = World::new();
            for index in 0..case.entities {
                world.spawn((C0(index as u64), C1(1), C2(2), C3(3))).unwrap();
            }
            let mut runtime = Runtime::new();
            match case.workload {
                Workload::Empty => runtime.add_systems(Measure, (empty_0, empty_1, empty_2, empty_3)),
                Workload::Reads => runtime.add_systems(Measure, (read_0, read_1, read_2, read_3)),
                Workload::IndependentWrites => runtime.add_systems(Measure, (write_0, write_1, write_2, write_3)),
                Workload::ConflictingWrites =>
                    runtime.add_systems(Measure, (write_0, conflict_1, conflict_2, conflict_3)),
            }.unwrap();
            runtime.validate().unwrap();
            Self { world, runtime, parallel: case.parallel }
        }

        pub fn run(&mut self) {
            if self.parallel {
                self.runtime.run_schedule_parallel::<Measure>(&mut self.world, 4).unwrap();
            } else {
                self.runtime.run_schedule::<Measure>(&mut self.world).unwrap();
            }
        }

        pub fn sums(&mut self) -> [u64; 4] {
            [
                self.world.query::<&C0>().iter(&self.world).map(|v| v.0).sum(),
                self.world.query::<&C1>().iter(&self.world).map(|v| v.0).sum(),
                self.world.query::<&C2>().iter(&self.world).map(|v| v.0).sum(),
                self.world.query::<&C3>().iter(&self.world).map(|v| v.0).sum(),
            ]
        }
    }
}

pub fn expected_sums(case: Case, runs: u64) -> [u64; 4] {
    let n = case.entities as u64;
    let base = [n.saturating_sub(1) * n / 2, n, 2*n, 3*n];
    match case.workload {
        Workload::Empty | Workload::Reads => base,
        Workload::IndependentWrites =>
            [base[0] + n*runs, base[1] + n*runs, base[2] + n*runs, base[3] + n*runs],
        Workload::ConflictingWrites => [base[0] + 4*n*runs, base[1], base[2], base[3]],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_and_parallel_engines_observe_identical_effects_without_relying_on_order() {
        for entities in [0usize, 64, 512] {
            for workload in Workload::ALL {
                let mut reference: Option<[u64; 4]> = None;
                for parallel in [false, true] {
                    for engine in [Engine::Bevy, Engine::Runen] {
                        let case = Case { workload, entities, parallel };
                        let mut fixture = Fixture::new(engine, case);
                        assert_eq!(fixture.sums(), expected_sums(case, 0));
                        for tick in 1..=3 {
                            fixture.run();
                            let snapshot = fixture.sums();
                            assert_eq!(snapshot, expected_sums(case, tick),
                                "unexpected write result for {engine:?} {case:?} tick={tick}");
                            match reference {
                                Some(previous) if tick == 3 => assert_eq!(snapshot, previous,
                                    "engine/mode parity failed: {engine:?} {case:?}"),
                                None if tick == 3 => reference = Some(snapshot),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
}
