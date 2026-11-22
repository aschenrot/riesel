//! Research #226: admission-index alternatives against the actual
//! session-owned graph runner. Never merge this diagnostic benchmark as a
//! public API, an accepted scheduler policy, or a production performance SLA.
#![cfg(feature = "async-tcp")]

use riesel::{
    AccessMode, App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft,
    GraphRunPhase, OperationKey, OperationNode, OperationOutcome, OperationRegistry,
    OperationState, RunningApp, async_operation,
};
use std::num::NonZeroUsize;
use std::sync::{
    Arc, Mutex,
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
            plan = plan.operation(OperationNode::new(
                format!("op-{index}"),
                key("independent"),
            ));
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
        assert_eq!(
            observer.snapshot().operations["gated"].state,
            OperationState::Pending
        );
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

#[test]
fn topology_changes_ready_tie_break_after_shared_signal() {
    runtime().block_on(async {
        let prerequisites = Arc::new(AtomicUsize::new(0));
        let observed = Arc::new(Mutex::new(Vec::<&'static str>::new()));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        for name in ["a", "b"] {
            registry
                .register(
                    key(name),
                    async_operation({
                        let prerequisites = Arc::clone(&prerequisites);
                        move || {
                            let prerequisites = Arc::clone(&prerequisites);
                            async move {
                                prerequisites.fetch_add(1, Ordering::SeqCst);
                                OperationOutcome::Completed
                            }
                        }
                    }),
                )
                .unwrap();
        }
        for name in ["x", "y"] {
            registry
                .register(
                    key(name),
                    async_operation({
                        let observed = Arc::clone(&observed);
                        move || {
                            let observed = Arc::clone(&observed);
                            async move {
                                observed.lock().unwrap().push(name);
                                OperationOutcome::Completed
                            }
                        }
                    }),
                )
                .unwrap();
        }
        // Declaration order: X, Y, B, A. Both X and Y are held by the
        // same signal until A and B have finished. Dynamic declaration-order
        // scheduling chooses X then Y. Validated topological traversal is
        // B, Y, A, X and chooses Y then X. Both are legal dependency orders,
        // but a mechanism change must not silently promise identical order.
        let plan = ExecutionPlanDraft::new("ready-tie-break", 1)
            .operation(OperationNode::new("x", key("x")).after("a").on_signal("go"))
            .operation(OperationNode::new("y", key("y")).after("b").on_signal("go"))
            .operation(OperationNode::new("b", key("b")))
            .operation(OperationNode::new("a", key("a")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = owner();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let signals = runner.signal_sender();
        let task = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while prerequisites.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        signals.fire("go").unwrap();
        let report = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, 4);
        let order = observed.lock().unwrap().clone();
        let expected = if std::env::var_os("RIESEL_RESEARCH_TOPO_CURSOR").is_some() {
            vec!["y", "x"]
        } else {
            vec!["x", "y"]
        };
        assert_eq!(order, expected);
        eprintln!(
            "research #226 tie_break topology={} order={order:?}",
            std::env::var_os("RIESEL_RESEARCH_TOPO_CURSOR").is_some()
        );
    });
}

#[test]
fn many_signal_gated_ready_nodes_leave_unrelated_work_eligible() {
    runtime().block_on(async {
        const GATED: usize = 256;
        const INDEPENDENT: usize = 64;
        let independent_count = Arc::new(AtomicUsize::new(0));
        let gated_count = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("gated"),
                async_operation({
                    let gated_count = Arc::clone(&gated_count);
                    move || {
                        let gated_count = Arc::clone(&gated_count);
                        async move {
                            gated_count.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        registry
            .register(
                key("independent"),
                async_operation({
                    let independent_count = Arc::clone(&independent_count);
                    move || {
                        let independent_count = Arc::clone(&independent_count);
                        async move {
                            independent_count.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let mut draft = ExecutionPlanDraft::new("signal-ready-stress", 1);
        for index in 0..GATED {
            draft = draft.operation(
                OperationNode::new(format!("gated-{index}"), key("gated")).on_signal("release"),
            );
        }
        for index in 0..INDEPENDENT {
            draft = draft.operation(OperationNode::new(
                format!("independent-{index}"),
                key("independent"),
            ));
        }
        let owner = owner();
        let runner = owner
            .execution_graph(
                draft.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(32).unwrap(),
            )
            .unwrap();
        let observer = runner.observer();
        let sender = runner.signal_sender();
        let task = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while independent_count.load(Ordering::SeqCst) < INDEPENDENT {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(gated_count.load(Ordering::SeqCst), 0);
        assert_eq!(observer.snapshot().operations["gated-0"].attempt, 0);
        sender.fire("release").unwrap();
        let report = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, GATED + INDEPENDENT);
        assert_eq!(gated_count.load(Ordering::SeqCst), GATED);
        eprintln!(
            "research #226 signal_ready_stress gated={GATED} independent={INDEPENDENT} finished"
        );
    });
}

#[test]
fn resource_contended_ready_nodes_preserve_independent_progress() {
    runtime().block_on(async {
        const WRITERS: usize = 256;
        const INDEPENDENT: usize = 16;
        let writers = Arc::new(AtomicUsize::new(0));
        let independent = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("writer"),
                async_operation({
                    let writers = Arc::clone(&writers);
                    move || {
                        let writers = Arc::clone(&writers);
                        async move {
                            writers.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        registry
            .register(
                key("unrelated"),
                async_operation({
                    let independent = Arc::clone(&independent);
                    move || {
                        let independent = Arc::clone(&independent);
                        async move {
                            independent.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();

        let mut draft = ExecutionPlanDraft::new("resource-ready-stress", 1);
        for index in 0..WRITERS {
            draft = draft.operation(
                OperationNode::new(format!("writer-{index}"), key("writer"))
                    .access("one-resource", AccessMode::Write),
            );
        }
        for index in 0..INDEPENDENT {
            draft = draft.operation(OperationNode::new(
                format!("independent-{index}"),
                key("unrelated"),
            ));
        }
        let runner = owner()
            .execution_graph(
                draft.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(32).unwrap(),
            )
            .unwrap();
        let report = tokio::time::timeout(Duration::from_secs(8), runner.execute())
            .await
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, WRITERS + INDEPENDENT);
        assert_eq!(writers.load(Ordering::SeqCst), WRITERS);
        assert_eq!(independent.load(Ordering::SeqCst), INDEPENDENT);
        eprintln!(
            "research #226 resource_ready_stress writers={WRITERS} independent={INDEPENDENT} finished"
        );
    });
}

#[test]
fn reverse_declared_fanout_fanin_completes_all_registered_operations() {
    runtime().block_on(async {
        const BRANCHES: usize = 96;
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("noop"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();

        let mut final_join = OperationNode::new("join", key("noop"));
        for index in 0..BRANCHES {
            final_join = final_join.after(format!("branch-{index}"));
        }
        let mut draft = ExecutionPlanDraft::new("reverse-fanout-fanin", 1).operation(final_join);
        for index in (0..BRANCHES).rev() {
            draft = draft.operation(
                OperationNode::new(format!("branch-{index}"), key("noop")).after("root"),
            );
        }
        draft = draft.operation(OperationNode::new("root", key("noop")));
        let runner = owner()
            .execution_graph(
                draft.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(32).unwrap(),
            )
            .unwrap();
        let observer = runner.observer();
        let report = tokio::time::timeout(Duration::from_secs(5), runner.execute())
            .await
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, BRANCHES + 2);
        assert_eq!(report.operations["join"], OperationState::Completed);
        assert_eq!(observer.snapshot().operations["join"].attempt, 1);
        eprintln!("research #226 reverse fanout/fanin branches={BRANCHES} finished");
    });
}
