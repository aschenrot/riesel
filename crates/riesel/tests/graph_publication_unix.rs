//! Real authoritative graph -> opt-in versioned capability -> T1 Unix client.
//! A second OS process reads the graph but cannot admit/control/mutate work.
#![cfg(all(unix, feature = "app-publication-unix"))]

use riesel::{
    App, AppReadPublication, AsyncOperation, ConnectorError, ConnectorRegistry,
    ExecutionGraphReadAsyncRpcDispatcher, ExecutionGraphReadFacade, ExecutionPlanDraft,
    GraphReadChanges, GraphReadError, GraphReadSnapshot, OperationKey, OperationNode,
    OperationOutcome, OperationRegistry, ReadExposure, RunningApp, UnixReadClient, UnixReadServer,
    ValidatedExecutionPlan, async_operation,
};
use serde_json::{Value, json};
use std::fs;
use std::num::NonZeroUsize;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

const SCHEMA: &str =
    r#"{"version":1,"app":{"name":"RealGraphReadOwner"},"models":{},"requires":[]}"#;
const APPLICATION: &str = "research.real_graph_owner";
const INTERFACE: &str = "gui.execution_graph";

fn app() -> RunningApp {
    App::from_schema(SCHEMA)
        .unwrap()
        .start_local(&ConnectorRegistry::new())
        .unwrap()
}

