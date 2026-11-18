use riesel::schema::ModelDefinition;
use riesel::storage::{RecordStore, TcpStore};
use riesel::{
    App, CapabilityBindings, DeploymentConfig, ProviderRegistry, ServiceEndpoint,
};

use std::collections::BTreeMap;
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use todo::Todo;

fn main() -> Result<(), Box<dyn Error>> {
    let app = App::from_schema(include_str!("../../app.json"))?;
    let schema = app.definition().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing application definition")
    })?;

    let mut args = std::env::args_os().skip(1);
    let deployment_path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/deployment.json"))
    });
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: todo_client [deployment.json]",
        ).into());
    }

    let deployment = DeploymentConfig::load(&deployment_path)?;
    deployment.validate(schema)?;

    let endpoint = deployment.services.get("storage").ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidData, "missing storage endpoint")
    })?;
    let ServiceEndpoint::Tcp { address } = endpoint;
    let store = TcpStore::new(*address);

    // Discover the running provider, then let Riesel resolve the
    // application's versioned requirement against its advertised metadata.
    let advertised = store.describe()?;
    let mut registry = ProviderRegistry::new();
    registry.register(advertised.provider.clone())?;
    let resolved = registry.resolve(schema, &BTreeMap::new())?;
    let selected = resolved
        .iter()
        .find(|item| item.requirement == "storage")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unresolved storage"))?;

    // Verify the full Rust-derived contract before exposing the TCP adapter.
    TcpStore::verify_service_info(
        &advertised,
        &selected.provider,
        &selected.capability,
    )?;

    let mut bindings = CapabilityBindings::<dyn RecordStore>::new();
    bindings.register(selected.provider.clone(), Arc::new(store))?;
    let store = bindings.get(selected)?;

    let task_model: ModelDefinition = schema
        .models
        .get("task")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing task model"))?
        .clone();
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

    println!("Todo + resolved remote Storage: passed");
    Ok(())
}
