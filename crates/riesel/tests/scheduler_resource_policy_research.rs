//! Riesel #226 cross-run resource admission research.
//! Research-only: do not use results to claim scheduler fairness or QoS.
#![cfg(feature = "async-tcp")]

use riesel::{
    AccessMode, App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft,
    GraphRunPhase, OperationKey, OperationNode, OperationOutcome, OperationRegistry, RunningApp,
    async_operation,
};
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

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
        r#"{"version":1,"app":{"name":"ResourcePolicyResearch"},"models":{},"requires":[]}"#,
    )
    .unwrap()
    .start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
    .unwrap()
}

#[test]
fn contending_runs_retain_independent_progress_without_a_fairness_claim() {
    runtime().block_on(async {
        const ROUNDS: usize = 8;
        const HOT_WRITERS: usize = 64;
        for round in 0..ROUNDS {
            let application = owner();
            let order = Arc::new(AtomicUsize::new(0));
            let hot_started = Arc::new(AtomicUsize::new(0));
            let cold_rank = Arc::new(AtomicUsize::new(0));
            let permits = Arc::new(Semaphore::new(0));
            let mut registry = OperationRegistry::<AsyncOperation>::new();
            registry
                .register(
                    key("hot"),
                    async_operation({
                        let order = Arc::clone(&order);
                        let hot_started = Arc::clone(&hot_started);
                        let permits = Arc::clone(&permits);
                        move || {
                            let order = Arc::clone(&order);
                            let hot_started = Arc::clone(&hot_started);
                            let permits = Arc::clone(&permits);
                            async move {
                                order.fetch_add(1, Ordering::SeqCst);
                                hot_started.fetch_add(1, Ordering::SeqCst);
                                let permit = permits.acquire().await.unwrap();
                                permit.forget();
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
                        let order = Arc::clone(&order);
                        let cold_rank = Arc::clone(&cold_rank);
                        move || {
                            let order = Arc::clone(&order);
                            let cold_rank = Arc::clone(&cold_rank);
                            async move {
                                cold_rank.store(order.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                                OperationOutcome::Completed
                            }
                        }
                    }),
                )
                .unwrap();
            registry
                .register(
                    key("independent"),
                    async_operation(|| async { OperationOutcome::Completed }),
                )
                .unwrap();

            let mut hot_plan = ExecutionPlanDraft::new(format!("hot-{round}"), 1);
            for index in 0..HOT_WRITERS {
                hot_plan = hot_plan.operation(
                    OperationNode::new(format!("op-{index}"), key("hot"))
                        .access("contested", AccessMode::Write),
                );
            }
            let hot = application
                .execution_graph(
                    hot_plan.validate().unwrap().bind(&registry).unwrap(),
                    NonZeroUsize::new(32).unwrap(),
                )
                .unwrap();
            let hot_task = tokio::spawn(hot.execute());
            tokio::time::timeout(Duration::from_secs(5), async {
                while hot_started.load(Ordering::SeqCst) != 1 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();

            let cold = application
                .execution_graph(
                    ExecutionPlanDraft::new(format!("cold-{round}"), 1)
                        .operation(
                            OperationNode::new("cold", key("cold"))
                                .access("contested", AccessMode::Write),
                        )
                        .validate()
                        .unwrap()
                        .bind(&registry)
                        .unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            let cold_observer = cold.observer();
            let cold_task = tokio::spawn(cold.execute());
            tokio::time::timeout(Duration::from_secs(5), async {
                while cold_observer.snapshot().phase != GraphRunPhase::Running {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert_eq!(cold_rank.load(Ordering::SeqCst), 0);

            // A cold writer waiting on "contested" must not block unrelated
            // work that has a different resource key within the same session.
            let independent = application
                .execution_graph(
                    ExecutionPlanDraft::new(format!("independent-{round}"), 1)
                        .operation(
                            OperationNode::new("independent", key("independent"))
                                .access("unrelated", AccessMode::Write),
                        )
                        .validate()
                        .unwrap()
                        .bind(&registry)
                        .unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            assert!(
                tokio::time::timeout(Duration::from_secs(5), independent.execute())
                    .await
                    .unwrap()
                    .succeeded()
            );
            assert_eq!(cold_rank.load(Ordering::SeqCst), 0);
            assert_eq!(hot_started.load(Ordering::SeqCst), 1);

            let after_release = Instant::now();
            permits.add_permits(HOT_WRITERS);
            let hot_result = tokio::time::timeout(Duration::from_secs(10), hot_task)
                .await
                .unwrap()
                .unwrap();
            let cold_result = tokio::time::timeout(Duration::from_secs(10), cold_task)
                .await
                .unwrap()
                .unwrap();
            assert!(hot_result.succeeded());
            assert!(cold_result.succeeded());
            assert_eq!(hot_result.started, HOT_WRITERS);
            assert_eq!(cold_result.started, 1);
            let rank = cold_rank.load(Ordering::SeqCst);
            assert!((2..=HOT_WRITERS + 1).contains(&rank));
            eprintln!(
                "research #226 round={round} hot={HOT_WRITERS} cold_rank={rank} of {} post_release_us={} (observed only, not fairness)",
                HOT_WRITERS + 1,
                after_release.elapsed().as_micros()
            );
        }
    });
}
