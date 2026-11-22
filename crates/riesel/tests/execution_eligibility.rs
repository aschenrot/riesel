//! Accepted scheduler A1 production regression: immutable, one-shot graph
//! eligibility must retain declaration ordering and authoritative admission.
#![cfg(feature = "async-tcp")]

use riesel::{
    App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft, OperationKey,
    OperationNode, OperationOutcome, OperationRegistry, OperationState, RunningApp,
    async_operation,
};
use std::num::NonZeroUsize;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

fn key(name: &str) -> OperationKey {
    OperationKey::new(name, 1).unwrap()
}

fn owner() -> RunningApp {
    App::from_schema(
        r#"{"version":1,"app":{"name":"ReadyEligibilityRegression"},"models":{},"requires":[]}"#,
    )
    .unwrap()
    .start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
    .unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Diagnostic only: process RSS includes the allocator, runtime, test harness
/// and all previous tests. It is not an index allocation measurement or SLA.
#[cfg(target_os = "linux")]
fn process_rss_kib() -> Option<usize> {
    std::fs::read_to_string("/proc/self/status")
        .ok()?
        .lines()
        .find_map(|line| {
            line.strip_prefix("VmRSS:")
                .and_then(|value| value.split_whitespace().next())
                .and_then(|value| value.parse().ok())
        })
}

#[test]
fn ready_nodes_retain_declaration_order_after_shared_signal() {
    runtime().block_on(async {
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let completed_prerequisites = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("prerequisite"),
                async_operation({
                    let completed_prerequisites = Arc::clone(&completed_prerequisites);
                    move || {
                        let completed_prerequisites = Arc::clone(&completed_prerequisites);
                        async move {
                            completed_prerequisites.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        for name in ["x", "y"] {
            registry
                .register(
                    key(name),
                    async_operation({
                        let seen = Arc::clone(&seen);
                        let name = name.to_owned();
                        move || {
                            let seen = Arc::clone(&seen);
                            let name = name.clone();
                            async move {
                                seen.lock().unwrap().push(name);
                                OperationOutcome::Completed
                            }
                        }
                    }),
                )
                .unwrap();
        }

        // X is declared before Y, but Y's prerequisite (B) appears before
        // X's prerequisite (A). Topological traversal would prefer Y first.
        let plan = ExecutionPlanDraft::new("a1-tie-break", 1)
            .operation(
                OperationNode::new("x", key("x"))
                    .after("a")
                    .on_signal("release"),
            )
            .operation(
                OperationNode::new("y", key("y"))
                    .after("b")
                    .on_signal("release"),
            )
            .operation(OperationNode::new("b", key("prerequisite")))
            .operation(OperationNode::new("a", key("prerequisite")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();

        let app = owner();
        let runner = app
            .execution_graph(plan, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let sender = runner.signal_sender();
        let task = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while completed_prerequisites.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        sender.fire("release").unwrap();
        let report = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, 4);
        assert_eq!(seen.lock().unwrap().as_slice(), &["x", "y"]);
    });
}

#[test]
fn signal_fired_before_prerequisite_completes_is_coalesced_once() {
    runtime().block_on(async {
        let release = Arc::new(Semaphore::new(0));
        let root_started = Arc::new(AtomicUsize::new(0));
        let child_started = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("root"),
                async_operation({
                    let release = Arc::clone(&release);
                    let root_started = Arc::clone(&root_started);
                    move || {
                        let release = Arc::clone(&release);
                        let root_started = Arc::clone(&root_started);
                        async move {
                            root_started.fetch_add(1, Ordering::SeqCst);
                            let permit = release.acquire().await.unwrap();
                            permit.forget();
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        registry
            .register(
                key("child"),
                async_operation({
                    let child_started = Arc::clone(&child_started);
                    move || {
                        let child_started = Arc::clone(&child_started);
                        async move {
                            child_started.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("a1-early-signal", 1)
            .operation(
                OperationNode::new("child", key("child"))
                    .after("root")
                    .on_signal("pulse"),
            )
            .operation(OperationNode::new("root", key("root")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let app = owner();
        let runner = app
            .execution_graph(plan, NonZeroUsize::new(2).unwrap())
            .unwrap();
        let observer = runner.observer();
        let sender = runner.signal_sender();
        let task = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while root_started.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        sender.fire("pulse").unwrap();
        sender.fire("pulse").unwrap();
        assert_eq!(child_started.load(Ordering::SeqCst), 0);
        release.add_permits(1);
        let report = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(child_started.load(Ordering::SeqCst), 1);
        assert_eq!(report.started, 2);
        assert_eq!(observer.snapshot().operations["child"].attempt, 1);
    });
}

#[test]
fn ready_index_handles_wide_and_forward_reverse_chains() {
    runtime().block_on(async {
        for count in [128usize, 512, 1024] {
            for shape in ["wide", "forward", "reverse"] {
                let app = owner();
                let mut registry = OperationRegistry::<AsyncOperation>::new();
                registry
                    .register(
                        key("noop"),
                        async_operation(|| async { OperationOutcome::Completed }),
                    )
                    .unwrap();
                let mut draft = ExecutionPlanDraft::new(format!("a1-{shape}-{count}"), 1);
                for position in 0..count {
                    let index = if shape == "reverse" {
                        count - position - 1
                    } else {
                        position
                    };
                    let mut operation = OperationNode::new(format!("op-{index}"), key("noop"));
                    if shape != "wide" && index > 0 {
                        operation = operation.after(format!("op-{}", index - 1));
                    }
                    draft = draft.operation(operation);
                }
                let plan = draft.validate().unwrap().bind(&registry).unwrap();
                let runner = app
                    .execution_graph(plan, NonZeroUsize::new(32).unwrap())
                    .unwrap();
                let start = Instant::now();
                let report = tokio::time::timeout(Duration::from_secs(10), runner.execute())
                    .await
                    .unwrap();
                assert!(report.succeeded(), "{shape} {count}");
                assert_eq!(report.started, count);
                assert_eq!(report.operations.len(), count);
                assert_eq!(report.operations["op-0"], OperationState::Completed);
                assert_eq!(
                    report.operations[&format!("op-{}", count - 1)],
                    OperationState::Completed
                );
                eprintln!(
                    "scheduler A1 actual_sdk shape={shape} nodes={count} elapsed_us={}",
                    start.elapsed().as_micros()
                );
            }
        }
    });
}

#[test]
fn many_signal_waiters_do_not_block_independent_work() {
    runtime().block_on(async {
        const GATED: usize = 256;
        const INDEPENDENT: usize = 64;
        let independent = Arc::new(AtomicUsize::new(0));
        let gated = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        for (name, calls) in [
            ("gated", Arc::clone(&gated)),
            ("independent", Arc::clone(&independent)),
        ] {
            registry
                .register(
                    key(name),
                    async_operation(move || {
                        let calls = Arc::clone(&calls);
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }),
                )
                .unwrap();
        }
        let mut draft = ExecutionPlanDraft::new("a1-many-signals", 1);
        for index in 0..GATED {
            draft = draft.operation(
                OperationNode::new(format!("gated-{index}"), key("gated")).on_signal("enable"),
            );
        }
        for index in 0..INDEPENDENT {
            draft = draft.operation(OperationNode::new(
                format!("independent-{index}"),
                key("independent"),
            ));
        }
        let app = owner();
        let runner = app
            .execution_graph(
                draft.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(32).unwrap(),
            )
            .unwrap();
        let sender = runner.signal_sender();
        let task = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while independent.load(Ordering::SeqCst) != INDEPENDENT {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(gated.load(Ordering::SeqCst), 0);
        sender.fire("enable").unwrap();
        let report = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, GATED + INDEPENDENT);
        assert_eq!(gated.load(Ordering::SeqCst), GATED);
    });
}

#[test]
fn resource_blocked_ready_nodes_preserve_unrelated_branch_progress() {
    runtime().block_on(async {
        const WRITERS: usize = 256;
        const INDEPENDENT: usize = 16;
        let release = Arc::new(Semaphore::new(0));
        let writers_started = Arc::new(AtomicUsize::new(0));
        let independent_started = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("writer"),
                async_operation({
                    let release = Arc::clone(&release);
                    let writers_started = Arc::clone(&writers_started);
                    move || {
                        let release = Arc::clone(&release);
                        let writers_started = Arc::clone(&writers_started);
                        async move {
                            if writers_started.fetch_add(1, Ordering::SeqCst) == 0 {
                                let permit = release.acquire().await.unwrap();
                                permit.forget();
                            }
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
                    let independent_started = Arc::clone(&independent_started);
                    move || {
                        let independent_started = Arc::clone(&independent_started);
                        async move {
                            independent_started.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();

        let mut draft = ExecutionPlanDraft::new("a1-resource-waiters", 1);
        for i in 0..WRITERS {
            draft = draft.operation(
                OperationNode::new(format!("writer-{i}"), key("writer"))
                    .access("one-exclusive-resource", riesel::AccessMode::Write),
            );
        }
        for i in 0..INDEPENDENT {
            draft = draft.operation(OperationNode::new(
                format!("independent-{i}"),
                key("independent"),
            ));
        }
        let app = owner();
        let runner = app
            .execution_graph(
                draft.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(32).unwrap(),
            )
            .unwrap();
        let task = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while independent_started.load(Ordering::SeqCst) != INDEPENDENT {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(writers_started.load(Ordering::SeqCst), 1);
        release.add_permits(1);
        let report = tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, WRITERS + INDEPENDENT);
        assert_eq!(writers_started.load(Ordering::SeqCst), WRITERS);
    });
}

#[test]
fn repeated_runs_share_immutable_plan_without_retaining_old_eligibility() {
    runtime().block_on(async {
        const COUNT: usize = 256;
        const REPEATS: usize = 40;
        let app = owner();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("counted"),
                async_operation({
                    let calls = Arc::clone(&calls);
                    move || {
                        let calls = Arc::clone(&calls);
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let mut draft = ExecutionPlanDraft::new("a1-repeated-frame", 1);
        for i in 0..COUNT {
            let mut node = OperationNode::new(format!("op-{i}"), key("counted"));
            if i != 0 {
                node = node.after(format!("op-{}", i - 1));
            }
            draft = draft.operation(node);
        }
        let plan = draft.validate().unwrap().bind(&registry).unwrap();
        let started_at = Instant::now();
        #[cfg(target_os = "linux")]
        let rss_before = process_rss_kib();
        let mut creation_us = 0u128;
        for _ in 0..REPEATS {
            let constructing = Instant::now();
            let runner = app
                .execution_graph(plan.clone(), NonZeroUsize::new(32).unwrap())
                .unwrap();
            creation_us += constructing.elapsed().as_micros();
            let report = tokio::time::timeout(Duration::from_secs(5), runner.execute())
                .await
                .unwrap();
            assert!(report.succeeded());
            assert_eq!(report.started, COUNT);
        }
        assert_eq!(calls.load(Ordering::SeqCst), COUNT * REPEATS);
        eprintln!(
            "scheduler A1 repeated_runs count={REPEATS} nodes={COUNT} total_us={} total_runner_construction_us={creation_us}",
            started_at.elapsed().as_micros()
        );
        #[cfg(target_os = "linux")]
        eprintln!(
            "scheduler A1 indicative_process_rss_kib_before={rss_before:?} after={:?}; process RSS is NOT per-index retained heap",
            process_rss_kib()
        );
    });
}
