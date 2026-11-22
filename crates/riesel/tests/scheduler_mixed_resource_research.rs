//! Research #226: mixed resource, waiter lifecycle and control behavior.
//! Actual public RunningApp graph runner; no scheduling policy is accepted.
#![cfg(feature = "async-tcp")]

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
        r#"{"version":1,"app":{"name":"MixedResourceResearch"},"models":{},"requires":[]}"#,
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

async fn wait_for_one(counter: &AtomicUsize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while counter.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn pending_multi_key_writer_does_not_hold_unavailable_or_unrelated_resources() {
    runtime().block_on(async {
        let application = owner();
        let held_started = Arc::new(AtomicUsize::new(0));
        let both_started = Arc::new(AtomicUsize::new(0));
        let beta_started = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Semaphore::new(0));
        let mut handlers = OperationRegistry::<AsyncOperation>::new();
        handlers
            .register(
                key("held"),
                async_operation({
                    let held_started = Arc::clone(&held_started);
                    let release = Arc::clone(&release);
                    move || {
                        let held_started = Arc::clone(&held_started);
                        let release = Arc::clone(&release);
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
        handlers
            .register(
                key("both"),
                async_operation({
                    let both_started = Arc::clone(&both_started);
                    move || {
                        let both_started = Arc::clone(&both_started);
                        async move {
                            both_started.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        handlers
            .register(
                key("beta"),
                async_operation({
                    let beta_started = Arc::clone(&beta_started);
                    move || {
                        let beta_started = Arc::clone(&beta_started);
                        async move {
                            beta_started.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();

        let held = application
            .execution_graph(
                ExecutionPlanDraft::new("held-alpha", 1)
                    .operation(
                        OperationNode::new("held", key("held")).access("alpha", AccessMode::Write),
                    )
                    .validate()
                    .unwrap()
                    .bind(&handlers)
                    .unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        let held_task = tokio::spawn(held.execute());
        wait_for_one(&held_started).await;

        let multi = application
            .execution_graph(
                ExecutionPlanDraft::new("waiting-alpha-beta", 1)
                    .operation(
                        OperationNode::new("both", key("both"))
                            .access("alpha", AccessMode::Write)
                            .access("beta", AccessMode::Write),
                    )
                    .validate()
                    .unwrap()
                    .bind(&handlers)
                    .unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        let observer = multi.observer();
        let multi_task = tokio::spawn(multi.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while observer.snapshot().phase != GraphRunPhase::Running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(both_started.load(Ordering::SeqCst), 0);

        // The denied {alpha,beta} request cannot reserve beta in advance.
        let beta = application
            .execution_graph(
                ExecutionPlanDraft::new("independent-beta", 1)
                    .operation(
                        OperationNode::new("beta", key("beta")).access("beta", AccessMode::Write),
                    )
                    .validate()
                    .unwrap()
                    .bind(&handlers)
                    .unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        let beta_report = tokio::time::timeout(Duration::from_secs(5), beta.execute())
            .await
            .unwrap();
        assert!(beta_report.succeeded());
        assert_eq!(beta_started.load(Ordering::SeqCst), 1);
        assert_eq!(both_started.load(Ordering::SeqCst), 0);

        release.add_permits(1);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), held_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        let multi_report = tokio::time::timeout(Duration::from_secs(5), multi_task)
            .await
            .unwrap()
            .unwrap();
        assert!(multi_report.succeeded());
        assert_eq!(both_started.load(Ordering::SeqCst), 1);
        eprintln!(
            "research #226 multi_key_waiter=blocked beta_independent=completed before_alpha_release"
        );
    });
}

#[test]
fn queued_writer_does_not_prevent_compatible_readers() {
    runtime().block_on(async {
        let application = owner();
        let read_started = Arc::new(AtomicUsize::new(0));
        let writer_started = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Semaphore::new(0));
        let mut handlers = OperationRegistry::<AsyncOperation>::new();
        handlers
            .register(
                key("held-read"),
                async_operation({
                    let read_started = Arc::clone(&read_started);
                    let release = Arc::clone(&release);
                    move || {
                        let read_started = Arc::clone(&read_started);
                        let release = Arc::clone(&release);
                        async move {
                            read_started.fetch_add(1, Ordering::SeqCst);
                            let permit = release.acquire().await.unwrap();
                            permit.forget();
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        handlers
            .register(
                key("reader"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        handlers
            .register(
                key("writer"),
                async_operation({
                    let writer_started = Arc::clone(&writer_started);
                    move || {
                        let writer_started = Arc::clone(&writer_started);
                        async move {
                            writer_started.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();

        let first = application
            .execution_graph(
                ExecutionPlanDraft::new("held-reader", 1)
                    .operation(
                        OperationNode::new("held", key("held-read"))
                            .access("shared", AccessMode::Read),
                    )
                    .validate()
                    .unwrap()
                    .bind(&handlers)
                    .unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        let first_task = tokio::spawn(first.execute());
        wait_for_one(&read_started).await;

        let waiting = application
            .execution_graph(
                ExecutionPlanDraft::new("waiting-writer", 1)
                    .operation(
                        OperationNode::new("writer", key("writer"))
                            .access("shared", AccessMode::Write),
                    )
                    .validate()
                    .unwrap()
                    .bind(&handlers)
                    .unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        let observer = waiting.observer();
        let writer_task = tokio::spawn(waiting.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while observer.snapshot().phase != GraphRunPhase::Running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(writer_started.load(Ordering::SeqCst), 0);

        let reader = application
            .execution_graph(
                ExecutionPlanDraft::new("additional-reader", 1)
                    .operation(
                        OperationNode::new("reader", key("reader"))
                            .access("shared", AccessMode::Read),
                    )
                    .validate()
                    .unwrap()
                    .bind(&handlers)
                    .unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), reader.execute())
                .await
                .unwrap()
                .succeeded()
        );
        assert_eq!(writer_started.load(Ordering::SeqCst), 0);
        release.add_permits(1);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), first_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(5), writer_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        assert_eq!(writer_started.load(Ordering::SeqCst), 1);
        eprintln!("research #226 read_read=compatible writer_waited=1 (not a starvation proof)");
    });
}

#[test]
fn paused_cross_run_waiter_does_not_claim_released_resource_until_step() {
    runtime().block_on(async {
        let application = owner();
        let held_started = Arc::new(AtomicUsize::new(0));
        let waiter_started = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(Semaphore::new(0));
        let mut handlers = OperationRegistry::<AsyncOperation>::new();
        handlers
            .register(
                key("held"),
                async_operation({
                    let held_started = Arc::clone(&held_started);
                    let release = Arc::clone(&release);
                    move || {
                        let held_started = Arc::clone(&held_started);
                        let release = Arc::clone(&release);
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
        handlers
            .register(
                key("waiter"),
                async_operation({
                    let waiter_started = Arc::clone(&waiter_started);
                    move || {
                        let waiter_started = Arc::clone(&waiter_started);
                        async move {
                            waiter_started.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();

        let first = application
            .execution_graph(
                ExecutionPlanDraft::new("held", 1)
                    .operation(
                        OperationNode::new("held", key("held")).access("shared", AccessMode::Write),
                    )
                    .validate()
                    .unwrap()
                    .bind(&handlers)
                    .unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        let first_task = tokio::spawn(first.execute());
        wait_for_one(&held_started).await;

        let waiter = application
            .execution_graph(
                ExecutionPlanDraft::new("paused-waiter", 1)
                    .operation(
                        OperationNode::new("waiter", key("waiter"))
                            .access("shared", AccessMode::Write),
                    )
                    .validate()
                    .unwrap()
                    .bind(&handlers)
                    .unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        let observer = waiter.observer();
        let controller = waiter.controller();
        let waiter_task = tokio::spawn(waiter.execute());
        tokio::time::timeout(Duration::from_secs(5), async {
            while observer.snapshot().phase != GraphRunPhase::Running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        controller.pause(GraphControlTarget::Run).unwrap();
        release.add_permits(1);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), first_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
        assert_eq!(waiter_started.load(Ordering::SeqCst), 0);
        assert!(!waiter_task.is_finished());
        controller.step(GraphControlTarget::Run).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(5), waiter_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        assert_eq!(waiter_started.load(Ordering::SeqCst), 1);
        eprintln!("research #226 paused_waiter=0_after_release step_then_admits=1");
    });
}

#[test]
fn rolling_short_runs_compete_with_existing_cold_waiter_without_service_guarantee() {
    runtime().block_on(async {
        const ROUNDS: usize = 6;
        const ROLLING_RUNS: usize = 32;
        for round in 0..ROUNDS {
            let application = owner();
            let permits = Arc::new(Semaphore::new(0));
            let hot_started = Arc::new(AtomicUsize::new(0));
            let event_order = Arc::new(AtomicUsize::new(0));
            let cold_rank = Arc::new(AtomicUsize::new(0));
            let mut handlers = OperationRegistry::<AsyncOperation>::new();
            handlers
                .register(
                    key("hot"),
                    async_operation({
                        let permits = Arc::clone(&permits);
                        let hot_started = Arc::clone(&hot_started);
                        let event_order = Arc::clone(&event_order);
                        move || {
                            let permits = Arc::clone(&permits);
                            let hot_started = Arc::clone(&hot_started);
                            let event_order = Arc::clone(&event_order);
                            async move {
                                event_order.fetch_add(1, Ordering::SeqCst);
                                let n = hot_started.fetch_add(1, Ordering::SeqCst);
                                if n == 0 {
                                    let permit = permits.acquire().await.unwrap();
                                    permit.forget();
                                }
                                OperationOutcome::Completed
                            }
                        }
                    }),
                )
                .unwrap();
            handlers
                .register(
                    key("cold"),
                    async_operation({
                        let event_order = Arc::clone(&event_order);
                        let cold_rank = Arc::clone(&cold_rank);
                        move || {
                            let event_order = Arc::clone(&event_order);
                            let cold_rank = Arc::clone(&cold_rank);
                            async move {
                                cold_rank.store(
                                    event_order.fetch_add(1, Ordering::SeqCst) + 1,
                                    Ordering::SeqCst,
                                );
                                OperationOutcome::Completed
                            }
                        }
                    }),
                )
                .unwrap();
            let hot_plan = ExecutionPlanDraft::new("rolling-hot", 1)
                .operation(
                    OperationNode::new("hot", key("hot")).access("shared", AccessMode::Write),
                )
                .validate()
                .unwrap()
                .bind(&handlers)
                .unwrap();
            let cold_plan = ExecutionPlanDraft::new("rolling-cold", 1)
                .operation(
                    OperationNode::new("cold", key("cold")).access("shared", AccessMode::Write),
                )
                .validate()
                .unwrap()
                .bind(&handlers)
                .unwrap();
            let first = application
                .execution_graph(hot_plan.clone(), NonZeroUsize::new(1).unwrap())
                .unwrap();
            let first_task = tokio::spawn(first.execute());
            wait_for_one(&hot_started).await;
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

            // Reuse the same immutable plan, but create and finish each hot
            // graph run separately. This is not one 33-node hot plan and
            // makes no assumptions about fairness between run boundaries.
            let submitted = AtomicUsize::new(0);
            let rolling_producer = async {
                for _ in 0..ROLLING_RUNS {
                    let runner = application
                        .execution_graph(hot_plan.clone(), NonZeroUsize::new(1).unwrap())
                        .unwrap();
                    submitted.fetch_add(1, Ordering::SeqCst);
                    let report = tokio::time::timeout(Duration::from_secs(5), runner.execute())
                        .await
                        .unwrap();
                    assert!(report.succeeded());
                    assert_eq!(report.started, 1);
                }
            };
            let unblock_and_join = async {
                wait_for_one(&submitted).await;
                permits.add_permits(1);
                assert!(
                    tokio::time::timeout(Duration::from_secs(5), first_task)
                        .await
                        .unwrap()
                        .unwrap()
                        .succeeded()
                );
                assert!(
                    tokio::time::timeout(Duration::from_secs(5), cold_task)
                        .await
                        .unwrap()
                        .unwrap()
                        .succeeded()
                );
            };
            let (_, ()) = tokio::join!(rolling_producer, unblock_and_join);
            assert_eq!(hot_started.load(Ordering::SeqCst), ROLLING_RUNS + 1);
            assert_eq!(event_order.load(Ordering::SeqCst), ROLLING_RUNS + 2);
            let rank = cold_rank.load(Ordering::SeqCst);
            assert!((2..=ROLLING_RUNS + 2).contains(&rank));
            eprintln!(
                "research #226 rolling_round={round} separate_hot_runs={ROLLING_RUNS} cold_rank={rank} of {} (observed only)",
                ROLLING_RUNS + 2
            );
        }
    });
}
