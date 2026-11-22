//! A genuine headless Riesel app publishing only a deliberately selected,
//! non-sensitive execution-graph read interface over optional Unix T1 IPC.
//!
//! cargo run --locked -p riesel --features app-publication-unix --example graph_read_host -- 20
//! The app prints GRAPH_SOCKET=<absolute path>. A separately run browser
//! viewer may attach; no renderer, browser or Manager is required by the app.
#[cfg(unix)]
mod unix {
    use riesel::{
        App, AsyncOperation, ConnectorError, ConnectorRegistry,
        ExecutionGraphReadAsyncRpcDispatcher, ExecutionGraphReadFacade, ExecutionPlanDraft,
        OperationKey, OperationNode, OperationOutcome, OperationRegistry, ReadExposure,
        UnixReadServer, async_operation,
    };
    use std::{fs, io, num::NonZeroUsize, os::unix::fs::PermissionsExt, sync::Arc, time::Duration};

    const SCHEMA: &str =
        r#"{"version":1,"app":{"name":"GraphReadHost"},"models":{},"requires":[]}"#;

    pub fn run() -> Result<(), Box<dyn std::error::Error>> {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let hold = match args.as_slice() {
            [] => 20,
            [seconds] => {
                let n = seconds.parse::<u64>()?;
                if !(5..=600).contains(&n) {
                    return Err("hold seconds must be in 5..=600".into());
                }
                n
            }
            _ => return Err("usage: graph_read_host [HOLD_SECONDS]".into()),
        };
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async move {
            let path =
                std::env::temp_dir().join(format!("riesel-graph-read-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path)?;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
            let socket = path.join("read.sock");
            let mut app = App::from_schema(SCHEMA)?.start_local(&ConnectorRegistry::new())?;
            let mut operations = OperationRegistry::<AsyncOperation>::new();
            for name in ["left", "right", "join"] {
                operations.register(
                    OperationKey::new(name, 1)?,
                    async_operation(move || async move {
                        if name != "join" {
                            tokio::time::sleep(Duration::from_secs(3)).await;
                        }
                        OperationOutcome::Completed
                    }),
                )?;
            }
            let plan = ExecutionPlanDraft::new("example.graph_inspector", 1)
                .operation(OperationNode::new("left", OperationKey::new("left", 1)?))
                .operation(OperationNode::new("right", OperationKey::new("right", 1)?))
                .operation(
                    OperationNode::new("join", OperationKey::new("join", 1)?)
                        .after("left")
                        .after("right"),
                )
                .validate()?;
            let runner =
                app.execution_graph(plan.bind(&operations)?, NonZeroUsize::new(2).unwrap())?;
            let facade = ExecutionGraphReadFacade::new(&plan, runner.observer())?;
            let reader = Arc::new(app.publish_local_reads(
                "example.graph_inspector",
                "gui.execution_graph",
                ReadExposure::NonSensitiveSharedUser,
                Arc::new(ExecutionGraphReadAsyncRpcDispatcher::new(facade)),
                ["snapshot".to_owned(), "changes".to_owned()],
            )?);
            let server = UnixReadServer::bind(&socket)?;
            let tasks = app.async_tasks();
            tasks.begin_scope()?;
            let serving = Arc::clone(&reader);
            tasks.spawn("local-graph-reads", async move {
                server
                    .serve(serving)
                    .await
                    .map_err(|error| -> ConnectorError { Box::new(error) })
            })?;
            println!("GRAPH_SOCKET={}", socket.display());
            println!("T1 non-sensitive local read, same-OS-user trusted. No external commands.");
            use std::io::Write;
            io::stdout().flush()?;
            tokio::time::sleep(Duration::from_secs(2)).await;
            let report = runner.execute().await;
            if !report.succeeded() {
                return Err(io::Error::other("the sample graph did not complete").into());
            }
            tokio::time::sleep(Duration::from_secs(hold)).await;
            reader.withdraw();
            tasks.finish_scope().await?;
            app.shutdown()?;
            fs::remove_dir(&path)?;
            Ok::<(), Box<dyn std::error::Error>>(())
        })
    }
}

#[cfg(unix)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    unix::run()
}

#[cfg(not(unix))]
fn main() {
    eprintln!("The first T1 graph-read host requires a Unix local adapter.");
    std::process::exit(2);
}
