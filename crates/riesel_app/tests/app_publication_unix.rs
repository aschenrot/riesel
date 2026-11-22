//! Real local T1 client lifecycle test. The native reader is an independent
//! operating-system process; the authoritative application never becomes GUI
//! owned, and no mutation is exported.
#![cfg(all(unix, feature = "app-publication-unix"))]

use riesel_app::{
    App, AppReadPublication, ConnectorError, ConnectorRegistry, ReadExposure, UnixReadClient,
    UnixReadServer,
};
use riesel_protocol::{AsyncRpcFuture, AsyncRpcHandler, RpcError};
use riesel_schema::{
    CapabilityContract, CapabilityRef, OperationDefinition, OperationKind, TypeRef,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const SCHEMA: &str =
    r#"{"version":1,"app":{"name":"IndependentReadOwner"},"models":{},"requires":[]}"#;
const APPLICATION: &str = "research.t1_read_owner";
const INTERFACE: &str = "read.public_counter";

struct Counter {
    reads: AtomicUsize,
    writes: AtomicUsize,
}

impl Counter {
    fn new() -> Self {
        Self {
            reads: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
        }
    }
}

impl AsyncRpcHandler for Counter {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        Ok(CapabilityContract {
            capability: CapabilityRef {
                id: "research.t1_counter".into(),
                version: 1,
            },
            operations: BTreeMap::from([
                (
                    "count".to_owned(),
                    OperationDefinition {
                        kind: OperationKind::Query,
                        description: "Read app-owned state".into(),
                        inputs: vec![],
                        output: TypeRef::U64,
                        error: TypeRef::Unit,
                    },
                ),
                (
                    "update".to_owned(),
                    OperationDefinition {
                        kind: OperationKind::Command,
                        description: "Mutate app-owned state".into(),
                        inputs: vec![],
                        output: TypeRef::U64,
                        error: TypeRef::Unit,
                    },
                ),
            ]),
            types: BTreeMap::new(),
        })
    }

    fn dispatch<'a>(&'a self, operation: &'a str, _arguments: Value) -> AsyncRpcFuture<'a> {
        Box::pin(async move {
            match operation {
                "count" => {
                    self.reads.fetch_add(1, Ordering::SeqCst);
                    Ok(json!(7))
                }
                "update" => {
                    self.writes.fetch_add(1, Ordering::SeqCst);
                    Ok(json!(8))
                }
                _ => Err(RpcError::UnknownOperation(operation.into())),
            }
        })
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn socket_directory() -> PathBuf {
    std::env::temp_dir().join(format!("riesel-t1-read-{}", uuid::Uuid::new_v4()))
}

#[test]
fn independent_same_user_gui_reader() {
    let Ok(path) = std::env::var("RIESEL_T1_READ_TEST_ENDPOINT") else {
        return;
    };
    runtime().block_on(async {
        let client = UnixReadClient::new(path);
        let described = client.describe().await.unwrap();
        assert_eq!(described.application, APPLICATION);
        assert_eq!(described.interface, INTERFACE);
        assert_eq!(described.contract.version, 1);
        assert_eq!(described.allowed_queries, ["count"]);
        let read = client.query(&described, "count", json!({})).await.unwrap();
        assert_eq!(read["status"], "ok");
        assert_eq!(read["value"], 7);
        let denied = client.query(&described, "update", json!({})).await.unwrap();
        assert_eq!(denied["status"], "error");
        assert_eq!(denied["code"], "not_published");
        println!("INDEPENDENT_SAME_USER_CLIENT_READ_ALLOWED_WRITE_DENIED");
    });
}

#[test]
fn real_owned_unix_client_attach_detach_and_recover() {
    runtime().block_on(async {
        let dir = socket_directory();
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("read.sock");
        let domain = Arc::new(Counter::new());

        let mut first = App::from_schema(SCHEMA)
            .unwrap()
            .start_local(&ConnectorRegistry::new())
            .unwrap();
        let tasks = first.async_tasks();
        tasks.begin_scope().unwrap();
        let publication: Arc<AppReadPublication> = Arc::new(
            first
                .publish_local_reads(
                    APPLICATION,
                    INTERFACE,
                    ReadExposure::NonSensitiveSharedUser,
                    domain.clone(),
                    ["count".into()],
                )
                .unwrap(),
        );
        let server = UnixReadServer::bind(&path).unwrap();
        assert_eq!(
            UnixReadServer::bind(&path).err().unwrap().kind(),
            std::io::ErrorKind::AlreadyExists,
            "do not unlink or steal an already-bound socket"
        );
        let active = Arc::clone(&publication);
        tasks
            .spawn("first-t1-read-host", async move {
                server
                    .serve(active)
                    .await
                    .map_err(|error| -> ConnectorError { Box::new(error) })
            })
            .unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let client = UnixReadClient::new(&path);
        let old_descriptor = client.describe().await.unwrap();
        assert_eq!(old_descriptor.allowed_queries, ["count"]);
        let first_read = client
            .query(&old_descriptor, "count", json!({}))
            .await
            .unwrap();
        assert_eq!(first_read["status"], "ok");
        assert_eq!(first_read["value"], 7);

        // The wire boundary rejects unsupported protocol versions before
        // interpreting any app descriptor or invoking the domain.
        let mut raw = tokio::net::UnixStream::connect(&path).await.unwrap();
        raw.write_all(br#"{"wire_version":99,"action":"describe"}"#)
            .await
            .unwrap();
        raw.shutdown().await.unwrap();
        let mut wrong_version_reply = Vec::new();
        raw.read_to_end(&mut wrong_version_reply).await.unwrap();
        let wrong_reply: Value = serde_json::from_slice(&wrong_version_reply).unwrap();
        assert_eq!(wrong_reply["status"], "error");
        assert_eq!(wrong_reply["code"], "unsupported_wire_version");

        // The client rejects oversize requests locally and the server may
        // never invoke the domain on a raw disallowed command.
        assert_eq!(
            client
                .query(&old_descriptor, "count", Value::String("x".repeat(200_000)),)
                .await
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );

        // Multiple client connections use one application publication and
        // one runtime admission owner, without requiring a GUI renderer.
        let mut concurrent = Vec::new();
        for _ in 0..8 {
            let path = path.clone();
            let descriptor = old_descriptor.clone();
            concurrent.push(tokio::spawn(async move {
                UnixReadClient::new(path)
                    .query(&descriptor, "count", json!({}))
                    .await
            }));
        }
        for reader in concurrent {
            let answer = reader.await.unwrap().unwrap();
            assert_eq!(answer["status"], "ok");
            assert_eq!(answer["value"], 7);
        }

        // A second actual OS process acts as a GUI, without acquiring an
        // individual protected principal. This is T1's explicit same-user
        // trust assumption, not a security assertion.
        let child_path = path.to_string_lossy().to_string();
        let child = std::env::current_exe().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let out = Command::new(child)
                .arg("--exact")
                .arg("independent_same_user_gui_reader")
                .arg("--nocapture")
                .env("RIESEL_T1_READ_TEST_ENDPOINT", child_path)
                .stdin(Stdio::null())
                .output();
            let _ = tx.send(out);
        });
        let output = tokio::time::timeout(Duration::from_secs(12), rx)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            output.status.success(),
            "independent reader: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains("INDEPENDENT_SAME_USER_CLIENT_READ_ALLOWED_WRITE_DENIED")
        );
        assert_eq!(domain.writes.load(Ordering::SeqCst), 0);

        // Neither client closing nor being re-created owns app shutdown.
        assert!(first.session_token().is_active());
        assert_eq!(
            client
                .query(&old_descriptor, "count", json!({}))
                .await
                .unwrap()["status"],
            "ok"
        );
        publication.withdraw();
        let withdrawn = client
            .query(&old_descriptor, "count", json!({}))
            .await
            .unwrap();
        assert_eq!(withdrawn["status"], "error");
        assert_eq!(withdrawn["code"], "withdrawn");
        tasks.finish_scope().await.unwrap();
        assert!(!path.exists(), "owned listener removed its exact socket");
        first.shutdown().unwrap();

        // A real replacement Riesel session publishes through the same path
        // and OS PID. A stale descriptor is rejected before domain dispatch.
        let mut second = App::from_schema(SCHEMA)
            .unwrap()
            .start_local(&ConnectorRegistry::new())
            .unwrap();
        let next_tasks = second.async_tasks();
        next_tasks.begin_scope().unwrap();
        let successor = Arc::new(
            second
                .publish_local_reads(
                    APPLICATION,
                    INTERFACE,
                    ReadExposure::NonSensitiveSharedUser,
                    domain.clone(),
                    ["count".into()],
                )
                .unwrap(),
        );
        let successor_server = UnixReadServer::bind(&path).unwrap();
        let successor_worker = Arc::clone(&successor);
        next_tasks
            .spawn("second-t1-read-host", async move {
                successor_server
                    .serve(successor_worker)
                    .await
                    .map_err(|error| -> ConnectorError { Box::new(error) })
            })
            .unwrap();
        let newer = client.describe().await.unwrap();
        assert_ne!(old_descriptor.generation, newer.generation);
        assert_ne!(old_descriptor.publication_id, newer.publication_id);
        let stale = client
            .query(&old_descriptor, "count", json!({}))
            .await
            .unwrap();
        assert_eq!(stale["status"], "error");
        assert_eq!(stale["code"], "stale_generation");
        let current = client.query(&newer, "count", json!({})).await.unwrap();
        assert_eq!(current["status"], "ok");
        assert_eq!(current["value"], 7);

        // No client can send a command through this T1 policy, even via the
        // raw independent-process wire format.
        assert_eq!(
            client.query(&newer, "update", json!({})).await.unwrap()["code"],
            "not_published"
        );
        assert_eq!(domain.writes.load(Ordering::SeqCst), 0);

        next_tasks.finish_scope().await.unwrap();
        second.shutdown().unwrap();
        assert!(!path.exists());
        fs::remove_dir(&dir).unwrap();
        println!("REAL_T1_UNIX_SDK_CLIENT_LIFECYCLE_OK");
    });
}

