//! Real OS managed-child exit while a Riesel session-owned graph runs.
//! The compact TCP verbs in this fixture are not capability RPC; startup,
//! process ownership, session revocation and graph scheduling are production.
#![cfg(all(feature = "async-tcp", feature = "test-fixtures"))]

use riesel_app::{
    App, AsyncOperation, BoundExecutionPlan, CapabilityConnector, ConnectedProvider,
    ConnectorError, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft, GraphRunError,
    GraphRunPhase, OperationKey, OperationNode, OperationOutcome, OperationRegistry,
    OperationState, RunningApp, RuntimeError, ServiceEndpoint, async_operation,
    async_operation_with_effects,
};
use riesel_protocol::SessionToken;
use riesel_schema::{
    CapabilityContract, CapabilityRef, OperationDefinition, OperationKind, ProviderDefinition,
    TypeRef,
};
use std::collections::BTreeMap;
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn contract() -> CapabilityContract {
    CapabilityContract {
        capability: CapabilityRef {
            id: "test.graph.managed".into(),
            version: 1,
        },
        operations: BTreeMap::from([(
            "dispatch".into(),
            OperationDefinition {
                kind: OperationKind::Command,
                description: "fixture dispatch; not a public RPC".into(),
                inputs: vec![],
                output: TypeRef::Unit,
                error: TypeRef::Unit,
            },
        )]),
        types: BTreeMap::new(),
    }
}

struct ManagedFixtureConnector;

impl CapabilityConnector for ManagedFixtureConnector {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        Ok(contract())
    }

    fn connect(
        &self,
        endpoint: &ServiceEndpoint,
        _session: &SessionToken,
    ) -> Result<ConnectedProvider, ConnectorError> {
        let ServiceEndpoint::Tcp { address } = endpoint else {
            return Err(io::Error::other("expected resolved managed endpoint").into());
        };
        // This verifies the advertised listener is real before accepting the
        // owned provider. It does not implement the generated RPC contract.
        TcpStream::connect_timeout(address, Duration::from_secs(2))?;
        let definition = ProviderDefinition {
            id: "test.graph.managedchild".into(),
            capabilities: vec![contract().capability],
        };
        Ok(ConnectedProvider {
            definition,
            contract: contract(),
            service: Box::new(Arc::new(*address)),
        })
    }
}

fn managed_application() -> App {
    App::from_schema(
        r#"{"version":1,"app":{"name":"ManagedGraph"},"models":{},
            "requires":[{"name":"managed","capability":{
                "id":"test.graph.managed","version":1}}]}"#,
    )
    .unwrap()
}

fn managed_config() -> DeploymentConfig {
    DeploymentConfig::new(BTreeMap::from([(
        "managed".into(),
        ServiceEndpoint::Managed {
            instance: None,
            program: env!("CARGO_BIN_EXE_managed_graph_child").into(),
            args: vec![],
            startup_timeout_ms: 5_000,
            shutdown_timeout_ms: 2_000,
        },
    )]))
}

fn connectors() -> ConnectorRegistry {
    let mut registry = ConnectorRegistry::new();
    registry.register(ManagedFixtureConnector).unwrap();
    registry
}

fn execution() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

