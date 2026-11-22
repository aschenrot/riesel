//! Specification #226: real-runner reader/writer service-order investigation.
//! Only an observed finite scheduling experiment; never a fairness contract.
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
        r#"{"version":1,"app":{"name":"ReaderWriterFairnessResearch"},"models":{},"requires":[]}"#,
    )
    .unwrap()
    .start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
    .unwrap()
}

async fn has_started(counter: &AtomicUsize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        while counter.load(Ordering::SeqCst) == 0 {
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

#[test]
fn long_reader_run_vs_waiting_writer_records_actual_callback_rank() {
    runtime().block_on(async {
        const READERS: usize = 64;
        const ROUNDS: usize = 8;
        for round in 0..ROUNDS {
            let app = owner();
            let first_release = Arc::new(Semaphore::new(0));
            let started_readers = Arc::new(AtomicUsize::new(0));
            let callbacks = Arc::new(AtomicUsize::new(0));
            let writer_rank = Arc::new(AtomicUsize::new(0));
            let mut registry = OperationRegistry::<AsyncOperation>::new();
            registry
                .register(
                    key("reader"),
                    async_operation({
                        let first_release = Arc::clone(&first_release);
                        let started_readers = Arc::clone(&started_readers);
                        let callbacks = Arc::clone(&callbacks);
                        move || {
                            let first_release = Arc::clone(&first_release);
                            let started_readers = Arc::clone(&started_readers);
                            let callbacks = Arc::clone(&callbacks);
                            async move {
                                callbacks.fetch_add(1, Ordering::SeqCst);
                                if started_readers.fetch_add(1, Ordering::SeqCst) == 0 {
                                    let permit = first_release.acquire().await.unwrap();
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
                    key("writer"),
                    async_operation({
                        let callbacks = Arc::clone(&callbacks);
                        let writer_rank = Arc::clone(&writer_rank);
                        move || {
                            let callbacks = Arc::clone(&callbacks);
                            let writer_rank = Arc::clone(&writer_rank);
                            async move {
                                writer_rank.store(
                                    callbacks.fetch_add(1, Ordering::SeqCst) + 1,
                                    Ordering::SeqCst,
                                );
                                OperationOutcome::Completed
                            }
                        }
                    }),
                )
                .unwrap();
            registry
                .register(
                    key("unrelated"),
                    async_operation(|| async { OperationOutcome::Completed }),
                )
                .unwrap();
            let mut draft = ExecutionPlanDraft::new(format!("reader-plan-{round}"), 1);
            for index in 0..READERS {
                draft = draft.operation(
                    OperationNode::new(format!("read-{index}"), key("reader"))
                        .access("shared", AccessMode::Read),
                );
            }
            let hot = app
                .execution_graph(
                    draft.validate().unwrap().bind(&registry).unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            let hot_task = tokio::spawn(hot.execute());
            has_started(&started_readers).await;
            let writer = app
                .execution_graph(
                    ExecutionPlanDraft::new(format!("waiting-writer-{round}"), 1)
                        .operation(
                            OperationNode::new("write", key("writer"))
                                .access("shared", AccessMode::Write),
                        )
                        .validate()
                        .unwrap()
                        .bind(&registry)
                        .unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            let observer = writer.observer();
            let writer_task = tokio::spawn(writer.execute());
            running(&observer).await;
            assert_eq!(writer_rank.load(Ordering::SeqCst), 0);
            let unrelated = app
                .execution_graph(
                    ExecutionPlanDraft::new(format!("unrelated-{round}"), 1)
                        .operation(
                            OperationNode::new("unrelated", key("unrelated"))
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
                tokio::time::timeout(Duration::from_secs(5), unrelated.execute())
                    .await
                    .unwrap()
                    .succeeded()
            );
            assert_eq!(writer_rank.load(Ordering::SeqCst), 0);
            assert_eq!(started_readers.load(Ordering::SeqCst), 1);

            first_release.add_permits(1);
            let hot_report = tokio::time::timeout(Duration::from_secs(10), hot_task)
                .await
                .unwrap()
                .unwrap();
            let writer_report = tokio::time::timeout(Duration::from_secs(10), writer_task)
                .await
                .unwrap()
                .unwrap();
            assert!(hot_report.succeeded());
            assert!(writer_report.succeeded());
            assert_eq!(hot_report.started, READERS);
            assert_eq!(writer_report.started, 1);
            assert_eq!(started_readers.load(Ordering::SeqCst), READERS);
            assert_eq!(callbacks.load(Ordering::SeqCst), READERS + 1);
            let rank = writer_rank.load(Ordering::SeqCst);
            assert!((2..=READERS + 1).contains(&rank));
            eprintln!(
                "research #226 reader_plan round={round} readers={READERS} writer_rank={rank} of {}",
                READERS + 1
            );
        }
    });
}

#[test]
fn later_compatible_reader_runs_can_overtake_an_existing_waiting_writer() {
    runtime().block_on(async {
        const LATER_READERS: usize = 8;
        const ROUNDS: usize = 8;
        for round in 0..ROUNDS {
            let app = owner();
            let first_release = Arc::new(Semaphore::new(0));
            let first_started = Arc::new(AtomicUsize::new(0));
            let all_reads = Arc::new(AtomicUsize::new(0));
            let writer_started = Arc::new(AtomicUsize::new(0));
            let mut registry = OperationRegistry::<AsyncOperation>::new();
            registry
                .register(
                    key("held"),
                    async_operation({
                        let first_release = Arc::clone(&first_release);
                        let first_started = Arc::clone(&first_started);
                        let all_reads = Arc::clone(&all_reads);
                        move || {
                            let first_release = Arc::clone(&first_release);
                            let first_started = Arc::clone(&first_started);
                            let all_reads = Arc::clone(&all_reads);
                            async move {
                                all_reads.fetch_add(1, Ordering::SeqCst);
                                first_started.fetch_add(1, Ordering::SeqCst);
                                let permit = first_release.acquire().await.unwrap();
                                permit.forget();
                                OperationOutcome::Completed
                            }
                        }
                    }),
                )
                .unwrap();
            registry
                .register(
                    key("reader"),
                    async_operation({
                        let all_reads = Arc::clone(&all_reads);
                        move || {
                            let all_reads = Arc::clone(&all_reads);
                            async move {
                                all_reads.fetch_add(1, Ordering::SeqCst);
                                OperationOutcome::Completed
                            }
                        }
                    }),
                )
                .unwrap();
            registry
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
            let first = app
                .execution_graph(
                    ExecutionPlanDraft::new(format!("held-reader-{round}"), 1)
                        .operation(
                            OperationNode::new("held", key("held"))
                                .access("shared", AccessMode::Read),
                        )
                        .validate()
                        .unwrap()
                        .bind(&registry)
                        .unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            let held_task = tokio::spawn(first.execute());
            has_started(&first_started).await;
            let writer = app
                .execution_graph(
                    ExecutionPlanDraft::new(format!("waiting-writer-{round}"), 1)
                        .operation(
                            OperationNode::new("writer", key("writer"))
                                .access("shared", AccessMode::Write),
                        )
                        .validate()
                        .unwrap()
                        .bind(&registry)
                        .unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            let observer = writer.observer();
            let writer_task = tokio::spawn(writer.execute());
            running(&observer).await;
            assert_eq!(writer_started.load(Ordering::SeqCst), 0);

            // New readers are compatible with the already-held read lease.
            // All eight can finish while the cold writer remains pending.
            for index in 0..LATER_READERS {
                let reader = app
                    .execution_graph(
                        ExecutionPlanDraft::new(format!("later-reader-{round}-{index}"), 1)
                            .operation(
                                OperationNode::new("read", key("reader"))
                                    .access("shared", AccessMode::Read),
                            )
                            .validate()
                            .unwrap()
                            .bind(&registry)
                            .unwrap(),
                        NonZeroUsize::new(1).unwrap(),
                    )
                    .unwrap();
                let report = tokio::time::timeout(Duration::from_secs(5), reader.execute())
                    .await
                    .unwrap();
                assert!(report.succeeded());
                assert_eq!(writer_started.load(Ordering::SeqCst), 0);
            }
            assert_eq!(all_reads.load(Ordering::SeqCst), LATER_READERS + 1);
            first_release.add_permits(1);
            assert!(
                tokio::time::timeout(Duration::from_secs(5), held_task)
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
            eprintln!(
                "research #226 later_readers round={round} overtook_writer={LATER_READERS} all_finished_before_held_release"
            );
        }
    });
}

#[test]
fn overlapping_read_handoffs_keep_writer_ungrantable_until_last_release() {
    runtime().block_on(async {
        const OVERLAPS: usize = 8;
        const ROUNDS: usize = 4;
        for round in 0..ROUNDS {
            let app = owner();
            let permits = (0..=OVERLAPS)
                .map(|_| Arc::new(Semaphore::new(0)))
                .collect::<Vec<_>>();
            let started = (0..=OVERLAPS)
                .map(|_| Arc::new(AtomicUsize::new(0)))
                .collect::<Vec<_>>();
            let writer_started = Arc::new(AtomicUsize::new(0));
            let mut registry = OperationRegistry::<AsyncOperation>::new();
            for i in 0..=OVERLAPS {
                registry
                    .register(
                        key(&format!("reader-{i}")),
                        async_operation({
                            let permit = Arc::clone(&permits[i]);
                            let started = Arc::clone(&started[i]);
                            move || {
                                let permit = Arc::clone(&permit);
                                let started = Arc::clone(&started);
                                async move {
                                    started.fetch_add(1, Ordering::SeqCst);
                                    let lease = permit.acquire().await.unwrap();
                                    lease.forget();
                                    OperationOutcome::Completed
                                }
                            }
                        }),
                    )
                    .unwrap();
            }
            registry
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

            let build_reader = |i: usize| {
                let plan = ExecutionPlanDraft::new(format!("overlap-read-{round}-{i}"), 1)
                    .operation(
                        OperationNode::new("read", key(&format!("reader-{i}")))
                            .access("shared", AccessMode::Read),
                    )
                    .validate()
                    .unwrap()
                    .bind(&registry)
                    .unwrap();
                app.execution_graph(plan, NonZeroUsize::new(1).unwrap())
                    .unwrap()
            };
            let mut previous_task = tokio::spawn(build_reader(0).execute());
            has_started(&started[0]).await;
            let writer = app
                .execution_graph(
                    ExecutionPlanDraft::new(format!("overlap-write-{round}"), 1)
                        .operation(
                            OperationNode::new("write", key("writer"))
                                .access("shared", AccessMode::Write),
                        )
                        .validate()
                        .unwrap()
                        .bind(&registry)
                        .unwrap(),
                    NonZeroUsize::new(1).unwrap(),
                )
                .unwrap();
            let observer = writer.observer();
            let writer_task = tokio::spawn(writer.execute());
            running(&observer).await;
            assert_eq!(writer_started.load(Ordering::SeqCst), 0);

            for i in 1..=OVERLAPS {
                // The next reader acquires a compatible read lease *before*
                // the preceding one is released. There is no interval when
                // this resource can be granted exclusively to the writer.
                let current_task = tokio::spawn(build_reader(i).execute());
                has_started(&started[i]).await;
                assert_eq!(writer_started.load(Ordering::SeqCst), 0);
                permits[i - 1].add_permits(1);
                let previous_report = tokio::time::timeout(Duration::from_secs(5), previous_task)
                    .await
                    .unwrap()
                    .unwrap();
                assert!(previous_report.succeeded());
                assert_eq!(previous_report.started, 1);
                previous_task = current_task;
                assert_eq!(writer_started.load(Ordering::SeqCst), 0);
            }
            // The last read still owns the shared lease here, even though
            // the original reader and all prior handoff readers completed.
            assert_eq!(started[OVERLAPS].load(Ordering::SeqCst), 1);
            assert_eq!(writer_started.load(Ordering::SeqCst), 0);
            permits[OVERLAPS].add_permits(1);
            assert!(
                tokio::time::timeout(Duration::from_secs(5), previous_task)
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
            eprintln!(
                "research #226 overlapping_read_handoffs round={round} completed_readers={} writer_after_final_release=true",
                OVERLAPS + 1
            );
        }
    });
}