#[test]
fn counterfeit_read_endpoint_process() {
    // A *different OS process* with the same UID can recreate an endpoint
    // and invent the advertised non-secret publication fields. This is
    // intentionally an expected T1 limitation, NOT a protected-identity test.
    let Ok(path) = std::env::var("RIESEL_T1_FORGED_SOCKET") else {
        return;
    };
    runtime().block_on(async {
        let listener = tokio::net::UnixListener::bind(path).unwrap();
        for _ in 0..2 {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(8), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let mut request = Vec::new();
            stream.read_to_end(&mut request).await.unwrap();
            let message: Value = serde_json::from_slice(&request).unwrap();
            let answer = match message["action"].as_str() {
                Some("describe") => json!({
                    "wire_version": 1,
                    "status": "descriptor",
                    "descriptor": {
                        "application": "forged.app",
                        "interface": "gui.counter_value",
                        "publication_id": "not-an-authentication-credential",
                        "contract": {
                            "id": "riesel.counter.public_value",
                            "version": 1
                        },
                        "generation": "fabricated-session-generation",
                        "allowed_queries": ["snapshot"],
                        "exposure": "non_sensitive_shared_user"
                    }
                }),
                Some("query") => json!({
                    "wire_version": 1,
                    "status": "ok",
                    "value": {"Ok": 999}
                }),
                other => panic!("unexpected fake-endpoint request: {other:?}"),
            };
            stream
                .write_all(&serde_json::to_vec(&answer).unwrap())
                .await
                .unwrap();
            stream.shutdown().await.unwrap();
        }
        println!("SAME_USER_ENDPOINT_IMPERSONATION_REPRODUCED");
    });
}

