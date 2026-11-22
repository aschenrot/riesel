//! Research #226: bounded replenishing cross-run resource competition.
//! Public SDK fixture only; no production fairness or scheduling API implied.
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
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

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
        r#"{"version":1,"app":{"name":"ContinuousProducerResearch"},"models":{},"requires":[]}"#,
    )
    .unwrap()
    .start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
    .unwrap()
}

#[test]
fn bounded_replenishing_hot_runs_measure_cold_service_rank() {
    runtime().block_on(async {
        const TOTAL_HOT: usize = 96;
        const ROUNDS: usize = 4;
        for backlog in [1usize, 4, 8] {
            for round in 0..ROUNDS {
                let application = owner();
                let release = Arc::new(Semaphore::new(0));
                let order = Arc::new(AtomicUsize::new(0));
                let hot_started = Arc::new(AtomicUsize::new(0));
                let cold_rank = Arc::new(AtomicUsize::new(0));
                let mut registry = OperationRegistry::<AsyncOperation>::new();
                registry
                    .register(
                        key("hot"),
                        async_operation({
                            let release = Arc::clone(&release);
                            let order = Arc::clone(&order);
                            let hot_started = Arc::clone(&hot_started);
                            move || {
                                let release = Arc::clone(&release);
                                let order = Arc::clone(&order);
                                let hot_started = Arc::clone(&hot_started);
                                async move {
                                    order.fetch_add(1, Ordering::SeqCst);
                                    if hot_started.fetch_add(1, Ordering::SeqCst) == 0 {
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
                            let order = Arc::clone(&order);
                            let cold_rank = Arc::clone(&cold_rank);
                            move || {
                                let order = Arc::clone(&order);
                                let cold_rank = Arc::clone(&cold_rank);
                                async move {
                                    cold_rank.store(
                                        order.fetch_add(1, Ordering::SeqCst) + 1,
                                        Ordering::SeqCst,
                                    );
                                    OperationOutcome::Completed
                                }
                            }
                        }),
                    )
                    .unwrap();

                let hot_plan = ExecutionPlanDraft::new("hot-replenishing", 1)
                    .operation(
                        OperationNode::new("hot", key("hot")).access("contested", AccessMode::Write),
                    )
                    .validate()
                    .unwrap()
                    .bind(&registry)
                    .unwrap();
                let cold_plan = ExecutionPlanDraft::new("cold-waiting", 1)
                    .operation(
                        OperationNode::new("cold", key("cold")).access("contested", AccessMode::Write),
                    )
                    .validate()
                    .unwrap()
                    .bind(&registry)
                    .unwrap();

                let first = application
                    .execution_graph(hot_plan.clone(), NonZeroUsize::new(1).unwrap())
                    .unwrap();
                let mut hot_jobs = JoinSet::new();
                hot_jobs.spawn(first.execute());
                tokio::time::timeout(Duration::from_secs(5), async {
                    while hot_started.load(Ordering::SeqCst) == 0 {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();

                let cold = application
                    .execution_graph(cold_plan, NonZeroUsize::new(1).unwrap())
                    .unwrap();
                let observer = cold.observer();
                let cold_task = tokio::spawn(cold.execute());
                tokio::time::timeout(Duration::from_secs(5), async {
                    while observer.snapshot().phase != GraphRunPhase::Running {
                        tokio::task::yield_now().await;
                    }
                })
                .await
                .unwrap();
                assert_eq!(cold_rank.load(Ordering::SeqCst), 0);

                // Maintain at most 'backlog' additional independent hot runs
                // in this session. Every replacement is a distinct one-shot
                // graph run sharing the same immutable bound plan.
                for _ in 0..backlog {
                    let runner = application
                        .execution_graph(hot_plan.clone(), NonZeroUsize::new(1).unwrap())
                        .unwrap();
                    hot_jobs.spawn(runner.execute());
                }
                let mut submitted = backlog + 1;
                release.add_permits(1);
                for _ in 0..TOTAL_HOT {
                    let report = tokio::time::timeout(Duration::from_secs(10), hot_jobs.join_next())
                        .await
                        .unwrap()
                        .unwrap()
                        .unwrap();
                    assert!(report.succeeded());
                    assert_eq!(report.started, 1);
                    if submitted < TOTAL_HOT {
                        let runner = application
                            .execution_graph(hot_plan.clone(), NonZeroUsize::new(1).unwrap())
                            .unwrap();
                        hot_jobs.spawn(runner.execute());
                        submitted += 1;
                    }
                }
                let cold_report = tokio::time::timeout(Duration::from_secs(10), cold_task)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(cold_report.succeeded());
                assert_eq!(cold_report.started, 1);
                assert_eq!(submitted, TOTAL_HOT);
                assert_eq!(hot_started.load(Ordering::SeqCst), TOTAL_HOT);
                assert_eq!(order.load(Ordering::SeqCst), TOTAL_HOT + 1);
                let rank = cold_rank.load(Ordering::SeqCst);
                assert!((2..=TOTAL_HOT + 1).contains(&rank));
                eprintln!(
                    "research #226 rolling_overlap backlog={backlog} round={round} hot={TOTAL_HOT} cold_rank={rank} of {}",
                    TOTAL_HOT + 1
                );
            }
        }
    });
}

#[test]
fn abandoning_pending_cold_run_releases_session_slot_without_dispatch() {
    runtime().block_on(async {
        let application = owner();
        let held_started = Arc::new(AtomicUsize::new(0));
        let cancelled_started = Arc::new(AtomicUsize::new(0));
        let next_started = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Semaphore::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("held"),
                async_operation({
                    let release = Arc::clone(&release);
                    let held_started = Arc::clone(&held_started);
                    move || {
                        let release = Arc::clone(&release);
                        let held_started = Arc::clone(&held_started);
                        async move {
                            held_started.fetch_add(1, Ordering::SeqCst);
                            let permit = release.acquire().await.unwrap();
                            permit.forget();
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        for (name, count) in [
            ("cancelled", Arc::clone(&cancelled_started)),
            ("next", Arc::clone(&next_started)),
        ] {
            registry
                .register(
                    key(name),
                    async_operation(move || {
                        let count = Arc::clone(&count);
                        async move {
                            count.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }),
                )
                .unwrap();
        }
        let writer = |name: &str| {
            ExecutionPlanDraft::new(name, 1)
                .operation(
                    OperationNode::new(name, key(name)).access("shared", AccessMode::Write),
                )
                .validate()
                .unwrap()
                .bind(&registry)
                .unwrap()
        };
        let first = application
            .execution_graph(writer("held"), NonZeroUsize::new(1).unwrap())
            .unwrap();
        let first_task = tokio::spawn(first.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while held_started.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let cancelled = application
            .execution_graph(writer("cancelled"), NonZeroUsize::new(1).unwrap())
            .unwrap();
        let observer = cancelled.observer();
        let task = tokio::spawn(cancelled.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while observer.snapshot().phase != GraphRunPhase::Running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(cancelled_started.load(Ordering::SeqCst), 0);
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());

        release.add_permits(1);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), first_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        let next = application
            .execution_graph(writer("next"), NonZeroUsize::new(1).unwrap())
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), next.execute())
                .await
                .unwrap()
                .succeeded()
        );
        assert_eq!(cancelled_started.load(Ordering::SeqCst), 0);
        assert_eq!(next_started.load(Ordering::SeqCst), 1);
        eprintln!("research #226 aborted_waiter=never_dispatched next_session_run=completed");
    });
}
