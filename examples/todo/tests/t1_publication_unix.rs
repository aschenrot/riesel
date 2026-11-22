//! Real registered Todo MemoryStore -> bounded app-approved read summary ->
//! shared T1 Unix adapter -> independent OS client. No task data or mutations
//! are published, and the normal Todo HTTP/CLI remains unchanged.
#![cfg(all(unix, feature = "t1-public-read"))]

use riesel::{
    Application, ReadExposure, UnixReadClient, UnixReadServer,
    storage::RecordStoreContract,
};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
use todo::{
    TaskData, Todo,
    public_read::{
        TodoPublicReadError, TodoPublicSummary, TodoPublicSummaryFacade,
        TodoPublicSummaryReadAsyncRpcDispatcher,
    },
};

const APPLICATION: &str = "example.todo_t1";
const INTERFACE: &str = "gui.todo_summary";

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn decode(reply: Value) -> TodoPublicSummary {
    assert_eq!(reply["status"], "ok", "{reply}");
    let value: Result<TodoPublicSummary, TodoPublicReadError> =
        serde_json::from_value(reply["value"].clone()).unwrap();
    value.unwrap()
}

#[test]
fn independent_todo_summary_process() {
    let Ok(socket) = std::env::var("RIESEL_T1_TODO_SOCKET") else {
        return;
    };
    runtime().block_on(async {
        let reader = UnixReadClient::new(socket);
        let descriptor = reader.describe().await.unwrap();
        assert_eq!(descriptor.application, APPLICATION);
        assert_eq!(descriptor.interface, INTERFACE);
        assert_eq!(descriptor.contract.id, "riesel.todo.public_summary");
        assert_eq!(descriptor.contract.version, 1);
        assert_eq!(descriptor.allowed_queries, ["snapshot"]);
        let value = reader.query(&descriptor, "snapshot", json!({})).await.unwrap();
        let summary = decode(value.clone());
        assert_eq!(summary.inspected, 2);
        assert_eq!(summary.open_in_page, 1);
        assert_eq!(summary.completed_in_page, 1);
        assert!(!summary.more_available);
        assert!(!value.to_string().contains("SECRET_NOT_FOR_T1_READERS"));
        let forbidden = reader.query(&descriptor, "delete", json!({})).await.unwrap();
        assert_eq!(forbidden["status"], "error");
        assert_eq!(forbidden["code"], "not_published");
        println!("REAL_TODO_T1_SUMMARY_ONLY");
    });
}

#[test]
fn actual_todo_provider_owner_selected_summary_no_task_contents() {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let socket = dir.path().join("todo.sock");
    Application::new("Todo T1 Summary Test")
        .model::<TaskData>()
        .unwrap()
        .rpc::<RecordStoreContract>("records")
        .unwrap()
        .development()
        .unwrap()
        .run(|context| {
            let domain = Todo::new(context.records::<TaskData>()?);
            let private = domain.create_task("SECRET_NOT_FOR_T1_READERS")?;
            domain.set_completed(private.id, private.revision, true)?;
            domain.create_task("also private")?;
            let facade = TodoPublicSummaryFacade::new(context.records::<TaskData>()?);
            let publication = Arc::new(context.publish_local_reads(
                APPLICATION,
                INTERFACE,
                ReadExposure::NonSensitiveSharedUser,
                Arc::new(TodoPublicSummaryReadAsyncRpcDispatcher::new(facade)),
                ["snapshot".to_owned()],
            )?);
            let tasks = context.async_tasks();
            let executor = runtime();
            executor.block_on(async {
                tasks.begin_scope().unwrap();
                let server = UnixReadServer::bind(&socket).unwrap();
                let active = Arc::clone(&publication);
                tasks
                    .spawn("todo-t1-read", async move {
                        server.serve(active).await.map_err(|error| -> riesel::ConnectorError {
                            Box::new(error)
                        })
                    })
                    .unwrap();
                let client = UnixReadClient::new(&socket);
                let descriptor = client.describe().await.unwrap();
                let first = client.query(&descriptor, "snapshot", json!({})).await.unwrap();
                let first_summary = decode(first.clone());
                assert_eq!(first_summary.inspected, 2);
                assert_eq!(first_summary.open_in_page, 1);
                assert_eq!(first_summary.completed_in_page, 1);
                assert!(!first_summary.more_available);
                assert!(!first.to_string().contains("SECRET_NOT_FOR_T1_READERS"));

                // A *second process* observes exactly this app-owned summary.
                let child = std::env::current_exe().unwrap();
                let endpoint = socket.to_string_lossy().to_string();
                let (tx, rx) = tokio::sync::oneshot::channel();
                std::thread::spawn(move || {
                    let output = Command::new(child)
                        .args(["--exact", "independent_todo_summary_process", "--nocapture"])
                        .env("RIESEL_T1_TODO_SOCKET", endpoint)
                        .stdin(Stdio::null())
                        .output();
                    let _ = tx.send(output);
                });
                let output = tokio::time::timeout(Duration::from_secs(15), rx)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "independent Todo read failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    String::from_utf8_lossy(&output.stdout).contains("REAL_TODO_T1_SUMMARY_ONLY")
                );

                // Domain owner changes actual stored state without any
                // public write method. The client sees a fresh read result.
                let latest = domain.get_task(private.id).unwrap().unwrap();
                domain.set_completed(latest.id, latest.revision, false).unwrap();
                let changed = decode(client.query(&descriptor, "snapshot", json!({})).await.unwrap());
                assert_eq!(changed.open_in_page, 2);
                assert_eq!(changed.completed_in_page, 0);

                // The read is one bounded page, not an unbounded whole-domain
                // scan or a misleading global count.
                for index in 0..34 {
                    domain.create_task(format!("private-{index}")).unwrap();
                }
                let page = decode(client.query(&descriptor, "snapshot", json!({})).await.unwrap());
                assert_eq!(page.inspected, 32);
                assert_eq!(page.open_in_page + page.completed_in_page, 32);
                assert!(page.more_available);
                let mut wrong = descriptor.clone();
                wrong.application.push_str(".other");
                let denied = client.query(&wrong, "snapshot", json!({})).await.unwrap();
                assert_eq!(denied["code"], "wrong_interface");
                let denied = client.query(&descriptor, "create", json!({"title":"new"})).await.unwrap();
                assert_eq!(denied["code"], "not_published");
                publication.withdraw();
                let denied = client.query(&descriptor, "snapshot", json!({})).await.unwrap();
                assert_eq!(denied["code"], "withdrawn");
                tasks.finish_scope().await.unwrap();
                assert!(!socket.exists(), "server must withdraw owned Unix socket");
            });
            assert_eq!(domain.list_tasks()?.len(), 36);
            Ok(())
        })
        .unwrap();
}
