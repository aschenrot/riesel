use riesel::schema::ModelDefinition;
use riesel::storage::RecordStore;
use riesel::{App, ConnectorRegistry, DeploymentConfig, TcpRecordStoreConnector};

use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use todo::Todo;

fn main() -> Result<(), Box<dyn Error>> {
    let app = App::from_schema(include_str!("../../app.json"))?;

    let mut args = std::env::args_os().skip(1);
    let deployment_path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/deployment.json"))
    });
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: todo_client [deployment.json]",
        )
        .into());
    }

    let deployment = DeploymentConfig::load(&deployment_path)?;
    let mut connectors = ConnectorRegistry::new();
    connectors.register(TcpRecordStoreConnector)?;

    // Riesel owns discovery, compatibility checks, resolution and binding.
    app.run(&deployment, &connectors, run_todo)?;
    Ok(())
}

fn run_todo(running: &riesel::RunningApp) -> Result<(), riesel::ConnectorError> {

    let task_model: ModelDefinition = running
        .app()
        .definition()
        .and_then(|schema| schema.models.get("task"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing task model"))?
        .clone();
    let store = running.service::<Arc<dyn RecordStore + Send + Sync>>("storage")?.clone();
    let todo = Todo::new(store, task_model);

    let created = todo.create_task("Build Riesel")?;
    assert!(!created.completed);

    let retrieved = todo.get_task(created.id)?.expect("created task must exist");
    assert_eq!(retrieved, created);

    let completed = todo.set_completed(created.id, true)?;
    assert!(completed.completed);
    assert_eq!(completed.revision, 2);

    let tasks = todo.list_tasks()?;
    assert!(tasks.iter().any(|task| task.id == created.id && task.completed));

    println!("Todo + Riesel runtime: passed");
    Ok(())
}