#[test]
fn same_uid_endpoint_rebinding_is_not_protected_server_authentication() {
    runtime().block_on(async {
        let dir = socket_directory();
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let path = dir.join("read.sock");

        // A legitimate application-owned endpoint was here, but is now gone.
        // UnixReadServer correctly removes its owned socket on drop; the
        // pathname, OS UID and known contract name do not authenticate a
        // later process taking over the same path.
        let original = UnixReadServer::bind(&path).unwrap();
        assert!(path.exists());
        drop(original);
        assert!(!path.exists());

        let child = std::env::current_exe().unwrap();
        let endpoint = path.to_string_lossy().into_owned();
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let result = Command::new(child)
                .arg("--exact")
                .arg("counterfeit_read_endpoint_process")
                .arg("--nocapture")
                .env("RIESEL_T1_FORGED_SOCKET", endpoint)
                .stdin(Stdio::null())
                .output();
            let _ = tx.send(result);
        });
        tokio::time::timeout(Duration::from_secs(8), async {
            while !path.exists() {
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        })
        .await
        .unwrap();

        let client = UnixReadClient::new(&path);
        let forged = client.describe().await.unwrap();
        assert_eq!(forged.application, "forged.app");
        assert_eq!(forged.generation, "fabricated-session-generation");
        assert_eq!(
            forged.contract.id, "riesel.counter.public_value",
            "matching an advertised contract is not server authentication"
        );
        let fake_read = client.query(&forged, "snapshot", json!({})).await.unwrap();
        assert_eq!(fake_read["status"], "ok");
        assert_eq!(fake_read["value"]["Ok"], 999);
        // This accepted *counterfeit* demonstrates why sensitive data and
        // mutations MUST use a different, stronger protected profile.
        let result = tokio::time::timeout(Duration::from_secs(8), rx)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            result.status.success(),
            "fake child: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            String::from_utf8_lossy(&result.stdout)
                .contains("SAME_USER_ENDPOINT_IMPERSONATION_REPRODUCED")
        );
        fs::remove_file(path).unwrap();
        fs::remove_dir(dir).unwrap();
    });
}
