//! Public SDK graph runner contract. A run is owned by the active session,
//! does not spawn a second runtime or retry external operations, and requires
//! async-tcp only to use the existing asynchronous execution infrastructure.
#![cfg(feature = "async-tcp")]
use riesel::{
    AccessMode, AsyncOperation, ExecutionGraphRunner, ExecutionPlanDraft, GraphRunError,
    OperationKey, OperationNode, OperationOutcome, OperationRegistry, OperationState, SessionToken,
    async_operation,
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
        let runner =
            ExecutionGraphRunner::new(plan, SessionToken::new(), NonZeroUsize::new(2).unwrap())
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
        let session = SessionToken::new();
        let runner =
            ExecutionGraphRunner::new(plan, session, NonZeroUsize::new(2).unwrap()).unwrap();
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
        let report =
            ExecutionGraphRunner::new(plan, SessionToken::new(), NonZeroUsize::new(3).unwrap())
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
        let session = SessionToken::new();
        let runner =
            ExecutionGraphRunner::new(plan, session.clone(), NonZeroUsize::new(2).unwrap())
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
        let report =
            ExecutionGraphRunner::new(plan, SessionToken::new(), NonZeroUsize::new(2).unwrap())
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
        ExecutionGraphRunner::new(plan, SessionToken::new(), NonZeroUsize::new(33).unwrap()),
        Err(GraphRunError::InvalidParallelism {
            requested: 33,
            maximum: 32
        })
    ));
}