struct OnDrop(Arc<AtomicBool>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn crash_plan(
    running: &RunningApp,
    confirmed: Arc<AtomicUsize>,
    dispatched: Arc<AtomicUsize>,
    dropped: Arc<AtomicBool>,
    invalid_successor: Arc<AtomicUsize>,
) -> BoundExecutionPlan<AsyncOperation> {
    let endpoint = *running
        .service::<Arc<SocketAddr>>("managed")
        .expect("one previously resolved managed provider")
        .as_ref();
    let mut handlers = OperationRegistry::<AsyncOperation>::new();

    handlers
        .register(
            OperationKey::new("commit", 1).unwrap(),
            async_operation_with_effects(move |effects| {
                let confirmed = Arc::clone(&confirmed);
                async move {
                    let Ok(mut stream) = tokio::net::TcpStream::connect(endpoint).await else {
                        return OperationOutcome::Failed("managed listener unavailable".into());
                    };
                    if stream.write_all(b"COMMIT\n").await.is_err() {
                        return OperationOutcome::OutcomeUnknown;
                    }
                    let mut reply = [0_u8; 4];
                    match stream.read_exact(&mut reply).await {
                        Ok(_) if &reply == b"ACK\n" => {
                            confirmed.fetch_add(1, Ordering::SeqCst);
                            // This real child explicitly acknowledged COMMIT.
                            // Do not infer a confirmation for its later CRASH.
                            effects.confirm_prefix("provider.acknowledged", 1).unwrap();
                            OperationOutcome::Completed
                        }
                        _ => OperationOutcome::OutcomeUnknown,
                    }
                }
            }),
        )
        .unwrap();

    handlers
        .register(
            OperationKey::new("crash", 1).unwrap(),
            async_operation(move || {
                let dispatched = Arc::clone(&dispatched);
                let dropped = Arc::clone(&dropped);
                async move {
                    let Ok(mut stream) = tokio::net::TcpStream::connect(endpoint).await else {
                        return OperationOutcome::Failed("managed listener unavailable".into());
                    };
                    if stream.write_all(b"CRASH\n").await.is_err() {
                        return OperationOutcome::OutcomeUnknown;
                    }
                    dispatched.fetch_add(1, Ordering::SeqCst);
                    // Keep the dispatched request unresolved. Only the
                    // process monitor can revoke this application's session.
                    let _guard = OnDrop(dropped);
                    let _stream = stream;
                    std::future::pending::<OperationOutcome>().await
                }
            }),
        )
        .unwrap();

    handlers
        .register(
            OperationKey::new("successor", 1).unwrap(),
            async_operation(move || {
                let invalid_successor = Arc::clone(&invalid_successor);
                async move {
                    invalid_successor.fetch_add(1, Ordering::SeqCst);
                    OperationOutcome::Completed
                }
            }),
        )
        .unwrap();

    ExecutionPlanDraft::new("managed-crash.graph", 1)
        .operation(OperationNode::new(
            "commit",
            OperationKey::new("commit", 1).unwrap(),
        ))
        .operation(
            OperationNode::new("crash", OperationKey::new("crash", 1).unwrap()).after("commit"),
        )
        .operation(
            OperationNode::new("successor", OperationKey::new("successor", 1).unwrap())
                .after("crash")
                .on_signal("never"),
        )
        .validate()
        .unwrap()
        .bind(&handlers)
        .unwrap()
}

#[test]
fn managed_process_crash_fences_run_and_preserves_confirmed_prefix() {
    let mut running = managed_application()
        .start(&managed_config(), &connectors())
        .unwrap();
    let expired = running.session_token();
    let confirmed = Arc::new(AtomicUsize::new(0));
    let dispatched = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let successor = Arc::new(AtomicUsize::new(0));
    let runner = running
        .execution_graph(
            crash_plan(
                &running,
                Arc::clone(&confirmed),
                Arc::clone(&dispatched),
                Arc::clone(&dropped),
                Arc::clone(&successor),
            ),
            NonZeroUsize::new(2).unwrap(),
        )
        .unwrap();
    let stale_signal = runner.signal_sender();
    let observer = runner.observer();
    let address = observer.snapshot().identity;

    let result = execution().block_on(async {
        tokio::time::timeout(Duration::from_secs(8), runner.execute())
            .await
            .expect("supervised OS child crash must cancel graph")
    });
    assert!(result.session_expired);
    assert_eq!(result.operations["commit"], OperationState::Completed);
    assert_eq!(result.confirmed_prefixes["commit"]["provider.acknowledged"], 1);
    assert!(!result.confirmed_prefixes.contains_key("crash"));
    assert_eq!(result.operations["crash"], OperationState::OutcomeUnknown);
    assert_eq!(result.operations["successor"], OperationState::Blocked);
    // The same runner owns the authoritative snapshot even after a genuine
    // child process dies. The trace does not invent a committed CRASH effect.
    let snapshot = observer.snapshot();
    assert_eq!(result.identity, address);
    assert_eq!(snapshot.identity, address);
    assert_eq!(snapshot.phase, GraphRunPhase::Finished);
    assert!(snapshot.session_expired);
    assert_eq!(
        snapshot.operations["commit"].state,
        OperationState::Completed
    );
    assert_eq!(snapshot.operations["commit"].attempt, 1);
    assert_eq!(
        snapshot.operations["commit"].confirmed_prefixes["provider.acknowledged"],
        1
    );
    assert!(snapshot.operations["crash"].confirmed_prefixes.is_empty());
    assert_eq!(
        snapshot.operations["crash"].state,
        OperationState::OutcomeUnknown
    );
    assert_eq!(snapshot.operations["crash"].attempt, 1);
    assert_eq!(
        snapshot.operations["successor"].state,
        OperationState::Blocked
    );
    assert_eq!(snapshot.operations["successor"].attempt, 0);
    assert!(!observer.changes_since(0).unwrap().truncated);
    assert_eq!(confirmed.load(Ordering::SeqCst), 1);
    assert_eq!(dispatched.load(Ordering::SeqCst), 1);
    assert_eq!(successor.load(Ordering::SeqCst), 0);
    assert!(dropped.load(Ordering::SeqCst));
    assert!(matches!(
        running.check_processes(),
        Err(RuntimeError::ServiceExited { requirement, .. }) if requirement == "managed"
    ));
    assert!(matches!(
        stale_signal.fire("never"),
        Err(GraphRunError::SessionExpired)
    ));
    running.shutdown().unwrap();

    // A new session can start independently; an old graph/command is neither
    // rebound nor re-dispatched on the replacement owned service.
    let mut next = managed_application()
        .start(&managed_config(), &connectors())
        .unwrap();
    assert!(!expired.shares_session_with(&next.session_token()));
    assert!(next.session_token().is_active());
    assert!(matches!(
        stale_signal.fire("never"),
        Err(GraphRunError::SessionExpired)
    ));
    assert_eq!(dispatched.load(Ordering::SeqCst), 1);
    next.shutdown().unwrap();
}

#[test]
fn structured_graph_drains_after_managed_child_crash_without_secondary_failure() {
    let mut running = managed_application()
        .start(&managed_config(), &connectors())
        .unwrap();
    let confirmed = Arc::new(AtomicUsize::new(0));
    let dispatched = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicBool::new(false));
    let successor = Arc::new(AtomicUsize::new(0));
    let runner = running
        .execution_graph(
            crash_plan(
                &running,
                Arc::clone(&confirmed),
                Arc::clone(&dispatched),
                Arc::clone(&dropped),
                Arc::clone(&successor),
            ),
            NonZeroUsize::new(2).unwrap(),
        )
        .unwrap();

    let observer = runner.observer();
    let tasks = running.async_tasks();
    execution().block_on(async {
        tasks.begin_scope().unwrap();
        tasks.spawn_graph("managed-crash", runner).unwrap();
        tokio::time::timeout(Duration::from_secs(8), running.session_token().cancelled())
            .await
            .expect("real managed child exit must invalidate session");
        tasks.finish_scope().await.unwrap();
    });
    assert_eq!(confirmed.load(Ordering::SeqCst), 1);
    assert_eq!(dispatched.load(Ordering::SeqCst), 1);
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(successor.load(Ordering::SeqCst), 0);
    assert_eq!(
        observer.snapshot().operations["commit"].confirmed_prefixes["provider.acknowledged"],
        1
    );
    assert!(observer.snapshot().operations["crash"].confirmed_prefixes.is_empty());
    assert!(matches!(
        running.check_processes(),
        Err(RuntimeError::ServiceExited { requirement, .. }) if requirement == "managed"
    ));
    running.shutdown().unwrap();
}
