#![cfg(feature = "experimental-typed-dataflow")]

use riesel::{
    Application, ExecutionPlanDraft, LocalPort, OperationKey,
    OperationNode, OperationState, TypedEdge, TypedExecutionPlan, TypedGraphError,
    TypedOperationOutcome, TypedOperationRegistry, TypedOperationSpec, TypedOutputs,
    typed_operation,
};
use std::num::NonZeroUsize;

fn key(name: &str) -> OperationKey {
    OperationKey::new(name, 1).unwrap()
}

fn ports() -> TypedOperationRegistry {
    let mut registry = TypedOperationRegistry::new();
    registry
        .register(
            TypedOperationSpec {
                binding: key("source"),
                inputs: vec![],
                outputs: vec![LocalPort::of::<Vec<u32>>("values", "sample.u32", 1, 1024)],
            },
            typed_operation(|_| async {
                let mut result = TypedOutputs::new();
                result.insert("values", vec![1_u32, 2, 3, 4], 16).unwrap();
                TypedOperationOutcome::Completed(result)
            }),
        )
        .unwrap();
    registry
        .register(
            TypedOperationSpec {
                binding: key("sum"),
                inputs: vec![LocalPort::of::<Vec<u32>>("values", "sample.u32", 1, 1024)],
                outputs: vec![LocalPort::of::<u32>("total", "sum.u32", 1, 4)],
            },
            typed_operation(|inputs| async move {
                let values = inputs.get::<Vec<u32>>("values").unwrap();
                let mut result = TypedOutputs::new();
                result.insert("total", values.iter().sum::<u32>(), 4).unwrap();
                TypedOperationOutcome::Completed(result)
            }),
        )
        .unwrap();
    registry
        .register(
            TypedOperationSpec {
                binding: key("join"),
                inputs: vec![
                    LocalPort::of::<u32>("left", "sum.u32", 1, 4),
                    LocalPort::of::<u32>("right", "sum.u32", 1, 4),
                ],
                outputs: vec![LocalPort::of::<u32>("result", "sum.u32", 1, 4)],
            },
            typed_operation(|inputs| async move {
                let left = inputs.get::<u32>("left").unwrap();
                let right = inputs.get::<u32>("right").unwrap();
                let mut result = TypedOutputs::new();
                result.insert("result", *left + *right, 4).unwrap();
                TypedOperationOutcome::Completed(result)
            }),
        )
        .unwrap();
    registry
}

fn graph(revision: u64) -> TypedExecutionPlan {
    let validated = ExecutionPlanDraft::new("typed.fanout", revision)
        .operation(OperationNode::new("join", key("join")))
        .operation(OperationNode::new("source", key("source")))
        .operation(OperationNode::new("left", key("sum")))
        .operation(OperationNode::new("right", key("sum")))
        .validate()
        .unwrap();
    TypedExecutionPlan::new(validated)
        .connect(TypedEdge::new("source", "values", "left", "values"))
        .connect(TypedEdge::new("source", "values", "right", "values"))
        .connect(TypedEdge::new("left", "total", "join", "left"))
        .connect(TypedEdge::new("right", "total", "join", "right"))
}

#[test]
fn typed_fanout_and_fanin_run_on_the_real_riesel_session_scheduler() {
    Application::new("typed actual runner")
        .run(|context| {
            let registry = ports();
            let plan = graph(7);
            let prepared = plan.prepare(&registry, 4096)?;
            let runner = context.typed_execution_graph(
                prepared,
                NonZeroUsize::new(2).unwrap(),
            )?;
            let before = runner.observer().snapshot();
            let tokio = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let output = tokio.block_on(runner.execute());
            assert!(output.report.succeeded(), "{:?}", output.report.operations);
            assert_eq!(before.identity, output.report.identity);
            assert_eq!(output.report.revision, 7);
            assert_eq!(&*output.get::<u32>("join", "result").unwrap(), &20);
            assert_eq!(&*output.get::<Vec<u32>>("source", "values").unwrap(), &[1, 2, 3, 4]);
            assert_eq!(output.used_bytes(), 28); // one shared 16-byte source + three u32 outputs
            assert_eq!(output.report.started, 4);
            Ok(())
        })
        .unwrap();
}

