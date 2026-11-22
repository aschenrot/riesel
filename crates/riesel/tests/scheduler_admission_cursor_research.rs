//! Research #226: admission-index alternatives against the actual
//! session-owned graph runner. Never merge this diagnostic benchmark as a
//! public API, an accepted scheduler policy, or a production performance SLA.
#![cfg(feature = "async-tcp")]

use riesel::{
    App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft, GraphRunPhase,
    OperationKey, OperationNode, OperationOutcome, OperationRegistry, OperationState, RunningApp,
    async_operation,
};
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

fn key(name: &str) -> OperationKey {
    OperationKey::new(name, 1).unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn owner() -> RunningApp {
    App::from_schema(
        r#"{"version":1,"app":{"name":"AdmissionCursorResearch"},"models":{},"requires":[]}"#,
    )
    .unwrap()
    .start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
    .unwrap()
}

#[test]
fn cursor_differential_shapes() {
    runtime().block_on(async {
        const REPEATS: usize = 5;
        let owner = owner();
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("noop"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();

        // Cases alternate graph shape under each size. Timings are diagnostic
        // microbenchmarks with fixed order, not a claim about real workloads.
        for nodes in [128usize, 512, 1_024] {
            for shape in ["wide", "forward", "reverse"] {
                let mut elapsed_us = Vec::with_capacity(REPEATS);
                for repeat in 0..REPEATS {
                    let mut plan = ExecutionPlanDraft::new(
                        format!("cursor-{shape}-{nodes}-{repeat}"),
                        1,
                    );
                    let ids: Vec<usize> = if shape == "reverse" {
                        (0..nodes).rev().collect()
                    } else {
                        (0..nodes).collect()
                    };
                    for index in ids {
                        let mut node = OperationNode::new(format!("op-{index}"), key("noop"));
                        if shape != "wide" && index > 0 {
                            node = node.after(format!("op-{}", index - 1));
                        }
                        plan = plan.operation(node);
                    }
                    let bound = plan.validate().unwrap().bind(&registry).unwrap();
                    let runner = owner
                        .execution_graph(bound, NonZeroUsize::new(32).unwrap())
                        .unwrap();
                    let started = Instant::now();
                    let report = tokio::time::timeout(Duration::from_secs(15), runner.execute())
                        .await
                        .unwrap();
                    elapsed_us.push(started.elapsed().as_micros());
                    assert!(report.succeeded());
                    assert_eq!(report.started, nodes);
                    assert_eq!(report.peak_in_flight, if shape == "wide" { 32 } else { 1 });
                    assert_eq!(report.operations.len(), nodes);
                }
                elapsed_us.sort_unstable();
                eprintln!(
                    "research #226 cursor_shape={shape} nodes={nodes} samples_us={elapsed_us:?} median_us={} (release-only diagnostic, not SLA)",
                    elapsed_us[REPEATS / 2]
                );
            }
        }
    });
}

#[test]
fn pending_signal_at_prefix_does_not_block_independent_ready_work() {
    runtime().block_on(async {
        const INDEPENDENT: usize = 64;
        let completed = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("gated"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        registry
            .register(
                key("independent"),
                async_operation({
                    let completed = Arc::clone(&completed);
                    move || {
                        let completed = Arc::clone(&completed);
                        async move {
                            completed.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let mut plan = ExecutionPlanDraft::new("gated-prefix", 1)
            .operation(OperationNode::new("gated", key("gated")).on_signal("release"));
        for index in 0..INDEPENDENT {
            plan = plan.operation(OperationNode::new(format!("op-{index}"), key("independent")));
        }
        let owner = owner();
        let runner = owner
            .execution_graph(
                plan.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(32).unwrap(),
            )
            .unwrap();
        let signals = runner.signal_sender();
        let observer = runner.observer();
        let task = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while completed.load(Ordering::SeqCst) < INDEPENDENT {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(observer.snapshot().phase, GraphRunPhase::Running);
        assert_eq!(observer.snapshot().operations["gated"].state, OperationState::Pending);
        assert_eq!(observer.snapshot().operations["gated"].attempt, 0);
        signals.fire("release").unwrap();
        let report = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, INDEPENDENT + 1);
    });
}
