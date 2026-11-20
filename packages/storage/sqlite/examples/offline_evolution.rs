//! Application-authored operator entrypoint example. This sample only knows
//! its two explicit Rust types; do NOT run it on unrelated SQLite files.
//!
//! cargo run -p riesel_storage_sqlite --example offline_evolution -- preview DB BACKUP
//! cargo run -p riesel_storage_sqlite --example offline_evolution -- execute DB BACKUP
//! cargo run -p riesel_storage_sqlite --example offline_evolution -- restore DB BACKUP RETAIN
use riesel_schema::{ModelDefinition, RieselModel, TypeRef};
use riesel_storage_sqlite::{
    EvolutionLimits, ModelEvolution, SnapshotBudget, run_operator_maintenance_cli,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Duration};

#[derive(Serialize, Deserialize)]
struct HistoricalTask {
    title: String,
}
impl RieselModel for HistoricalTask {
    const MODEL_NAME: &'static str = "task";
    fn model_definition() -> ModelDefinition {
        ModelDefinition {
            fields: BTreeMap::from([("title".into(), TypeRef::String)]),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct CurrentTask {
    title: String,
    priority: i64,
}
impl RieselModel for CurrentTask {
    const MODEL_NAME: &'static str = "task";
    fn model_definition() -> ModelDefinition {
        ModelDefinition {
            fields: BTreeMap::from([
                ("title".into(), TypeRef::String),
                ("priority".into(), TypeRef::I64),
            ]),
        }
    }
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

    // Example bounds only: the operator must choose sizes for the actual
    // database and recovery filesystem before using a real migration plan.
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
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    run_operator_maintenance_cli(&args, &plan, limits, &mut input, &mut output)?;
    Ok(())
}
