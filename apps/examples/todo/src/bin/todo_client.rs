use riesel::schema::parse_schema;
use riesel::storage::TcpStore;

use std::error::Error;
use std::io;
use std::net::SocketAddr;

use todo::Todo;

fn main() -> Result<(), Box<dyn Error>> {
    let mut schema = parse_schema(include_str!("../../app.json"))?;

    schema.validate()?;

    let task_model = schema
        .models
        .remove("task")
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing task model"))?;

    let address: SocketAddr = "127.0.0.1:47911".parse()?;

    let store = TcpStore::new(address);

    let todo = Todo::new(store, task_model);

    let created = todo.create_task("Build Riesel")?;

    assert!(!created.completed);

    let retrieved = todo.get_task(created.id)?.expect("created task must exist");

    assert_eq!(retrieved, created);

    let completed = todo.set_completed(created.id, true)?;

    assert!(completed.completed);
    assert_eq!(completed.revision, 2);

    let tasks = todo.list_tasks()?;

    assert!(
        tasks
            .iter()
            .any(|task| { task.id == created.id && task.completed })
    );

    println!("Todo + Storage component: passed");

    Ok(())
}
