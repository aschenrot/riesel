//! Actual Todo domain under the shared P3/T1 publication boundary.
//! A separately spawned OS-process client sees only intentionally public
//! aggregate counts, never the app's task titles or mutation capability.
#![cfg(all(unix, feature = "t1-public-summary"))]

use riesel::{
    Application, AppReadPublication, ConnectorError, ReadExposure, ReadPublicationDescriptor,
    Records, UnixReadClient, UnixReadServer,
};
use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use todo::public_read::{PublicTodoReadAsyncRpcDispatcher, PublicTodoReadFacade};
use todo::{TaskData, Todo};

const APP: &str = "example.todo_public_demo";
const VIEW: &str = "gui.todo_summary";
const PUBLIC_TASK_A: &str = "PUBLIC TEST TASK A";
const PUBLIC_TASK_B: &str = "PUBLIC TEST TASK B";

fn executor() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn second_os_process_reads_only_explicit_public_todo_summary() {
    let Ok(path) = std::env::var("RIESEL_TODO_T1_TEST_SOCKET") else {
        return;
    };
    executor().block_on(async {
        let client = UnixReadClient::new(path);
        let descriptor = client.describe().await.unwrap();
        assert_eq!(descriptor.application, APP);
        assert_eq!(descriptor.interface, VIEW);
        assert_eq!(descriptor.contract.id, "example.todo.public_summary");
        assert_eq!(descriptor.contract.version, 1);
        assert_eq!(descriptor.allowed_queries, ["summary"]);
        let response = client.query(&descriptor, "summary", json!({})).await.unwrap();
        assert_eq!(response["status"], "ok");
        assert_eq!(response["value"]["Ok"]["total"], 2);
        assert_eq!(response["value"]["Ok"]["completed"], 1);
        assert_eq!(response["value"]["Ok"]["pending"], 1);
        let raw = serde_json::to_string(&response).unwrap();
        assert!(!raw.contains(PUBLIC_TASK_A));
        assert!(!raw.contains(PUBLIC_TASK_B));
        assert!(!raw.contains("title"));
        let forbidden = client.query(&descriptor, "delete_task", json!({})).await.unwrap();
        assert_eq!(forbidden["status"], "error");
        assert_eq!(forbidden["code"], "not_published");
        println!("TODO_T1_REAL_CHILD_READ_WITHOUT_PRIVATE_TITLES_OR_MUTATIONS");
    });
}

async fn spawn_external_client(path: PathBuf) {
    let bin = std::env::current_exe().unwrap();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let output = Command::new(bin)
            .arg("--exact")
            .arg("second_os_process_reads_only_explicit_public_todo_summary")
            .arg("--nocapture")
            .env("RIESEL_TODO_T1_TEST_SOCKET", path)
            .stdin(Stdio::null())
            .output();
        let _ = sender.send(output);
    });
    let output = tokio::time::timeout(Duration::from_secs(12), receiver)
        .await
        .expect("external client completion")
        .expect("child launcher")
        .expect("child process");
    assert!(
        output.status.success(),
        "child failure: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains("TODO_T1_REAL_CHILD_READ_WITHOUT_PRIVATE_TITLES_OR_MUTATIONS")
    );
}

#[test]
fn actual_todo_sdk_records_have_one_selected_read_view_across_sessions() {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let socket = temp.path().join("todo-read.sock");
    let prior = Arc::new(Mutex::new(None::<ReadPublicationDescriptor>));
    let pid = std::process::id();
    for cycle in 0..2 {
        let previous = Arc::clone(&prior);
        let socket = socket.clone();
        Application::new("TodoPublicDemo")
            .records::<TaskData>()
            .unwrap()
            .development()
            .unwrap()
            .run(move |context| {
                let records = context.records::<TaskData>()?;
                let todo = Todo::new(records.clone());
                let created = todo.create_task(PUBLIC_TASK_A)?;
                let task = todo.create_task(PUBLIC_TASK_B)?;
                todo.set_completed(task.id, task.revision, true)?;
                assert_eq!(created.revision, 1);

                let publication: Arc<AppReadPublication> = Arc::new(
                    context.publish_local_reads(
                        APP,
                        VIEW,
                        ReadExposure::NonSensitiveSharedUser,
                        Arc::new(PublicTodoReadAsyncRpcDispatcher::new(
                            PublicTodoReadFacade::new(records),
                        )),
                        ["summary".to_owned()],
                    )?,
                );
                let server = UnixReadServer::bind(&socket)?;
                let tasks = context.async_tasks();
                let active = Arc::clone(&publication);
                let rt = executor();
                rt.block_on(async {
                    tasks.begin_scope()?;
                    tasks.spawn("todo-public-summary", async move {
                        server.serve(active).await.map_err(|error| -> ConnectorError {
                            Box::new(error)
                        })
                    })?;
                    let client = UnixReadClient::new(&socket);
                    let current = client.describe().await?;
                    assert_eq!(current.allowed_queries, ["summary"]);
                    assert_eq!(current.exposure, ReadExposure::NonSensitiveSharedUser);
                    let first = client.query(&current, "summary", json!({})).await?;
                    assert_eq!(first["status"], "ok");
                    assert_eq!(first["value"]["Ok"]["total"], 2);
                    assert_eq!(first["value"]["Ok"]["completed"], 1);
                    if cycle == 0 {
                        spawn_external_client(socket.clone()).await;
                    } else {
                        let stale = previous.lock().unwrap().clone().unwrap();
                        assert_ne!(stale.generation, current.generation);
                        assert_ne!(stale.publication_id, current.publication_id);
                        let refused = client.query(&stale, "summary", json!({})).await?;
                        assert_eq!(refused["status"], "error");
                        assert_eq!(refused["code"], "stale_generation");
                    }
                    *previous.lock().unwrap() = Some(current.clone());
                    publication.withdraw();
                    let refused = client.query(&current, "summary", json!({})).await?;
                    assert_eq!(refused["code"], "withdrawn");
                    tasks.finish_scope().await?;
                    Ok::<(), ConnectorError>(())
                })?;
                assert!(!socket.exists(), "owned reader removed its socket at shutdown");
                Ok(())
            })
            .unwrap();
        assert_eq!(std::process::id(), pid);
    }
}
