//! Issue #83 research-only release-profile sampling driver. No benchmark API is adopted.
//! Runs matched Bevy ECS / RunenECS public-API schedules with 4 configured workers.
//! Timing on shared GitHub-hosted CPUs is exploratory and NOT a performance SLA.
#[path = "../src/ecs_parallel.rs"]
mod ecs_parallel;

use ecs_parallel::{Case, Engine, Fixture, Workload, expected_sums};
use std::time::Instant;

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(|a, b| a.total_cmp(b));
    values[values.len() / 2]
}

fn main() {
    println!(
        "BENCH_CONTEXT|rust_target={}|os={}|available_parallelism={}|compiler=rustc_1.100.0|profile=release|bevy_pool=4|runen_capacity=4|measurement=warm_process",
        std::env::consts::ARCH,
        std::env::consts::OS,
        std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0)
    );

    let cases = [
        Case { workload: Workload::Empty, entities: 0, parallel: false },
        Case { workload: Workload::Empty, entities: 0, parallel: true },
        Case { workload: Workload::Reads, entities: 1_000, parallel: false },
        Case { workload: Workload::Reads, entities: 1_000, parallel: true },
        Case { workload: Workload::IndependentWrites, entities: 1_000, parallel: false },
        Case { workload: Workload::IndependentWrites, entities: 1_000, parallel: true },
        Case { workload: Workload::ConflictingWrites, entities: 1_000, parallel: false },
        Case { workload: Workload::ConflictingWrites, entities: 1_000, parallel: true },
        Case { workload: Workload::Reads, entities: 10_000, parallel: false },
        Case { workload: Workload::Reads, entities: 10_000, parallel: true },
        Case { workload: Workload::IndependentWrites, entities: 10_000, parallel: false },
        Case { workload: Workload::IndependentWrites, entities: 10_000, parallel: true },
        Case { workload: Workload::ConflictingWrites, entities: 10_000, parallel: false },
        Case { workload: Workload::ConflictingWrites, entities: 10_000, parallel: true },
    ];

    for case in cases {
        let runs_per_sample = if case.entities == 0 { 400usize }
            else if case.entities == 1_000 { 100usize } else { 30usize };
        let warmups = 6usize;
        let samples = 7usize;

        // Separate Worlds. Initialize compiled schedules outside the measurements.
        let mut fixtures = [
            Fixture::new(Engine::Bevy, case),
            Fixture::new(Engine::Runen, case),
        ];
        for fixture in &mut fixtures {
            for _ in 0..warmups { fixture.run(); }
        }

        let mut raw = [Vec::<f64>::new(), Vec::<f64>::new()];
        for repetition in 0..samples {
            // Interleave engine order to reduce systematic warm-cache/thermal bias.
            let order: [usize; 2] = if repetition % 2 == 0 { [0, 1] } else { [1, 0] };
            for engine_idx in order {
                let fixture = &mut fixtures[engine_idx];
                let start = Instant::now();
                for _ in 0..runs_per_sample {
                    fixture.run();
                }
                let elapsed = start.elapsed().as_nanos() as f64;
                raw[engine_idx].push(elapsed / runs_per_sample as f64);
            }
        }
        let observed_runs = (warmups + runs_per_sample * samples) as u64;
        for fixture in &mut fixtures {
            assert_eq!(fixture.sums(), expected_sums(case, observed_runs),
                "timing must not corrupt final ECS state");
        }

        let bevy = median(raw[0].clone());
        let runen = median(raw[1].clone());
        println!(
            "BENCH_RESULT|workload={}|entities={}|mode={}|samples={}|warmups={}|ticks_per_sample={}|bevy_median_ns={:.0}|runen_median_ns={:.0}|bevy_min_ns={:.0}|bevy_max_ns={:.0}|runen_min_ns={:.0}|runen_max_ns={:.0}|runen_over_bevy_ratio={:.3}",
            case.workload.label(),
            case.entities,
            if case.parallel { "parallel_4" } else { "serial" },
            samples, warmups, runs_per_sample, bevy, runen,
            raw[0].iter().copied().reduce(f64::min).unwrap(),
            raw[0].iter().copied().reduce(f64::max).unwrap(),
            raw[1].iter().copied().reduce(f64::min).unwrap(),
            raw[1].iter().copied().reduce(f64::max).unwrap(),
            runen / bevy,
        );
    }

    println!("BENCH_COMPLETE|samples_validated=true|selected_architecture=none");
}
