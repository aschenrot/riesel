//! Optional T1 demonstration: real Todo MemoryStore session, deliberately
//! publishing only an aggregate first-page status summary. No private task
//! titles/IDs and no mutations are available to the independent reader.
//!
//! cargo run --locked -p todo --features t1-public-read --bin todo_t1_read_host -- 45
//! Prints TODO_T1_SOCKET=/absolute/path; native T1 clients can attach.
//! Never use this trusted-shared-user profile on private Todo data.
#[cfg(unix)]
mod unix {
    use riesel::{
        Application, ReadExposure, UnixReadServer,
        storage::RecordStoreContract,
    };
    use std::{fs, io, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
    use todo::{
        TaskData, Todo,
        public_read::{TodoPublicSummaryFacade, TodoPublicSummaryReadAsyncRpcDispatcher},
    };

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let hold = match args.as_slice() {
            [] => 45,
            [seconds] => seconds.parse::<u64>()?,
            _ => return Err("usage: todo_t1_read_host [HOLD_SECONDS]".into()),
        };
        if !(5..=600).contains(&hold) {
            return Err("hold seconds must be 5..=600".into());
        }
        let dir = std::env::temp_dir().join(format!("riesel-todo-t1-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        let socket = dir.join("read.sock");
        Application::new("Todo T1 Public Read Example")
            .model::<TaskData>()?
            .rpc::<RecordStoreContract>("records")?
            .development()?
            .run(|context| {
                let records = context.records::<TaskData>()?;
                let todo = Todo::new(records.clone());
                // This standalone fixture owns only ephemeral, explicitly
                // non-sensitive synthetic demo data.
                todo.create_task("demo-one")?;
                let second = todo.create_task("demo-two")?;
                todo.set_completed(second.id, second.revision, true)?;
                let publication = Arc::new(context.publish_local_reads(
                    "example.todo_t1",
                    "gui.todo_summary",
                    ReadExposure::NonSensitiveSharedUser,
                    Arc::new(TodoPublicSummaryReadAsyncRpcDispatcher::new(
                        TodoPublicSummaryFacade::new(records),
                    )),
                    ["snapshot".to_owned()],
                )?);
                let worker = context.async_tasks();
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                rt.block_on(async {
                    worker.begin_scope()?;
                    let server = UnixReadServer::bind(&socket)?;
                    let active = Arc::clone(&publication);
                    worker.spawn("todo-local-t1", async move {
                        server.serve(active).await.map_err(|error| -> riesel::ConnectorError {
                            Box::new(error)
                        })
                    })?;
                    println!("TODO_T1_SOCKET={}", socket.display());
                    println!("T1 local summary only; shared OS user trusted; no task contents or commands.");
                    use io::Write;
                    io::stdout().flush()?;
                    tokio::time::sleep(Duration::from_secs(hold)).await;
                    publication.withdraw();
                    worker.finish_scope().await?;
                    Ok::<(), riesel::ConnectorError>(())
                })?;
                Ok(())
            })?;
        fs::remove_dir(&dir)?;
        Ok(())
    }
}

#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    unix::run()
}

#[cfg(not(unix))]
fn main() {
    eprintln!("T1 local Todo reader currently requires a Unix OS.");
    std::process::exit(2);
}
