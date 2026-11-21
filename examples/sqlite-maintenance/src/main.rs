//! Application-authored operator CLI using exactly one Rust schema source.
use riesel::RieselModel;
use riesel_storage_sqlite::{
    EvolutionLimits, ModelEvolution, SnapshotBudget, run_operator_maintenance_cli,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

#[derive(Serialize, Deserialize, RieselModel)]
#[riesel(model = "task")]
struct HistoricalTask {
    title: String,
}

#[derive(Serialize, Deserialize, RieselModel)]
#[riesel(model = "task")]
struct CurrentTask {
    title: String,
    priority: i64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = BTreeMap::from([(
        HistoricalTask::MODEL_NAME.into(),
        HistoricalTask::model_definition(),
    )]);
    let target = BTreeMap::from([(
        CurrentTask::MODEL_NAME.into(),
        CurrentTask::model_definition(),
    )]);
    let plan = ModelEvolution::new(source, target)?.transform::<HistoricalTask, CurrentTask, _>(
        |historical| {
            Ok(CurrentTask {
                title: historical.title,
                priority: 0,
            })
        },
    )?;

    // Demonstration-only bounds; adjust for the actual local volume, model
    // count and worst-case retained snapshot before executing real changes.
    let limits = EvolutionLimits {
        snapshot: SnapshotBudget {
            max_records: 10_000,
            max_json_bytes: 64 * 1024 * 1024,
            max_snapshot_bytes: 128 * 1024 * 1024,
            max_sidecar_bytes: 16 * 1024 * 1024,
            max_elapsed: Duration::from_secs(120),
        },
        max_elapsed: Duration::from_secs(120),
    };
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    run_operator_maintenance_cli(&args, &plan, limits, &mut input, &mut output)?;
    Ok(())
}
