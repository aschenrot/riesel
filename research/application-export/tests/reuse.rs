//! Two distinct Riesel application shapes exercise the *same* provisional
//! export policy. No test pretends that simulated principals authenticate TCP.
use riesel::{
    Application, AsyncCapability, AsyncOperation, ExecutionPlanDraft, GraphRunObserver,
    OperationKey, OperationNode, OperationOutcome, OperationRegistry, OperationState, Records,
    async_operation,
};
use riesel_application_export_research::{
    AttachmentRequest, ExportError, ResearchExport, SimulatedPrincipal, grant,
};
use riesel_counter::{AsyncCounterContract, CounterError};
use riesel_counter_component::AsyncMemoryCounter;
use riesel_macros::{RieselType, capability};
use riesel_protocol::{AsyncRpcFuture, AsyncRpcHandler, CallError, RpcError};
use riesel_schema::{CapabilityContract, CapabilityDescriptor};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::error::Error;
use std::num::NonZeroUsize;
use std::sync::Arc;
use todo::{TaskData, Todo};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddArgs {
    amount: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LabelArgs {
    prefix: String,
}

fn encode<T: Serialize>(value: Result<T, CallError<CounterError>>) -> Result<Value, RpcError> {
    let domain: Result<T, CounterError> = match value {
        Ok(ok) => Ok(ok),
        Err(CallError::Domain(error)) => Err(error),
        Err(CallError::Rpc(error)) => return Err(error),
    };
    serde_json::to_value(domain).map_err(|error| RpcError::Codec(error.to_string()))
}

/// The unresolved cost: SDK-resolved client handles do not implement the
/// domain provider trait because their methods can fail at the RPC layer.
/// As in merged PR #82, forwarding needs one manual arm per operation.
struct ResolvedCounter(AsyncCapability<AsyncCounterContract>);

impl AsyncRpcHandler for ResolvedCounter {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        AsyncCounterContract::definition().map_err(|error| RpcError::Contract(error.to_string()))
    }

    fn dispatch<'a>(&'a self, operation: &'a str, payload: Value) -> AsyncRpcFuture<'a> {
        Box::pin(async move {
            match operation {
                "value" => {
                    let _: Empty = serde_json::from_value(payload)
                        .map_err(|error| RpcError::InvalidArguments(error.to_string()))?;
                    encode(self.0.value().await)
                }
                "add" => {
                    let args: AddArgs = serde_json::from_value(payload)
                        .map_err(|error| RpcError::InvalidArguments(error.to_string()))?;
                    encode(self.0.add(args.amount).await)
                }
                "label" => {
                    let args: LabelArgs = serde_json::from_value(payload)
                        .map_err(|error| RpcError::InvalidArguments(error.to_string()))?;
                    encode(self.0.label(&args.prefix).await)
                }
                _ => Err(RpcError::UnknownOperation(operation.to_owned())),
            }
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Empty {}

/// Todo uses a deliberately application-facing contract, not an export of
/// arbitrary Storage internals or of the browser HTTP schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType, thiserror::Error)]
pub enum TodoPublicError {
    #[error("Todo rejected request: {0}")]
    Domain(String),
}

#[capability(id = "riesel.research.todo.public", version = 1)]
pub trait TodoPublic {
    /// Create one validated task and return its stable record identifier.
    #[command]
    async fn create(&self, title: String) -> Result<String, TodoPublicError>;

    /// Get a current domain snapshot of the number of tasks.
    #[query]
    async fn count(&self) -> Result<u64, TodoPublicError>;
}

struct TodoFacade(Records<TaskData>);

impl TodoPublic for TodoFacade {
    async fn create(&self, title: String) -> Result<String, TodoPublicError> {
        Todo::new(self.0.clone())
            .create_task(title)
            .map(|record| record.id.to_string())
            .map_err(|error| TodoPublicError::Domain(error.to_string()))
    }

    async fn count(&self) -> Result<u64, TodoPublicError> {
        Todo::new(self.0.clone())
            .list_tasks()
            .map(|items| items.len() as u64)
            .map_err(|error| TodoPublicError::Domain(error.to_string()))
    }
}

fn success<T: serde::de::DeserializeOwned, E: serde::de::DeserializeOwned + std::fmt::Debug>(
    value: Value,
) -> T {
    let decoded: Result<T, E> = serde_json::from_value(value).unwrap();
    decoded.unwrap()
}

#[test]
fn counter_uses_sdk_resolver_and_export_policy_without_a_second_resolver() -> TestResult {
    Application::new("Counter Export Research")
        .async_handler(
            |counter: AsyncCapability<AsyncCounterContract>| async move {
                let relay = Arc::new(ResolvedCounter(counter));
                let gateway = ResearchExport::new(
                    "org.example.counter",
                    "gui.counter",
                    "s1-provisional-nonce",
                    relay,
                    grant("gui", &["value", "add"]),
                )?;
                let descriptor = gateway.descriptor();
                let attached = gateway.attach(&descriptor, SimulatedPrincipal::for_test("gui"))?;
                assert_eq!(
                    success::<u64, CounterError>(
                        gateway.invoke(&attached, "value", json!({})).await?
                    ),
                    0
                );
                assert_eq!(
                    success::<u64, CounterError>(
                        gateway
                            .invoke(&attached, "add", json!({"amount": 5}))
                            .await?
                    ),
                    5
                );
                // Disallowed operation is blocked *before* dispatch.
                assert_eq!(
                    gateway
                        .invoke(&attached, "label", json!({"prefix": "x"}))
                        .await,
                    Err(ExportError::Forbidden)
                );
                assert_eq!(
                    gateway
                        .attach(&descriptor, SimulatedPrincipal::for_test("untrusted"))
                        .unwrap_err(),
                    ExportError::Unauthorized
                );
                let mut wrong = descriptor.clone();
                wrong.contract.version += 1;
                assert_eq!(
                    gateway
                        .attach(&wrong, SimulatedPrincipal::for_test("gui"))
                        .unwrap_err(),
                    ExportError::ContractMismatch
                );
                assert_eq!(
                    success::<u64, CounterError>(
                        gateway.invoke(&attached, "value", json!({})).await?
                    ),
                    5
                );
                gateway.revoke();
                assert_eq!(
                    gateway.invoke(&attached, "add", json!({"amount": 1})).await,
                    Err(ExportError::NotReady)
                );
                Ok::<(), Box<dyn Error + Send + Sync>>(())
            },
        )?
        .component::<AsyncMemoryCounter>()?
        .run()?;
    Ok(())
}

#[test]
fn todo_uses_same_gateway_with_sdk_records_and_a_generated_app_contract() -> TestResult {
    Application::new("Todo Export Research")
        .async_handler(|records: Records<TaskData>| async move {
            let handler: Arc<dyn AsyncRpcHandler> =
                Arc::new(TodoPublicAsyncRpcDispatcher::new(TodoFacade(records)));
            let first = ResearchExport::new(
                "org.example.todo",
                "gui.todo",
                "session-one",
                Arc::clone(&handler),
                grant("gui", &["count", "create"]),
            )?;
            let descriptor = first.descriptor();
            let one = first.attach(&descriptor, SimulatedPrincipal::for_test("gui"))?;
            assert_eq!(
                success::<u64, TodoPublicError>(first.invoke(&one, "count", json!({})).await?),
                0
            );
            let created: String = success::<String, TodoPublicError>(
                first
                    .invoke(&one, "create", json!({"title": "Plan"}))
                    .await?,
            );
            assert!(!created.is_empty());
            let domain: Result<String, TodoPublicError> = serde_json::from_value(
                first.invoke(&one, "create", json!({"title": "  "})).await?,
            )?;
            assert!(matches!(domain, Err(TodoPublicError::Domain(_))));

            // Simulated restart: same domain handler is deliberately reused.
            // This proves policy epoch rejection, NOT actual OS/session recovery.
            let second = ResearchExport::new(
                "org.example.todo",
                "gui.todo",
                "session-two",
                handler,
                grant("gui", &["count", "create"]),
            )?;
            assert_eq!(
                second
                    .invoke(&one, "create", json!({"title": "stale"}))
                    .await,
                Err(ExportError::StaleGeneration)
            );
            let mut forged = AttachmentRequest {
                generation: "session-one".into(),
                ..second.descriptor()
            };
            assert_eq!(
                second
                    .attach(&forged, SimulatedPrincipal::for_test("gui"))
                    .unwrap_err(),
                ExportError::StaleGeneration
            );
            forged.generation = "session-two".into();
            let fresh = second.attach(&forged, SimulatedPrincipal::for_test("gui"))?;
            assert_eq!(
                success::<u64, TodoPublicError>(second.invoke(&fresh, "count", json!({})).await?),
                1
            );
            assert_eq!(
                first.invoke(&one, "count", json!({})).await.is_ok(),
                true,
                "an unrevoked gateway remains active: old owner must revoke it"
            );
            first.revoke();
            assert_eq!(
                first.invoke(&one, "count", json!({})).await,
                Err(ExportError::NotReady)
            );
            Ok::<(), Box<dyn Error + Send + Sync>>(())
        })?
        .development()?
        .run()?;
    Ok(())
}

struct LostReply;
impl AsyncRpcHandler for LostReply {
    fn contract(&self) -> Result<CapabilityContract, RpcError> {
        AsyncCounterContract::definition().map_err(|error| RpcError::Contract(error.to_string()))
    }

    fn dispatch<'a>(&'a self, _: &'a str, _: Value) -> AsyncRpcFuture<'a> {
        Box::pin(async { Err(RpcError::Transport("reply deliberately withheld".into())) })
    }
}

#[tokio::test]
async fn uncertain_write_is_not_a_domain_error_or_automatic_retry() {
    let gateway = ResearchExport::new(
        "counter",
        "facade",
        "epoch",
        Arc::new(LostReply),
        grant("gui", &["value", "add"]),
    )
    .unwrap();
    let ticket = gateway
        .attach(&gateway.descriptor(), SimulatedPrincipal::for_test("gui"))
        .unwrap();
    assert_eq!(
        gateway.invoke(&ticket, "add", json!({"amount": 1})).await,
        Err(ExportError::UnknownMutationOutcome)
    );
    assert!(matches!(
        gateway.invoke(&ticket, "value", json!({})).await,
        Err(ExportError::Rpc(RpcError::Transport(_)))
    ));
}

#[test]
fn invalid_export_grants_fail_during_construction() {
    let bad = ResearchExport::new(
        "counter",
        "facade",
        "epoch",
        Arc::new(LostReply),
        grant("gui", &["not_an_operation"]),
    );
    assert!(matches!(bad, Err(ExportError::Invalid(_))));
}

// Third actual SDK owner shape: the graph runner is created inside the real
// application session. This view never owns execution, a graph-run controller
// or a provider; it reads GraphRunObserver only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType)]
pub struct GraphReadSummary {
    plan_id: String,
    revision: u64,
    session_generation: String,
    run_sequence: u64,
    phase: String,
    latest_sequence: u64,
    session_expired: bool,
    trace_truncated: bool,
    pending: u64,
    completed: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, RieselType, thiserror::Error)]
pub enum GraphReadError {
    #[error("graph observation failed: {0}")]
    Unavailable(String),
}

#[capability(id = "riesel.research.graph.read", version = 1)]
pub trait GraphRead {
    /// Read a bounded summary of the authoritative Riesel-owned graph run.
    #[query]
    async fn snapshot(&self) -> Result<GraphReadSummary, GraphReadError>;
}

struct GraphReadFacade(GraphRunObserver);

impl GraphRead for GraphReadFacade {
    async fn snapshot(&self) -> Result<GraphReadSummary, GraphReadError> {
        let current = self.0.snapshot();
        let history = self
            .0
            .changes_since(0)
            .map_err(|err| GraphReadError::Unavailable(err.to_string()))?;
        Ok(GraphReadSummary {
            plan_id: current.identity.plan_id().to_owned(),
            revision: current.identity.revision(),
            session_generation: current.identity.session_generation().to_string(),
            run_sequence: current.identity.run_sequence(),
            phase: format!("{:?}", current.phase),
            latest_sequence: current.latest_sequence,
            session_expired: current.session_expired,
            trace_truncated: history.truncated,
            pending: current
                .operations
                .values()
                .filter(|n| matches!(n.state, OperationState::Pending))
                .count() as u64,
            completed: current
                .operations
                .values()
                .filter(|n| matches!(n.state, OperationState::Completed))
                .count() as u64,
        })
    }
}

#[test]
fn graph_uses_same_gateway_with_sdk_run_and_read_only_generated_contract() -> TestResult {
    Application::new("Graph Export Research").run(|context| {
        let mut operations = OperationRegistry::<AsyncOperation>::new();
        for name in ["source", "sink"] {
            operations.register(
                OperationKey::new(name, 1)?,
                async_operation(|| async { OperationOutcome::Completed }),
            )?;
        }
        let plan = ExecutionPlanDraft::new("research.gui-public-graph", 1)
            .operation(OperationNode::new(
                "source",
                OperationKey::new("source", 1)?,
            ))
            .operation(OperationNode::new("sink", OperationKey::new("sink", 1)?).after("source"))
            .validate()?
            .bind(&operations)?;
        let runner = context.execution_graph(plan, NonZeroUsize::new(1).unwrap())?;
        let observer = runner.observer();
        let handler: Arc<dyn AsyncRpcHandler> = Arc::new(GraphReadAsyncRpcDispatcher::new(
            GraphReadFacade(observer.clone()),
        ));
        let gateway = ResearchExport::new(
            "org.example.graph",
            "gui.graph",
            "simulated-graph-epoch",
            handler,
            grant("gui", &["snapshot"]),
        )
        .expect("test-only publication policy");
        let descriptor = gateway.descriptor();
        let attached = gateway
            .attach(&descriptor, SimulatedPrincipal::for_test("gui"))
            .expect("simulated approved client");
        let mut stale = descriptor.clone();
        stale.generation = "another-epoch".into();
        assert_eq!(
            gateway
                .attach(&stale, SimulatedPrincipal::for_test("gui"))
                .unwrap_err(),
            ExportError::StaleGeneration
        );
        assert_eq!(
            gateway
                .attach(&descriptor, SimulatedPrincipal::for_test("other"))
                .unwrap_err(),
            ExportError::Unauthorized
        );
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("research Tokio runtime");
        executor.block_on(async {
            let initial: GraphReadSummary = success(
                gateway
                    .invoke(&attached, "snapshot", json!({}))
                    .await
                    .unwrap(),
            );
            assert_eq!(initial.plan_id, "research.gui-public-graph");
            assert_eq!(initial.revision, 1);
            assert_eq!(initial.phase, "Created");
            assert_eq!(initial.pending, 2);
            assert_eq!(initial.completed, 0);
            assert!(!initial.session_expired);
            assert!(!initial.trace_truncated);
            let report = runner.execute().await;
            assert!(report.succeeded());
            let finished: GraphReadSummary = success(
                gateway
                    .invoke(&attached, "snapshot", json!({}))
                    .await
                    .unwrap(),
            );
            assert_eq!(finished.phase, "Finished");
            assert_eq!(finished.completed, 2);
            assert_eq!(finished.pending, 0);
            assert_eq!(finished.session_generation, initial.session_generation);
            assert_eq!(finished.run_sequence, initial.run_sequence);
            assert!(finished.latest_sequence > initial.latest_sequence);
            assert_eq!(
                finished.session_generation,
                report.identity.session_generation().to_string()
            );
            assert_eq!(
                gateway.invoke(&attached, "not-exposed", json!({})).await,
                Err(ExportError::UnknownOperation)
            );
            gateway.revoke();
            assert_eq!(
                gateway.invoke(&attached, "snapshot", json!({})).await,
                Err(ExportError::NotReady)
            );
        });
        Ok(())
    })?;
    Ok(())
}