#[test]
fn type_mismatch_missing_producer_combined_cycle_and_budget_fail_before_dispatch() {
    let registry = ports();
    assert!(matches!(
        graph(1).prepare(&registry, 100),
        Err(TypedGraphError::BudgetExceeded)
    ));

    let plain = ExecutionPlanDraft::new("broken", 1)
        .operation(OperationNode::new("source", key("source")))
        .operation(OperationNode::new("left", key("sum")))
        .validate()
        .unwrap();
    let invalid = TypedExecutionPlan::new(plain.clone())
        .connect(TypedEdge::new("source", "values", "left", "missing"));
    assert!(matches!(
        invalid.prepare(&registry, 4096),
        Err(TypedGraphError::UnknownEndpoint(..))
    ));
    assert!(matches!(
        TypedExecutionPlan::new(plain).prepare(&registry, 4096),
        Err(TypedGraphError::MissingInput(..))
    ));

    let incompatible = ExecutionPlanDraft::new("mismatch", 1)
        .operation(OperationNode::new("source", key("source")))
        .operation(OperationNode::new("join", key("join")))
        .validate()
        .unwrap();
    let typed = TypedExecutionPlan::new(incompatible)
        .connect(TypedEdge::new("source", "values", "join", "left"))
        .connect(TypedEdge::new("source", "values", "join", "right"));
    assert!(matches!(
        typed.prepare(&registry, 4096),
        Err(TypedGraphError::IncompatibleEdge(..))
    ));

    let cyclic = ExecutionPlanDraft::new("cycle", 1)
        .operation(OperationNode::new("source", key("source")).after("left"))
        .operation(OperationNode::new("left", key("sum")))
        .validate()
        .unwrap();
    let typed = TypedExecutionPlan::new(cyclic)
        .connect(TypedEdge::new("source", "values", "left", "values"));
    assert!(matches!(
        typed.prepare(&registry, 4096),
        Err(TypedGraphError::Plan(riesel::ExecutionPlanError::Cycle { .. }))
    ));
}

#[test]
fn completed_with_wrong_runtime_type_cannot_publish_and_blocks_consumer() {
    let mut registry = ports();
    // An entirely separate registry has a dishonest source implementation.
    registry = {
        let mut mismatched = TypedOperationRegistry::new();
        mismatched
            .register(
                TypedOperationSpec {
                    binding: key("source"),
                    inputs: vec![],
                    outputs: vec![LocalPort::of::<Vec<u32>>("values", "sample.u32", 1, 1024)],
                },
                typed_operation(|_| async {
                    let mut result = TypedOutputs::new();
                    result.insert("values", String::from("wrong"), 5).unwrap();
                    TypedOperationOutcome::Completed(result)
                }),
            )
            .unwrap();
        mismatched
            .register(
                TypedOperationSpec {
                    binding: key("sum"),
                    inputs: vec![LocalPort::of::<Vec<u32>>("values", "sample.u32", 1, 1024)],
                    outputs: vec![LocalPort::of::<u32>("total", "sum.u32", 1, 4)],
                },
                typed_operation(|_| async {
                    panic!("blocked consumer must never execute");
                }),
            )
            .unwrap();
        mismatched
    };

    let validated = ExecutionPlanDraft::new("invalid.output", 1)
        .operation(OperationNode::new("source", key("source")))
        .operation(OperationNode::new("consumer", key("sum")))
        .validate()
        .unwrap();
    let prepared = TypedExecutionPlan::new(validated)
        .connect(TypedEdge::new("source", "values", "consumer", "values"))
        .prepare(&registry, 2048)
        .unwrap();

    Application::new("typed mismatched output")
        .run(|context| {
            let runner = context.typed_execution_graph(
                prepared,
                NonZeroUsize::new(2).unwrap(),
            )?;
            let tokio = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let result = tokio.block_on(runner.execute());
            assert!(matches!(
                result.report.operations.get("source"),
                Some(OperationState::Failed(_))
            ));
            assert!(matches!(
                result.report.operations.get("consumer"),
                Some(OperationState::Blocked)
            ));
            assert!(result.get::<Vec<u32>>("source", "values").is_none());
            assert_eq!(result.used_bytes(), 0);
            Ok(())
        })
        .unwrap();
}
