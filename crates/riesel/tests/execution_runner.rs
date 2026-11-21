//! Public SDK graph runner contract. A run is owned by the active session,
//! does not spawn a second runtime or retry external operations, and requires
//! async-tcp only to use the existing asynchronous execution infrastructure.
#![cfg(feature = "async-tcp")]
use riesel::{
    AccessMode, App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft,
    GraphRunError, OperationKey, OperationNode, OperationOutcome, OperationRegistry,
    OperationState, RunningApp, async_operation,
};
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

fn key(name: &str) -> OperationKey {
    OperationKey::new(name, 1).unwrap()
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

/// Each public SDK test enters a real validated application session, rather
/// than supplying a self-minted session token to the graph constructor.
fn application_session() -> RunningApp {
    let app = App::from_schema(
        r#"{"version":1,"app":{"name":"GraphFixture"},"models":{},"requires":[]}"#,
    )
    .unwrap();
    app.start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
    .unwrap()
}

#[test]
fn parallel_branches_complete_before_a_join() {
    runtime().block_on(async {
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let called = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        for name in ["left", "right"] {
            let barrier = Arc::clone(&barrier);
            let called = Arc::clone(&called);
            registry
                .register(
                    key(name),
                    async_operation(move || {
                        let barrier = Arc::clone(&barrier);
                        let called = Arc::clone(&called);
                        async move {
                            called.fetch_add(1, Ordering::SeqCst);
                            barrier.wait().await;
                            OperationOutcome::Completed
                        }
                    }),
                )
                .unwrap();
        }
        registry
            .register(
                key("join"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();

        let plan = ExecutionPlanDraft::new("branching", 1)
            .operation(
                OperationNode::new("join", key("join"))
                    .after("left")
                    .after("right"),
            )
            .operation(OperationNode::new("left", key("left")))
            .operation(OperationNode::new("right", key("right")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(2).unwrap())
            .unwrap();
        let report = tokio::time::timeout(Duration::from_secs(3), runner.execute())
            .await
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, 3);
        assert_eq!(report.peak_in_flight, 2);
        assert_eq!(called.load(Ordering::SeqCst), 2);
        assert_eq!(report.plan_id, "branching");
        assert_eq!(report.revision, 1);
    });
}

#[test]
fn signal_gates_only_its_own_branch_without_blocking_independent_work() {
    runtime().block_on(async {
        let independent = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("tick"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        let independent_clone = Arc::clone(&independent);
        registry
            .register(
                key("background"),
                async_operation(move || {
                    let independent = Arc::clone(&independent_clone);
                    async move {
                        independent.fetch_add(1, Ordering::SeqCst);
                        OperationOutcome::Completed
                    }
                }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("frame", 1)
            .operation(OperationNode::new("tick", key("tick")).on_signal("advance-tick"))
            .operation(OperationNode::new("background", key("background")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(2).unwrap())
            .unwrap();
        let signals = runner.signal_sender();
        assert!(matches!(
            signals.fire("undeclared"),
            Err(GraphRunError::UnknownSignal { .. })
        ));
        let run = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(3), async {
            while independent.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(
            !run.is_finished(),
            "tick is still waiting for its owner signal"
        );
        signals.fire("advance-tick").unwrap();
        let report = tokio::time::timeout(Duration::from_secs(3), run)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, 2);
    });
}

#[test]
fn resource_conflicts_are_serialized_and_uncertain_effects_are_not_retried() {
    runtime().block_on(async {
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        for name in ["first", "second"] {
            let active = Arc::clone(&active);
            let peak = Arc::clone(&peak);
            let calls = Arc::clone(&calls);
            registry
                .register(
                    key(name),
                    async_operation(move || {
                        let active = Arc::clone(&active);
                        let peak = Arc::clone(&peak);
                        let calls = Arc::clone(&calls);
                        async move {
                            calls.fetch_add(1, Ordering::SeqCst);
                            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(now, Ordering::SeqCst);
                            tokio::task::yield_now().await;
                            active.fetch_sub(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }),
                )
                .unwrap();
        }
        registry
            .register(
                key("unknown"),
                async_operation(|| async { OperationOutcome::OutcomeUnknown }),
            )
            .unwrap();
        registry
            .register(
                key("dependent"),
                async_operation(|| async { panic!("must not run after an unknown effect") }),
            )
            .unwrap();

        let plan = ExecutionPlanDraft::new("conflicts", 1)
            .operation(OperationNode::new("first", key("first")).access("world", AccessMode::Write))
            .operation(
                OperationNode::new("second", key("second")).access("world", AccessMode::Read),
            )
            .operation(OperationNode::new("unknown", key("unknown")))
            .operation(OperationNode::new("dependent", key("dependent")).after("unknown"))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let report = owner
            .execution_graph(plan, NonZeroUsize::new(3).unwrap())
            .unwrap()
            .execute()
            .await;
        assert_eq!(report.peak_in_flight, 2); // one writer plus the unrelated unknown op
        assert_eq!(peak.load(Ordering::SeqCst), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(report.operations["unknown"], OperationState::OutcomeUnknown);
        assert_eq!(report.operations["dependent"], OperationState::Blocked);
        assert!(!report.succeeded());
    });
}

#[test]
fn expired_session_aborts_work_conservatively_and_rejects_stale_signals() {
    runtime().block_on(async {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let started = Arc::new(std::sync::Mutex::new(Some(started_tx)));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("remote"),
                async_operation(move || {
                    let started = Arc::clone(&started);
                    async move {
                        if let Some(started) = started.lock().unwrap().take() {
                            let _ = started.send(());
                        }
                        std::future::pending::<()>().await;
                        OperationOutcome::Completed
                    }
                }),
            )
            .unwrap();
        registry
            .register(
                key("downstream"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("session", 1)
            .operation(OperationNode::new("remote", key("remote")))
            .operation(OperationNode::new("downstream", key("downstream")).after("remote"))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let session = owner.session_token();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(2).unwrap())
            .unwrap();
        let signal = runner.signal_sender();
        let run = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(3), started_rx)
            .await
            .unwrap()
            .unwrap();
        session.invalidate();
        let report = tokio::time::timeout(Duration::from_secs(3), run)
            .await
            .unwrap()
            .unwrap();
        assert!(report.session_expired);
        assert_eq!(report.operations["remote"], OperationState::OutcomeUnknown);
        assert_eq!(report.operations["downstream"], OperationState::Blocked);
        assert!(matches!(
            signal.fire("anything"),
            Err(GraphRunError::SessionExpired)
        ));
    });
}

#[test]
fn failed_prerequisite_blocks_success_dependent_without_stopping_independent_branch() {
    runtime().block_on(async {
        let independent = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("failure"),
                async_operation(|| async { OperationOutcome::Failed("operation refused".into()) }),
            )
            .unwrap();
        registry
            .register(
                key("should-not-run"),
                async_operation(|| async { panic!("dependent cannot execute after failure") }),
            )
            .unwrap();
        let completed = Arc::clone(&independent);
        registry
            .register(
                key("independent"),
                async_operation(move || {
                    let completed = Arc::clone(&completed);
                    async move {
                        completed.store(1, Ordering::SeqCst);
                        OperationOutcome::Completed
                    }
                }),
            )
            .unwrap();

        let plan = ExecutionPlanDraft::new("failure-isolation", 1)
            .operation(OperationNode::new("failed", key("failure")))
            .operation(OperationNode::new("dependent", key("should-not-run")).after("failed"))
            .operation(OperationNode::new("independent", key("independent")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let report = owner
            .execution_graph(plan, NonZeroUsize::new(2).unwrap())
            .unwrap()
            .execute()
            .await;
        assert_eq!(report.started, 2);
        assert_eq!(
            report.operations["failed"],
            OperationState::Failed("operation refused".into())
        );
        assert_eq!(report.operations["dependent"], OperationState::Blocked);
        assert_eq!(report.operations["independent"], OperationState::Completed);
        assert_eq!(independent.load(Ordering::SeqCst), 1);
        assert!(!report.succeeded());
    });
}

#[test]
fn admission_above_explicit_parallel_limit_is_rejected_before_work() {
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    registry
        .register(
            key("one"),
            async_operation(|| async { OperationOutcome::Completed }),
        )
        .unwrap();
    let plan = ExecutionPlanDraft::new("bounded", 1)
        .operation(OperationNode::new("one", key("one")))
        .validate()
        .unwrap()
        .bind(&registry)
        .unwrap();
    assert!(matches!(
        application_session().execution_graph(plan, NonZeroUsize::new(33).unwrap()),
        Err(GraphRunError::InvalidParallelism {
            requested: 33,
            maximum: 32
        })
    ));
}

#[test]
fn different_graph_runs_share_one_session_parallelism_budget() {
    runtime().block_on(async {
        const OPERATIONS_PER_RUN: usize = 24;
        const MAX_SESSION_ACTIVE: usize = 32;
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let started = Arc::new(AtomicUsize::new(0));
        let first_wave = Arc::new(tokio::sync::Barrier::new(MAX_SESSION_ACTIVE));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        let op = key("shared-capacity");
        registry
            .register(
                op.clone(),
                async_operation({
                    let active = Arc::clone(&active);
                    let peak = Arc::clone(&peak);
                    let started = Arc::clone(&started);
                    let first_wave = Arc::clone(&first_wave);
                    move || {
                        let active = Arc::clone(&active);
                        let peak = Arc::clone(&peak);
                        let started = Arc::clone(&started);
                        let first_wave = Arc::clone(&first_wave);
                        async move {
                            let sequence = started.fetch_add(1, Ordering::SeqCst);
                            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(now, Ordering::SeqCst);
                            if sequence < MAX_SESSION_ACTIVE {
                                first_wave.wait().await;
                            }
                            tokio::task::yield_now().await;
                            active.fetch_sub(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let mut left = ExecutionPlanDraft::new("shared-budget.left", 1);
        let mut right = ExecutionPlanDraft::new("shared-budget.right", 1);
        for index in 0..OPERATIONS_PER_RUN {
            left = left.operation(OperationNode::new(format!("left-{index}"), op.clone()));
            right = right.operation(OperationNode::new(format!("right-{index}"), op.clone()));
        }
        let owner = application_session();
        let left = owner
            .execution_graph(
                left.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(OPERATIONS_PER_RUN).unwrap(),
            )
            .unwrap();
        let right = owner
            .execution_graph(
                right.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(OPERATIONS_PER_RUN).unwrap(),
            )
            .unwrap();
        let left = tokio::spawn(left.execute());
        let right = tokio::spawn(right.execute());
        let result_left = tokio::time::timeout(Duration::from_secs(5), left)
            .await
            .unwrap()
            .unwrap();
        let result_right = tokio::time::timeout(Duration::from_secs(5), right)
            .await
            .unwrap()
            .unwrap();
        assert!(result_left.succeeded());
        assert!(result_right.succeeded());
        assert_eq!(started.load(Ordering::SeqCst), OPERATIONS_PER_RUN * 2);
        assert_eq!(active.load(Ordering::SeqCst), 0);
        assert_eq!(peak.load(Ordering::SeqCst), MAX_SESSION_ACTIVE);
    });
}

#[test]
fn access_conflicts_across_runs_wait_for_release_without_a_new_signal() {
    runtime().block_on(async {
        let (release_sender, release_receiver) = tokio::sync::oneshot::channel::<()>();
        let release = Arc::new(std::sync::Mutex::new(Some(release_receiver)));
        let writer_started = Arc::new(AtomicUsize::new(0));
        let reader_started = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("writer"),
                async_operation({
                    let release = Arc::clone(&release);
                    let started = Arc::clone(&writer_started);
                    move || {
                        let reply = release.lock().unwrap().take().unwrap();
                        let started = Arc::clone(&started);
                        async move {
                            started.store(1, Ordering::SeqCst);
                            let _ = reply.await;
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
                    let started = Arc::clone(&reader_started);
                    move || {
                        let started = Arc::clone(&started);
                        async move {
                            started.store(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let writer = ExecutionPlanDraft::new("same-world.writer", 1)
            .operation(OperationNode::new("write", key("writer")).access("world", AccessMode::Write))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let reader = ExecutionPlanDraft::new("same-world.reader", 1)
            .operation(OperationNode::new("read", key("reader")).access("world", AccessMode::Read))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let writer = owner.execution_graph(writer, NonZeroUsize::new(1).unwrap()).unwrap();
        let write_task = tokio::spawn(writer.execute());
        tokio::time::timeout(Duration::from_secs(3), async {
            while writer_started.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let reader = owner.execution_graph(reader, NonZeroUsize::new(1).unwrap()).unwrap();
        let read_task = tokio::spawn(reader.execute());
        tokio::time::sleep(Duration::from_millis(30)).await;
        assert_eq!(reader_started.load(Ordering::SeqCst), 0);
        release_sender.send(()).unwrap();
        assert!(
            tokio::time::timeout(Duration::from_secs(3), write_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(3), read_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        assert_eq!(reader_started.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn session_run_limit_is_bounded_and_releases_slots_on_drop() {
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    registry
        .register(
            key("slot"),
            async_operation(|| async { OperationOutcome::Completed }),
        )
        .unwrap();
    let plan = || {
        ExecutionPlanDraft::new("bounded-session", 1)
            .operation(OperationNode::new("slot", key("slot")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap()
    };
    let owner = application_session();
    let mut held = Vec::new();
    for _ in 0..16 {
        held.push(owner.execution_graph(plan(), NonZeroUsize::new(1).unwrap()).unwrap());
    }
    assert!(matches!(
        owner.execution_graph(plan(), NonZeroUsize::new(1).unwrap()),
        Err(GraphRunError::SessionRunLimitExceeded { limit: 16 })
    ));
    held.pop();
    assert!(owner.execution_graph(plan(), NonZeroUsize::new(1).unwrap()).is_ok());
}

#[test]
fn aborting_a_run_releases_reserved_resources_for_other_runs() {
    runtime().block_on(async {
        let first_started = Arc::new(AtomicUsize::new(0));
        let second_started = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("pending"),
                async_operation({
                    let first_started = Arc::clone(&first_started);
                    move || {
                        let first_started = Arc::clone(&first_started);
                        async move {
                            first_started.store(1, Ordering::SeqCst);
                            std::future::pending::<()>().await;
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        registry
            .register(
                key("after-abort"),
                async_operation({
                    let second_started = Arc::clone(&second_started);
                    move || {
                        let second_started = Arc::clone(&second_started);
                        async move {
                            second_started.store(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let first = ExecutionPlanDraft::new("abort.first", 1)
            .operation(OperationNode::new("pending", key("pending")).access("lock", AccessMode::Write))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let second = ExecutionPlanDraft::new("abort.second", 1)
            .operation(OperationNode::new("after", key("after-abort")).access("lock", AccessMode::Write))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let first = owner.execution_graph(first, NonZeroUsize::new(1).unwrap()).unwrap();
        let second = owner.execution_graph(second, NonZeroUsize::new(1).unwrap()).unwrap();
        let first_task = tokio::spawn(first.execute());
        tokio::time::timeout(Duration::from_secs(3), async {
            while first_started.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let second_task = tokio::spawn(second.execute());
        tokio::task::yield_now().await;
        assert_eq!(second_started.load(Ordering::SeqCst), 0);
        first_task.abort();
        let _ = first_task.await;
        let result = tokio::time::timeout(Duration::from_secs(3), second_task)
            .await
            .unwrap()
            .unwrap();
        assert!(result.succeeded());
        assert_eq!(second_started.load(Ordering::SeqCst), 1);
    });
}
