//! Real SDK-selected Counter provider -> separate app-owned query facet ->
//! existing Unix T1 admission -> independent second process. The original
//! versioned Counter add command is deliberately not published.
#![cfg(all(unix, feature = "t1-public-read"))]

use riesel::{Application, ReadExposure, UnixReadClient, UnixReadServer};
use riesel_counter::{Counter, CounterContract};
use riesel_counter_component::memory_counter_provider;
use riesel_counter_example::public_read::{
    CounterT1ReadAsyncRpcDispatcher, CounterT1ReadFacade,
};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};

const APPLICATION: &str = "example.counter_t1";
const INTERFACE: &str = "gui.counter_value";

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn independent_resolved_counter_reader() {
    let Ok(path) = std::env::var("RIESEL_T1_COUNTER_SOCKET") else {
        return;
    };
    runtime().block_on(async {
        let client = UnixReadClient::new(path);
        let descriptor = client.describe().await.unwrap();
        assert_eq!(descriptor.application, APPLICATION);
        assert_eq!(descriptor.interface, INTERFACE);
        assert_eq!(descriptor.contract.id, "riesel.counter.public_value");
        assert_eq!(descriptor.contract.version, 1);
        assert_eq!(descriptor.allowed_queries, ["snapshot"]);
        let read = client.query(&descriptor, "snapshot", json!({})).await.unwrap();
        assert_eq!(read["status"], "ok");
        assert_eq!(read["value"]["Ok"], 5);
        // A program cannot bypass T1 simply by requesting the underlying
        // CounterContract add command over the public read socket.
        let denied = client.query(&descriptor, "add", json!({"amount":100})).await.unwrap();
        assert_eq!(denied["status"], "error");
        assert_eq!(denied["code"], "not_published");
        println!("RESOLVED_COUNTER_PUBLIC_QUERY_ONLY");
    });
}

#[test]
fn selected_counter_provider_is_only_authoritative_mutation_owner() {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let socket = dir.path().join("counter.sock");
    Application::new("Counter T1 Summary")
        .rpc::<CounterContract>("counter")
        .unwrap()
        .provider(memory_counter_provider().unwrap())
        .unwrap()
        .run(|context| {
            let counter = context.capability::<CounterContract>("counter")?.clone();
            assert_eq!(counter.add(5)?, 5);
            let facade = CounterT1ReadFacade::new(counter.clone());
            let publication = Arc::new(context.publish_local_reads(
                APPLICATION,
                INTERFACE,
                ReadExposure::NonSensitiveSharedUser,
                Arc::new(CounterT1ReadAsyncRpcDispatcher::new(facade)),
                ["snapshot".to_owned()],
            )?);
            let tasks = context.async_tasks();
            runtime().block_on(async {
                tasks.begin_scope().unwrap();
                let server = UnixReadServer::bind(&socket).unwrap();
                let active = Arc::clone(&publication);
                tasks
                    .spawn("counter-t1-reader", async move {
                        server.serve(active).await.map_err(|error| -> riesel::ConnectorError {
                            Box::new(error)
                        })
                    })
                    .unwrap();

                let client = UnixReadClient::new(&socket);
                let descriptor = client.describe().await.unwrap();
                assert_eq!(
                    client.query(&descriptor, "snapshot", json!({})).await.unwrap()["value"]["Ok"],
                    5
                );

                let executable = std::env::current_exe().unwrap();
                let pathname = socket.to_string_lossy().to_string();
                let (tx, rx) = tokio::sync::oneshot::channel();
                std::thread::spawn(move || {
                    let result = Command::new(executable)
                        .args(["--exact", "independent_resolved_counter_reader", "--nocapture"])
                        .env("RIESEL_T1_COUNTER_SOCKET", pathname)
                        .stdin(Stdio::null())
                        .output();
                    let _ = tx.send(result);
                });
                let output = tokio::time::timeout(Duration::from_secs(15), rx)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "second Counter client: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
                assert!(
                    String::from_utf8_lossy(&output.stdout)
                        .contains("RESOLVED_COUNTER_PUBLIC_QUERY_ONLY")
                );

                // Only the app-owned actual Counter provider may mutate.
                assert_eq!(counter.add(4).unwrap(), 9);
                let fresh = client.query(&descriptor, "snapshot", json!({})).await.unwrap();
                assert_eq!(fresh["value"]["Ok"], 9);
                let bad = client.query(&descriptor, "label", json!({"prefix":"x"})).await.unwrap();
                assert_eq!(bad["code"], "not_published");
                let mut wrong_contract = descriptor.clone();
                wrong_contract.contract.version += 1;
                let bad = client.query(&wrong_contract, "snapshot", json!({})).await.unwrap();
                assert_eq!(bad["code"], "wrong_contract");
                publication.withdraw();
                let bad = client.query(&descriptor, "snapshot", json!({})).await.unwrap();
                assert_eq!(bad["code"], "withdrawn");
                tasks.finish_scope().await.unwrap();
                assert!(!socket.exists());
            });
            assert_eq!(counter.value()?, 9);
            Ok(())
        })
        .unwrap();
}
