mod task;

pub use task::{Task, TaskData, Todo, TodoError};

pub fn run_demo(ctx: &riesel::Context<'_>) -> Result<(), riesel::ConnectorError> {
    let todo = Todo::new(ctx.records::<TaskData>()?);

    let created = todo.create_task("Build Riesel")?;
    assert!(!created.completed);

    let retrieved = todo.get_task(created.id)?.expect("created task must exist");
    assert_eq!(retrieved, created);

    let completed = todo.set_completed(created.id, true)?;
    assert!(completed.completed);
    assert_eq!(completed.revision, 2);

    let tasks = todo.list_tasks()?;
    assert!(tasks.iter().any(|task| task.id == created.id && task.completed));

    println!("Todo + Riesel Rust SDK: passed");
    Ok(())
}