fn graph(
    running: &RunningApp,
    count: usize,
) -> (riesel::ExecutionGraphRunner, ValidatedExecutionPlan) {
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    let mut draft = ExecutionPlanDraft::new("example.gui_public_graph", 1);
    let mut predecessor: Option<String> = None;
    for index in 0..count {
        let name = format!("node_{index:03}");
        let key = OperationKey::new(&name, 1).unwrap();
        registry
            .register(
                key.clone(),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        let mut node = OperationNode::new(&name, key);
        if let Some(previous) = &predecessor {
            node = node.after(previous);
        }
        draft = draft.operation(node);
        predecessor = Some(name);
    }
    let validated = draft.validate().unwrap();
    let runner = running
        .execution_graph(
            validated.bind(&registry).unwrap(),
            NonZeroUsize::new(2).unwrap(),
        )
        .unwrap();
    (runner, validated)
}

fn graph_with_uncertain_effects(
    running: &RunningApp,
) -> (riesel::ExecutionGraphRunner, ValidatedExecutionPlan) {
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    registry
        .register(
            OperationKey::new("failed", 1).unwrap(),
            async_operation(|| async { OperationOutcome::Failed("domain rejected".into()) }),
        )
        .unwrap();
    registry
        .register(
            OperationKey::new("blocked", 1).unwrap(),
            async_operation(|| async { OperationOutcome::Completed }),
        )
        .unwrap();
    registry
        .register(
            OperationKey::new("unknown", 1).unwrap(),
            async_operation(|| async { OperationOutcome::OutcomeUnknown }),
        )
        .unwrap();
    let validated = ExecutionPlanDraft::new("example.gui_public_graph", 1)
        .operation(OperationNode::new("failed", OperationKey::new("failed", 1).unwrap()))
        .operation(
            OperationNode::new("blocked", OperationKey::new("blocked", 1).unwrap())
                .after("failed"),
        )
        .operation(OperationNode::new("unknown", OperationKey::new("unknown", 1).unwrap()))
        .validate()
        .unwrap();
    let runner = running
        .execution_graph(validated.bind(&registry).unwrap(), NonZeroUsize::new(2).unwrap())
        .unwrap();
    (runner, validated)
}

fn published(
    app: &RunningApp,
    validated: &ValidatedExecutionPlan,
    runner: &riesel::ExecutionGraphRunner,
) -> Arc<AppReadPublication> {
    let facade =
        ExecutionGraphReadFacade::new(validated, runner.observer()).expect("matching authority");
    let handler = Arc::new(ExecutionGraphReadAsyncRpcDispatcher::new(facade));
    Arc::new(
        app.publish_local_reads(
            APPLICATION,
            INTERFACE,
            ReadExposure::NonSensitiveSharedUser,
            handler,
            ["snapshot".to_owned(), "changes".to_owned()],
        )
        .expect("selected only read queries"),
    )
}

fn decode<T: serde::de::DeserializeOwned>(reply: Value) -> T {
    assert_eq!(reply["status"], "ok", "transport reply: {reply}");
    serde_json::from_value(reply["value"].clone()).expect("typed generated dispatch result")
}

async fn read_snapshot(
    client: &UnixReadClient,
    descriptor: &riesel::ReadPublicationDescriptor,
) -> GraphReadSnapshot {
    let result: Result<GraphReadSnapshot, GraphReadError> = decode(
        client
            .query(descriptor, "snapshot", json!({}))
            .await
            .unwrap(),
    );
    result.unwrap()
}

async fn read_changes(
    client: &UnixReadClient,
    descriptor: &riesel::ReadPublicationDescriptor,
    identity: &riesel::GraphReadIdentity,
    cursor: u64,
) -> Result<GraphReadChanges, GraphReadError> {
    decode(
        client
            .query(
                descriptor,
                "changes",
                json!({
                    "session_generation": identity.session_generation,
                    "run_sequence": identity.run_sequence,
                    "cursor": cursor
                }),
            )
            .await
            .unwrap(),
    )
}

/// This executable test process acts as an independent native read-only GUI.
#[test]
fn independent_native_graph_reader() {
    let Ok(socket) = std::env::var("RIESEL_GRAPH_READER_SOCKET") else {
        return;
    };
    let expected_phase = std::env::var("RIESEL_GRAPH_READER_PHASE").unwrap();
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    executor.block_on(async {
        let client = UnixReadClient::new(socket);
        let descriptor = client.describe().await.unwrap();
        assert_eq!(descriptor.application, APPLICATION);
        assert_eq!(descriptor.interface, INTERFACE);
        assert_eq!(descriptor.contract.id, "riesel.execution.graph_read");
        assert_eq!(descriptor.contract.version, 1);
        assert_eq!(descriptor.allowed_queries, ["changes", "snapshot"]);
        let snapshot = read_snapshot(&client, &descriptor).await;
        assert_eq!(snapshot.nodes.len(), 80);
        assert_eq!(snapshot.phase, expected_phase);
        assert_eq!(snapshot.nodes[0].id, "node_000");
        assert_eq!(snapshot.nodes[79].depends_on, ["node_078"]);
        let forbidden = client
            .query(&descriptor, "activate", json!({}))
            .await
            .unwrap();
        assert_eq!(forbidden["status"], "error");
        assert_eq!(forbidden["code"], "not_published");
        println!("INDEPENDENT_GRAPH_READER_NO_CONTROL");
    });
}

async fn external_reader(path: &Path, phase: &str) {
    let command = std::env::current_exe().unwrap();
    let socket = path.to_string_lossy().to_string();
    let phase = phase.to_owned();
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let output = Command::new(command)
            .arg("--exact")
            .arg("independent_native_graph_reader")
            .arg("--nocapture")
            .env("RIESEL_GRAPH_READER_SOCKET", socket)
            .env("RIESEL_GRAPH_READER_PHASE", phase)
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
        "independent reader failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("INDEPENDENT_GRAPH_READER_NO_CONTROL")
    );
}

