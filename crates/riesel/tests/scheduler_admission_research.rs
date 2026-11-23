//! #226 research-only cross-run successful-admission witness.
//! A feature-gated, bounded control-gate probe; not a supported public API.
#![cfg(feature = "internal-admission-research")]

use riesel::{
    AccessMode, App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft,
    GraphControlTarget, GraphRunPhase, OperationKey, OperationNode, OperationOutcome,
    OperationRegistry, RunningApp, async_operation,
};
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::sync::Semaphore;

fn key(name: &str) -> OperationKey {
    OperationKey::new(name, 1).unwrap()
}

fn owner() -> RunningApp {
    App::from_schema(
        r#"{"version":1,"app":{"name":"AdmissionWitnessResearch"},"models":{},"requires":[]}"#,
    )
    .unwrap()
    .start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
    .unwrap()
}

fn runtime(multi_worker: bool) -> tokio::runtime::Runtime {
    let mut builder = if multi_worker {
        let mut builder = tokio::runtime::Builder::new_multi_thread();
        builder.worker_threads(4);
        builder
    } else {
        tokio::runtime::Builder::new_current_thread()
    };
    builder.enable_all().build().unwrap()
}

async fn wait_started(counter: &AtomicUsize, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while counter.load(Ordering::SeqCst) < expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn running(observer: &riesel::GraphRunObserver) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while observer.snapshot().phase != GraphRunPhase::Running {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn verify_session_admissions(multi_worker: bool, capacity_only: bool) {
    const HOT_COUNT: usize = 64;
    const ROUNDS: usize = 4;
    let profile = if multi_worker {
        "four_workers"
    } else {
        "current_thread"
    };
    let workload = if capacity_only {
        "capacity_only"
    } else {
        "read_write"
    };
    runtime(multi_worker).block_on(async {
        for round in 0..ROUNDS {
            let app = owner();
            let held = if capacity_only { 32 } else { 1 };
            let release = Arc::new(Semaphore::new(0));
            let hot_started = Arc::new(AtomicUsize::new(0));
            let callback_order = Arc::new(AtomicUsize::new(0));
            let cold_callback_rank = Arc::new(AtomicUsize::new(0));
            let mut registry = OperationRegistry::<AsyncOperation>::new();

            registry
                .register(
                    key("hot"),
                    async_operation({
                        let release = Arc::clone(&release);
                        let hot_started = Arc::clone(&hot_started);
                        let callback_order = Arc::clone(&callback_order);
                        move || {
                            let release = Arc::clone(&release);
                            let hot_started = Arc::clone(&hot_started);
                            let callback_order = Arc::clone(&callback_order);
                            async move {
                                callback_order.fetch_add(1, Ordering::SeqCst);
                                let number = hot_started.fetch_add(1, Ordering::SeqCst) + 1;
                                if number <= held {
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
                    key("cold"),
                    async_operation({
                        let callback_order = Arc::clone(&callback_order);
                        let cold_callback_rank = Arc::clone(&cold_callback_rank);
                        move || {
                            let callback_order = Arc::clone(&callback_order);
                            let cold_callback_rank = Arc::clone(&cold_callback_rank);
                            async move {
                                let rank = callback_order.fetch_add(1, Ordering::SeqCst) + 1;
                                cold_callback_rank.store(rank, Ordering::SeqCst);
                                OperationOutcome::Completed
                            }
                        }
                    }),
                )
                .unwrap();

            let mut hot_draft =
                ExecutionPlanDraft::new(format!("hot-{workload}-{profile}-{round}"), 1);
            for index in 0..HOT_COUNT {
                let mut node = OperationNode::new(format!("hot-{index}"), key("hot"));
                if !capacity_only {
                    node = node.access("shared", AccessMode::Read);
                }
                hot_draft = hot_draft.operation(node);
            }
            let hot = app
                .execution_graph(
                    hot_draft.validate().unwrap().bind(&registry).unwrap(),
                    NonZeroUsize::new(held).unwrap(),
                )
                .unwrap();
            let hot_task = tokio::spawn(hot.execute());
            wait_started(&hot_started, held).await;

            let cold_plan_id = format!("cold-{workload}-{profile}-{round}");
            let mut cold_node = OperationNode::new("cold", key("cold"));
            if !capacity_only {
                cold_node = cold_node.access("shared", AccessMode::Write);
            }
            let cold = app
                .execution_graph(
                    ExecutionPlanDraft::new(cold_plan_id.clone(), 1)
                        .operation(cold_node)
                        .validate()
                        .unwrap()
                        .bind(&registry)
                        .unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            let observer = cold.observer();
            let cold_task = tokio::spawn(cold.execute());
            running(&observer).await;
            assert_eq!(cold_callback_rank.load(Ordering::SeqCst), 0);

            release.add_permits(held);
            let hot_report = tokio::time::timeout(Duration::from_secs(10), hot_task)
                .await
                .unwrap()
                .unwrap();
            let cold_report = tokio::time::timeout(Duration::from_secs(10), cold_task)
                .await
                .unwrap()
                .unwrap();
            assert!(hot_report.succeeded());
            assert!(cold_report.succeeded());
            assert_eq!(hot_report.started, HOT_COUNT);
            assert_eq!(cold_report.started, 1);

            // Source-owned proof: exactly one event per successful control
            // admission, ordered within this session, not by callback poll.
            let (events, truncated) = app.research_graph_admissions();
            assert!(!truncated);
            assert_eq!(events.len(), HOT_COUNT + 1);
            for (index, (sequence, identity, _, attempt)) in events.iter().enumerate() {
                assert_eq!(*sequence, (index + 1) as u64);
                assert_eq!(identity.session_generation(), hot_report.identity.session_generation());
                assert_eq!(*attempt, 1);
            }
            let cold_events = events
                .iter()
                .filter(|(_, identity, op, _)| {
                    identity.plan_id() == cold_plan_id.as_str() && op == "cold"
                })
                .map(|(sequence, _, _, _)| *sequence)
                .collect::<Vec<_>>();
            assert_eq!(cold_events.len(), 1);
            let admission_rank = cold_events[0] as usize;
            assert!(((held + 1)..=(HOT_COUNT + 1)).contains(&admission_rank));
            eprintln!(
                "research #226 admission_witness workload={workload} mode={profile} round={round} admission_rank={admission_rank} callback_rank={} of {}",
                cold_callback_rank.load(Ordering::SeqCst),
                HOT_COUNT + 1
            );
        }
    });
}

#[test]
fn read_writer_actual_admission_current_thread() {
    verify_session_admissions(false, false);
}

#[test]
fn read_writer_actual_admission_four_workers() {
    verify_session_admissions(true, false);
}

#[test]
fn capacity_only_actual_admission_current_thread() {
    verify_session_admissions(false, true);
}

#[test]
fn capacity_only_actual_admission_four_workers() {
    verify_session_admissions(true, true);
}

#[test]
fn paused_safe_point_does_not_generate_successful_admission() {
    runtime(false).block_on(async {
        let app = owner();
        let callback_count = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("single"),
                async_operation({
                    let callback_count = Arc::clone(&callback_count);
                    move || {
                        let callback_count = Arc::clone(&callback_count);
                        async move {
                            callback_count.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let graph = app
            .execution_graph(
                ExecutionPlanDraft::new("paused-witness", 1)
                    .operation(OperationNode::new("single", key("single")))
                    .validate()
                    .unwrap()
                    .bind(&registry)
                    .unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        let controller = graph.controller();
        controller.pause(GraphControlTarget::Run).unwrap();
        let observer = graph.observer();
        let graph_task = tokio::spawn(graph.execute());
        running(&observer).await;
        assert_eq!(callback_count.load(Ordering::SeqCst), 0);
        assert!(app.research_graph_admissions().0.is_empty());

        controller.step(GraphControlTarget::Run).unwrap();
        let report = tokio::time::timeout(Duration::from_secs(5), graph_task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(callback_count.load(Ordering::SeqCst), 1);
        let (events, truncated) = app.research_graph_admissions();
        assert!(!truncated);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, 1);
        assert_eq!(events[0].1, report.identity);
        assert_eq!(events[0].2, "single");
        assert_eq!(events[0].3, 1);
    });
}
