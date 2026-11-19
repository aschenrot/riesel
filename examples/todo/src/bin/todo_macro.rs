//! Minimal development Todo application: a typed Records<T> requirement
//! and an explicitly opted-in in-memory Storage provider.
use riesel::{AppResult, Records, app};
use todo::TaskData;

#[app(name = "TodoMacro", development)]
async fn main(tasks: Records<TaskData>) -> AppResult<()> {
    todo::run_demo(tasks)?;
    Ok(())
}