#[test]
fn actual_graph_read_loss_resnapshot_and_same_pid_session_recovery() {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(async {
        let tmp = tempfile::tempdir().unwrap();
        let endpoint = tmp.path().join("graph.sock");
        fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let pid = std::process::id();
        let mut first = app();
        let (runner, validated) = graph(&first, 80);
        let publication = published(&first, &validated, &runner);
        let tasks = first.async_tasks();
        tasks.begin_scope().unwrap();
        let server = UnixReadServer::bind(&endpoint).unwrap();
        let serving = Arc::clone(&publication);
        tasks
            .spawn("graph-read-export", async move {
                server
                    .serve(serving)
                    .await
                    .map_err(|error| -> ConnectorError { Box::new(error) })
            })
            .unwrap();

        let client = UnixReadClient::new(&endpoint);
        let old_descriptor = client.describe().await.unwrap();
        let created = read_snapshot(&client, &old_descriptor).await;
        assert_eq!(created.phase, "created");
        assert_eq!(created.nodes.len(), 80);
        assert!(created.nodes.iter().all(|node| node.state == "pending"));
        external_reader(&endpoint, "created").await;

        let report = runner.execute().await;
        assert!(report.succeeded(), "{:?}", report.operations);
        let finished = read_snapshot(&client, &old_descriptor).await;
        assert_eq!(finished.phase, "finished");
        assert_eq!(finished.identity, created.identity);
        assert!(finished.latest_sequence > 128);
        assert!(finished.nodes.iter().all(|node| node.state == "completed"));
        assert_eq!(finished.nodes.len(), 80);
        external_reader(&endpoint, "finished").await;

        let history = read_changes(&client, &old_descriptor, &finished.identity, 0)
            .await
            .unwrap();
        assert!(history.truncated, "real graph exceeded bounded trace ring");
        assert!(history.events.is_empty(), "no partial replay on loss");
        assert_eq!(history.latest_sequence, finished.latest_sequence);
        let current = read_changes(
            &client,
            &old_descriptor,
            &finished.identity,
            finished.latest_sequence,
        )
        .await
        .unwrap();
        assert!(!current.truncated);
        assert!(current.events.is_empty());
        assert_eq!(
            read_changes(
                &client,
                &old_descriptor,
                &finished.identity,
                finished.latest_sequence + 1,
            )
            .await,
            Err(GraphReadError::FutureCursor)
        );
        assert_eq!(
            read_changes(
                &client,
                &old_descriptor,
                &riesel::GraphReadIdentity {
                    session_generation: "other-run".into(),
                    ..finished.identity.clone()
                },
                0,
            )
            .await,
            Err(GraphReadError::StaleRun)
        );
        // Stale run has priority even when a cursor is ahead of its last
        // sequence; never confuse a different generation with a future event.
        assert_eq!(
            read_changes(
                &client,
                &old_descriptor,
                &riesel::GraphReadIdentity {
                    session_generation: "other-run".into(),
                    ..finished.identity.clone()
                },
                finished.latest_sequence + 1,
            )
            .await,
            Err(GraphReadError::StaleRun)
        );

        publication.withdraw();
        let denied = client
            .query(&old_descriptor, "snapshot", json!({}))
            .await
            .unwrap();
        assert_eq!(denied["code"], "withdrawn");
        tasks.finish_scope().await.unwrap();
        first.shutdown().unwrap();
        assert!(!endpoint.exists());

        // Exact same OS PID/socket, a new true Riesel runtime generation.
        let mut successor_app = app();
        assert_eq!(std::process::id(), pid);
        let (new_runner, new_plan) = graph_with_uncertain_effects(&successor_app);
        let new_publication = published(&successor_app, &new_plan, &new_runner);
        let next_tasks = successor_app.async_tasks();
        next_tasks.begin_scope().unwrap();
        let next_server = UnixReadServer::bind(&endpoint).unwrap();
        let active = Arc::clone(&new_publication);
        next_tasks
            .spawn("successor-graph-read-export", async move {
                next_server
                    .serve(active)
                    .await
                    .map_err(|error| -> ConnectorError { Box::new(error) })
            })
            .unwrap();
        let stale = client
            .query(&old_descriptor, "snapshot", json!({}))
            .await
            .unwrap();
        assert_eq!(stale["code"], "stale_generation");
        let fresh = client.describe().await.unwrap();
        assert_ne!(fresh.generation, old_descriptor.generation);
        let second = read_snapshot(&client, &fresh).await;
        assert_eq!(second.phase, "created");
        assert_eq!(second.nodes.len(), 3);
        assert_ne!(
            second.identity.session_generation,
            finished.identity.session_generation
        );
        assert_ne!(second.identity.run_sequence, 0);
        let second_report = new_runner.execute().await;
        assert!(!second_report.succeeded(), "known failure/unknown are not completion");
        let terminal = read_snapshot(&client, &fresh).await;
        assert_eq!(terminal.phase, "finished");
        assert_eq!(terminal.nodes[0].state, "failed");
        assert_eq!(terminal.nodes[0].failure.as_deref(), Some("domain rejected"));
        assert_eq!(terminal.nodes[1].state, "blocked");
        assert_eq!(terminal.nodes[2].state, "outcome_unknown");
        next_tasks.finish_scope().await.unwrap();
        successor_app.shutdown().unwrap();
        assert!(!endpoint.exists());
    });
}
