mod task;
#[cfg(feature = "t1-public-read")]
pub mod public_read;

pub use task::{Task, TaskData, Todo, TodoError};

pub fn run_demo(records: riesel::Records<TaskData>) -> Result<(), TodoError> {
    let todo = Todo::new(records);

    let created = todo.create_task("Build Riesel")?;
    assert!(!created.value.completed);

    let retrieved = todo.get_task(created.id)?.expect("created task must exist");
    assert_eq!(retrieved, created);

    let completed = todo.set_completed(created.id, created.revision, true)?;
    assert!(completed.value.completed);
    assert_eq!(completed.revision, 2);

    let tasks = todo.list_tasks()?;
    assert!(
        tasks
            .iter()
            .any(|task| task.id == created.id && task.value.completed)
    );

    println!("Todo + Riesel Rust SDK: passed");
    Ok(())
}

#[cfg(feature = "http")]
pub mod http;
