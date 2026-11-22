//! Public SDK graph runner contract. A run is owned by the active session,
//! does not spawn a second runtime or retry external operations, and requires
//! async-tcp only to use the existing asynchronous execution infrastructure.
#![cfg(feature = "async-tcp")]
use riesel::{
    AccessMode, App, AsyncOperation, ConnectorRegistry, DeploymentConfig, DomainAdmissionMode,
    ExecutionPlanDraft, FixedStepAdmission, FixedStepBacklog, FixedStepPolicy, GraphControlError,
    GraphControlMode, GraphControlTarget, GraphEffectError, GraphEffectRecorder,
    GraphObservationError, GraphRunChange, GraphRunError, GraphRunPhase, MAX_GRAPH_EFFECT_STREAMS,
    MAX_GRAPH_TRANSITIONS, OperationKey, OperationNode, OperationOutcome, OperationRegistry,
    OperationState, RunningApp, async_operation, async_operation_with_effects,
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
fn reverse_declared_failure_chain_blocks_without_dispatching_dependents() {
    runtime().block_on(async {
        const DEPENDENTS: usize = 256;
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("failure"),
                async_operation(|| async { OperationOutcome::Failed("root failed".into()) }),
            )
            .unwrap();
        registry
            .register(
                key("blocked"),
                async_operation(|| async { panic!("blocked dependent must never run") }),
            )
            .unwrap();
        registry
            .register(
                key("independent"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();

        // Each dependent appears before its prerequisite in declaration
        // order. Topological failure propagation must still block the entire
        // chain without dispatching any dependent handlers.
        let mut draft = ExecutionPlanDraft::new("reverse-failure-chain", 1);
        for index in (0..DEPENDENTS).rev() {
            let dependency = if index == 0 {
                "root".to_owned()
            } else {
                format!("node-{}", index - 1)
            };
            draft = draft.operation(
                OperationNode::new(format!("node-{index}"), key("blocked")).after(dependency),
            );
        }
        let plan = draft
            .operation(OperationNode::new("root", key("failure")))
            .operation(OperationNode::new("independent", key("independent")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(2).unwrap())
            .unwrap();
        let observer = runner.observer();
        let report = tokio::time::timeout(Duration::from_secs(3), runner.execute())
            .await
            .unwrap();

        assert_eq!(report.started, 2);
        assert_eq!(
            report.operations["root"],
            OperationState::Failed("root failed".into())
        );
        assert_eq!(report.operations["independent"], OperationState::Completed);
        let snapshot = observer.snapshot();
        for index in 0..DEPENDENTS {
            let id = format!("node-{index}");
            assert_eq!(report.operations[&id], OperationState::Blocked);
            assert_eq!(snapshot.operations[&id].state, OperationState::Blocked);
            assert_eq!(snapshot.operations[&id].attempt, 0);
        }
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
            .operation(
                OperationNode::new("write", key("writer")).access("world", AccessMode::Write),
            )
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
        let writer = owner
            .execution_graph(writer, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let write_task = tokio::spawn(writer.execute());
        tokio::time::timeout(Duration::from_secs(3), async {
            while writer_started.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let reader = owner
            .execution_graph(reader, NonZeroUsize::new(1).unwrap())
            .unwrap();
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
        held.push(
            owner
                .execution_graph(plan(), NonZeroUsize::new(1).unwrap())
                .unwrap(),
        );
    }
    assert!(matches!(
        owner.execution_graph(plan(), NonZeroUsize::new(1).unwrap()),
        Err(GraphRunError::SessionRunLimitExceeded { limit: 16 })
    ));
    held.pop();
    assert!(
        owner
            .execution_graph(plan(), NonZeroUsize::new(1).unwrap())
            .is_ok()
    );
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
            .operation(
                OperationNode::new("pending", key("pending")).access("lock", AccessMode::Write),
            )
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let second = ExecutionPlanDraft::new("abort.second", 1)
            .operation(
                OperationNode::new("after", key("after-abort")).access("lock", AccessMode::Write),
            )
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let first = owner
            .execution_graph(first, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let second = owner
            .execution_graph(second, NonZeroUsize::new(1).unwrap())
            .unwrap();
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

#[test]
fn domain_authority_claims_reject_conflicting_live_runs_before_any_operation() {
    let called = Arc::new(AtomicUsize::new(0));
    let mut handlers = OperationRegistry::<AsyncOperation>::new();
    handlers
        .register(
            key("operation"),
            async_operation({
                let called = Arc::clone(&called);
                move || {
                    let called = Arc::clone(&called);
                    async move {
                        called.fetch_add(1, Ordering::SeqCst);
                        OperationOutcome::Completed
                    }
                }
            }),
        )
        .unwrap();
    let central = || {
        ExecutionPlanDraft::new("domain.central", 1)
            .operation(OperationNode::new("tick", key("operation")).domain_tick("simulation"))
            .validate()
            .unwrap()
            .bind(&handlers)
            .unwrap()
    };
    let delegated = || {
        ExecutionPlanDraft::new("domain.delegated", 1)
            .operation(OperationNode::new("frame", key("operation")).delegated_frame("simulation"))
            .validate()
            .unwrap()
            .bind(&handlers)
            .unwrap()
    };
    let owner = application_session();
    let first = owner
        .execution_graph(central(), NonZeroUsize::new(1).unwrap())
        .unwrap();
    assert!(matches!(
        owner.execution_graph(delegated(), NonZeroUsize::new(1).unwrap()),
        Err(GraphRunError::ConflictingDomainAuthority {
            domain,
            existing: DomainAdmissionMode::RieselTick,
            requested: DomainAdmissionMode::DelegatedFrame,
        }) if domain == "simulation"
    ));
    assert_eq!(called.load(Ordering::SeqCst), 0);

    // A rejected plan does not corrupt the existing owner, and ownership
    // can be handed off only after the previous graph lease has ended.
    let parallel = owner
        .execution_graph(central(), NonZeroUsize::new(1).unwrap())
        .unwrap();
    drop(parallel);
    drop(first);
    assert!(
        owner
            .execution_graph(delegated(), NonZeroUsize::new(1).unwrap())
            .is_ok()
    );
    assert_eq!(called.load(Ordering::SeqCst), 0);
}

#[test]
fn declared_domain_admission_serializes_cross_run_ticks_without_resource_keys() {
    runtime().block_on(async {
        let first_started = Arc::new(AtomicUsize::new(0));
        let second_started = Arc::new(AtomicUsize::new(0));
        let independent_started = Arc::new(AtomicUsize::new(0));
        let release = Arc::new(tokio::sync::Notify::new());
        let mut handlers = OperationRegistry::<AsyncOperation>::new();
        handlers
            .register(
                key("hold"),
                async_operation({
                    let first_started = Arc::clone(&first_started);
                    let release = Arc::clone(&release);
                    move || {
                        let first_started = Arc::clone(&first_started);
                        let release = Arc::clone(&release);
                        async move {
                            first_started.store(1, Ordering::SeqCst);
                            release.notified().await;
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        handlers
            .register(
                key("next"),
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
        handlers
            .register(
                key("independent"),
                async_operation({
                    let independent_started = Arc::clone(&independent_started);
                    move || {
                        let independent_started = Arc::clone(&independent_started);
                        async move {
                            independent_started.store(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let first_plan = ExecutionPlanDraft::new("domain.first", 1)
            .operation(OperationNode::new("hold", key("hold")).domain_tick("world"))
            .validate()
            .unwrap()
            .bind(&handlers)
            .unwrap();
        let second_plan = ExecutionPlanDraft::new("domain.second", 1)
            .operation(OperationNode::new("next", key("next")).domain_tick("world"))
            .operation(OperationNode::new("other", key("independent")))
            .validate()
            .unwrap()
            .bind(&handlers)
            .unwrap();

        let owner = application_session();
        let first = owner
            .execution_graph(first_plan, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let second = owner
            .execution_graph(second_plan, NonZeroUsize::new(2).unwrap())
            .unwrap();
        let first_task = tokio::spawn(first.execute());
        tokio::time::timeout(Duration::from_secs(3), async {
            while first_started.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let second_task = tokio::spawn(second.execute());
        tokio::time::timeout(Duration::from_secs(3), async {
            while independent_started.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(second_started.load(Ordering::SeqCst), 0);
        release.notify_one();
        assert!(
            tokio::time::timeout(Duration::from_secs(3), first_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(3), second_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        assert_eq!(second_started.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn bounded_frame_budget_runs_a_reused_immutable_plan_without_implicit_retries() {
    runtime().block_on(async {
        let completed = Arc::new(AtomicUsize::new(0));
        let mut handlers = OperationRegistry::<AsyncOperation>::new();
        handlers
            .register(
                key("tick"),
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
        let plan = ExecutionPlanDraft::new("reusable.fixed-tick", 1)
            .operation(OperationNode::new("tick", key("tick")).domain_tick("world"))
            .validate()
            .unwrap()
            .bind(&handlers)
            .unwrap();
        let mut clock = FixedStepAdmission::new(
            FixedStepPolicy::new(
                Duration::from_millis(10),
                NonZeroUsize::new(2).unwrap(),
                FixedStepBacklog::DiscardExcess,
            )
            .unwrap(),
        );
        // One external frame advances three logical intervals, but this
        // Riesel-owned domain admits only two. The third is explicitly dropped.
        let budget = clock.advance_frame(Duration::from_millis(30));
        assert_eq!(budget.eligible, 2);
        assert_eq!(budget.discarded, 1);
        let owner = application_session();
        for _ in 0..budget.eligible {
            assert_eq!(clock.claim(1), 1);
            // Reuse the exact immutable semantic plan and registered Rust
            // handlers; each run receives new session admission resources.
            let runner = owner
                .execution_graph(plan.clone(), NonZeroUsize::new(1).unwrap())
                .unwrap();
            let result = runner.execute().await;
            assert!(result.succeeded());
            assert_eq!(result.plan_id, "reusable.fixed-tick");
            assert_eq!(result.revision, 1);
        }
        assert_eq!(clock.available(), 0);
        assert_eq!(completed.load(Ordering::SeqCst), 2);
        assert_eq!(clock.advance_frame(Duration::ZERO).eligible, 0);

        // A new application session is not a continuation of the old clock.
        let restarted = application_session();
        let mut fresh_clock = FixedStepAdmission::new(clock.policy());
        assert_eq!(fresh_clock.advance_frame(Duration::ZERO).eligible, 0);
        assert_eq!(fresh_clock.claim(1), 0);
        let runner = restarted
            .execution_graph(plan, NonZeroUsize::new(1).unwrap())
            .unwrap();
        assert!(runner.execute().await.succeeded());
        assert_eq!(completed.load(Ordering::SeqCst), 3);
    });
}

#[test]
fn observer_exposes_live_running_state_and_correlated_completion_without_polling_traces() {
    runtime().block_on(async {
        let release = Arc::new(tokio::sync::Notify::new());
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("hold"),
                async_operation({
                    let release = Arc::clone(&release);
                    move || {
                        let release = Arc::clone(&release);
                        async move {
                            release.notified().await;
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        registry
            .register(
                key("follow"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("observed", 7)
            .operation(OperationNode::new("hold", key("hold")))
            .operation(OperationNode::new("follow", key("follow")).after("hold"))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(2).unwrap())
            .unwrap();
        let observer = runner.observer();
        let created = observer.snapshot();
        assert_eq!(created.phase, GraphRunPhase::Created);
        assert_eq!(created.identity.plan_id(), "observed");
        assert_eq!(created.identity.revision(), 7);
        assert_eq!(created.latest_sequence, 0);
        assert_eq!(created.operations["hold"].attempt, 0);
        let handle = tokio::spawn(runner.execute());

        let running = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let snapshot = observer.snapshot();
                if snapshot.operations["hold"].attempt == 1 {
                    break snapshot;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(running.phase, GraphRunPhase::Running);
        assert_eq!(running.operations["hold"].state, OperationState::Running);
        assert_eq!(running.operations["follow"].state, OperationState::Pending);
        assert_eq!(running.operations["follow"].attempt, 0);
        assert!(!handle.is_finished());
        let events = observer.changes_since(0).unwrap();
        assert!(!events.truncated);
        assert!(
            events
                .events
                .iter()
                .all(|event| event.identity == created.identity)
        );
        assert!(events.events.iter().any(|event| matches!(
            &event.change,
            GraphRunChange::Operation { operation_id, attempt: 1, state: OperationState::Running }
                if operation_id == "hold"
        )));

        release.notify_one();
        let report = tokio::time::timeout(Duration::from_secs(3), handle)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.identity, created.identity);
        let done = observer.snapshot();
        assert_eq!(done.phase, GraphRunPhase::Finished);
        assert_eq!(done.identity, report.identity);
        assert_eq!(done.operations["hold"].state, OperationState::Completed);
        assert_eq!(done.operations["follow"].state, OperationState::Completed);
        assert_eq!(done.operations["follow"].attempt, 1);
        let tail = observer.changes_since(events.latest_sequence).unwrap();
        assert!(!tail.truncated);
        assert!(
            tail.events.iter().any(|event| matches!(
                event.change,
                GraphRunChange::Phase(GraphRunPhase::Finished)
            ))
        );
    });
}

#[test]
fn run_identities_bind_one_immutable_revision_to_distinct_session_generations() {
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    registry
        .register(
            key("read"),
            async_operation(|| async { OperationOutcome::Completed }),
        )
        .unwrap();
    let plan = |revision| {
        ExecutionPlanDraft::new("revised", revision)
            .operation(OperationNode::new("read", key("read")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap()
    };
    let first_session = application_session();
    let first = first_session
        .execution_graph(plan(2), NonZeroUsize::new(1).unwrap())
        .unwrap();
    let first_observer = first.observer();
    let second = first_session
        .execution_graph(plan(3), NonZeroUsize::new(1).unwrap())
        .unwrap();
    let first_id = first_observer.snapshot().identity;
    let second_id = second.observer().snapshot().identity;
    assert_eq!(
        first_id.session_generation(),
        second_id.session_generation()
    );
    assert_ne!(first_id.run_sequence(), second_id.run_sequence());
    assert_eq!(first_id.revision(), 2);
    assert_eq!(second_id.revision(), 3);

    let restarted_session = application_session();
    let restarted = restarted_session
        .execution_graph(plan(2), NonZeroUsize::new(1).unwrap())
        .unwrap();
    let restarted_id = restarted.observer().snapshot().identity;
    assert_ne!(
        first_id.session_generation(),
        restarted_id.session_generation()
    );
    assert_ne!(first_id, restarted_id);
    assert_eq!(first_observer.snapshot().identity, first_id);
    // Dropping an unexecuted plan never creates a completed trace.
    drop(first);
    assert_eq!(first_observer.snapshot().phase, GraphRunPhase::Abandoned);
    assert_eq!(
        first_observer.snapshot().operations["read"].state,
        OperationState::Cancelled
    );
}

#[test]
fn lossy_transition_ring_reports_overflow_and_resnapshot_reconciles_every_operation() {
    runtime().block_on(async {
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("noop"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        let mut plan = ExecutionPlanDraft::new("overflow", 1);
        for index in 0..90 {
            plan = plan.operation(OperationNode::new(format!("node-{index}"), key("noop")));
        }
        let owner = application_session();
        let runner = owner
            .execution_graph(
                plan.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(8).unwrap(),
            )
            .unwrap();
        let observer = runner.observer();
        let report = runner.execute().await;
        assert!(report.succeeded());
        let history = observer.changes_since(0).unwrap();
        assert!(history.truncated, "ring overflow is detectable");
        assert_eq!(history.events.len(), MAX_GRAPH_TRANSITIONS);
        assert!(history.events[0].sequence > 1);
        assert!(
            history
                .events
                .windows(2)
                .all(|pair| pair[0].sequence + 1 == pair[1].sequence)
        );
        assert_eq!(history.latest_sequence, observer.snapshot().latest_sequence);
        assert!(matches!(
            observer.changes_since(history.latest_sequence + 1),
            Err(GraphObservationError::FutureCursor { .. })
        ));
        let recovered = observer.snapshot();
        assert_eq!(recovered.phase, GraphRunPhase::Finished);
        assert_eq!(recovered.operations.len(), 90);
        assert!(
            recovered
                .operations
                .values()
                .all(|node| node.state == OperationState::Completed && node.attempt == 1)
        );
        let recent = observer
            .changes_since(recovered.latest_sequence - 1)
            .unwrap();
        assert!(!recent.truncated);
        assert_eq!(recent.events.len(), 1);
        assert!(matches!(
            recent.events[0].change,
            GraphRunChange::Phase(GraphRunPhase::Finished)
        ));
        assert!(
            observer
                .changes_since(recovered.latest_sequence)
                .unwrap()
                .events
                .is_empty()
        );
    });
}

#[test]
fn abrupt_graph_future_abort_reports_in_flight_unknown_and_blocks_success_dependents() {
    runtime().block_on(async {
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("pending"),
                async_operation(|| async {
                    std::future::pending::<()>().await;
                    OperationOutcome::Completed
                }),
            )
            .unwrap();
        registry
            .register(
                key("noop"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("abandon", 1)
            .operation(OperationNode::new("pending", key("pending")))
            .operation(OperationNode::new("dependent", key("noop")).after("pending"))
            .operation(OperationNode::new("unrelated", key("noop")).on_signal("not-fired"))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(2).unwrap())
            .unwrap();
        let observer = runner.observer();
        let handle = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(3), async {
            while observer.snapshot().operations["pending"].state != OperationState::Running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        handle.abort();
        let _ = handle.await;
        let terminal = observer.snapshot();
        assert_eq!(terminal.phase, GraphRunPhase::Abandoned);
        assert_eq!(
            terminal.operations["pending"].state,
            OperationState::OutcomeUnknown
        );
        assert_eq!(terminal.operations["pending"].attempt, 1);
        assert_eq!(
            terminal.operations["dependent"].state,
            OperationState::Blocked
        );
        assert_eq!(terminal.operations["dependent"].attempt, 0);
        assert_eq!(
            terminal.operations["unrelated"].state,
            OperationState::Cancelled
        );
        assert_eq!(terminal.operations["unrelated"].attempt, 0);
        assert!(
            observer
                .changes_since(0)
                .unwrap()
                .events
                .iter()
                .any(|event| matches!(
                    event.change,
                    GraphRunChange::Phase(GraphRunPhase::Abandoned)
                ))
        );
    });
}

#[test]
fn confirmed_effect_prefixes_survive_failed_operation_without_unblocking_dependents() {
    runtime().block_on(async {
        let retained = Arc::new(std::sync::Mutex::new(None::<GraphEffectRecorder>));
        let held = Arc::clone(&retained);
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("partially-applied"),
                async_operation_with_effects(move |effects| {
                    let held = Arc::clone(&held);
                    async move {
                        *held.lock().unwrap() = Some(effects.clone());
                        effects.confirm_prefix("world.ticks", 1).unwrap();
                        effects.confirm_prefix("world.ticks", 1).unwrap();
                        assert!(matches!(
                            effects.confirm_prefix("world.ticks", 0),
                            Err(GraphEffectError::ZeroProgress)
                        ));
                        assert!(matches!(effects.confirm_prefix("world.ticks", 2), Ok(())));
                        assert!(matches!(
                            effects.confirm_prefix("world.ticks", 1),
                            Err(GraphEffectError::RegressingPrefix {
                                previous: 2,
                                requested: 1
                            })
                        ));
                        OperationOutcome::Failed("failed after two committed ticks".into())
                    }
                }),
            )
            .unwrap();
        registry
            .register(
                key("dependent"),
                async_operation(|| async { panic!("must not run after failed prerequisite") }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("effects.failed-prefix", 1)
            .operation(OperationNode::new("partial", key("partially-applied")))
            .operation(OperationNode::new("dependent", key("dependent")).after("partial"))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(2).unwrap())
            .unwrap();
        let observer = runner.observer();
        let report = runner.execute().await;
        assert!(!report.succeeded());
        assert!(matches!(
            report.operations["partial"],
            OperationState::Failed(_)
        ));
        assert_eq!(report.operations["dependent"], OperationState::Blocked);
        assert_eq!(report.confirmed_prefixes["partial"]["world.ticks"], 2);
        let snapshot = observer.snapshot();
        assert_eq!(
            snapshot.operations["partial"].confirmed_prefixes["world.ticks"],
            2
        );
        assert!(
            snapshot.operations["dependent"]
                .confirmed_prefixes
                .is_empty()
        );
        let updates = observer.changes_since(0).unwrap();
        assert!(!updates.truncated);
        assert_eq!(
            updates
                .events
                .iter()
                .filter(|event| matches!(
                    &event.change,
                    GraphRunChange::ConfirmedPrefix {
                        operation_id,
                        attempt: 1,
                        stream,
                        ..
                    } if operation_id == "partial" && stream == "world.ticks"
                ))
                .count(),
            2,
            "equal duplicate reports must not advance a confirmed prefix"
        );
        let old = retained.lock().unwrap().take().unwrap();
        assert!(matches!(
            old.confirm_prefix("world.ticks", 3),
            Err(GraphEffectError::InactiveAttempt)
        ));
    });
}

#[test]
fn abandoned_attempt_preserves_confirmed_prefix_but_not_unacknowledged_effects() {
    runtime().block_on(async {
        let retained = Arc::new(std::sync::Mutex::new(None::<GraphEffectRecorder>));
        let held = Arc::clone(&retained);
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("pending-effect"),
                async_operation_with_effects(move |effects| {
                    let held = Arc::clone(&held);
                    async move {
                        *held.lock().unwrap() = Some(effects.clone());
                        effects.confirm_prefix("world.ticks", 1).unwrap();
                        // The second remote tick might commit after dispatch,
                        // but no acknowledgement arrives before cancellation.
                        std::future::pending::<()>().await;
                        OperationOutcome::Completed
                    }
                }),
            )
            .unwrap();
        registry
            .register(
                key("dependent"),
                async_operation(|| async { panic!("unknown prerequisite cannot succeed") }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("effects.abandoned", 1)
            .operation(OperationNode::new("pending", key("pending-effect")))
            .operation(OperationNode::new("dependent", key("dependent")).after("pending"))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let observer = runner.observer();
        let handle = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if observer.snapshot().operations["pending"]
                    .confirmed_prefixes
                    .get("world.ticks")
                    == Some(&1)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        handle.abort();
        let _ = handle.await;
        let state = observer.snapshot();
        assert_eq!(state.phase, GraphRunPhase::Abandoned);
        assert_eq!(
            state.operations["pending"].state,
            OperationState::OutcomeUnknown
        );
        assert_eq!(
            state.operations["pending"].confirmed_prefixes["world.ticks"],
            1
        );
        assert_eq!(state.operations["dependent"].state, OperationState::Blocked);
        let old = retained.lock().unwrap().take().unwrap();
        assert!(matches!(
            old.confirm_prefix("world.ticks", 2),
            Err(GraphEffectError::InactiveAttempt)
        ));
    });
}

#[test]
fn effect_stream_limits_and_invalid_names_fail_without_ledger_mutation() {
    runtime().block_on(async {
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("bounded-effects"),
                async_operation_with_effects(|effects| async move {
                    assert!(matches!(
                        effects.confirm_prefix("", 1),
                        Err(GraphEffectError::InvalidStream)
                    ));
                    assert!(matches!(
                        effects.confirm_prefix("invalid name", 1),
                        Err(GraphEffectError::InvalidStream)
                    ));
                    for stream in 0..MAX_GRAPH_EFFECT_STREAMS {
                        effects
                            .confirm_prefix(&format!("stream.{stream}"), 1)
                            .unwrap();
                    }
                    assert!(matches!(
                        effects.confirm_prefix("stream.extra", 1),
                        Err(GraphEffectError::StreamLimitExceeded {
                            limit: MAX_GRAPH_EFFECT_STREAMS
                        })
                    ));
                    // Reusing an existing stream does not consume new capacity.
                    effects.confirm_prefix("stream.0", 2).unwrap();
                    OperationOutcome::OutcomeUnknown
                }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("effects.bound", 1)
            .operation(OperationNode::new("bounded", key("bounded-effects")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let observer = runner.observer();
        let report = runner.execute().await;
        assert_eq!(report.operations["bounded"], OperationState::OutcomeUnknown);
        assert_eq!(
            report.confirmed_prefixes["bounded"].len(),
            MAX_GRAPH_EFFECT_STREAMS
        );
        assert_eq!(report.confirmed_prefixes["bounded"]["stream.0"], 2);
        assert_eq!(
            observer.snapshot().operations["bounded"].confirmed_prefixes,
            report.confirmed_prefixes["bounded"]
        );
    });
}

#[test]
fn overflowed_effect_transitions_reconcile_from_authoritative_snapshot() {
    runtime().block_on(async {
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("confirmed"),
                async_operation_with_effects(|effects| async move {
                    effects.confirm_prefix("tick", 1).unwrap();
                    effects.confirm_prefix("tick", 2).unwrap();
                    OperationOutcome::Completed
                }),
            )
            .unwrap();
        let mut draft = ExecutionPlanDraft::new("effects.overflow", 1);
        for index in 0..50 {
            draft = draft.operation(OperationNode::new(format!("op-{index}"), key("confirmed")));
        }
        let owner = application_session();
        let runner = owner
            .execution_graph(
                draft.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(8).unwrap(),
            )
            .unwrap();
        let observer = runner.observer();
        let report = runner.execute().await;
        assert!(report.succeeded());
        let events = observer.changes_since(0).unwrap();
        assert!(events.truncated);
        assert_eq!(events.events.len(), MAX_GRAPH_TRANSITIONS);
        let snapshot = observer.snapshot();
        assert_eq!(snapshot.operations.len(), 50);
        for index in 0..50 {
            let name = format!("op-{index}");
            assert_eq!(snapshot.operations[&name].confirmed_prefixes["tick"], 2);
            assert_eq!(report.confirmed_prefixes[&name]["tick"], 2);
        }
        let recent = observer.changes_since(snapshot.latest_sequence).unwrap();
        assert!(!recent.truncated);
        assert!(recent.events.is_empty());
    });
}

fn domain_ticks(name: &str) -> GraphControlTarget {
    GraphControlTarget::RieselDomainTicks(name.to_owned())
}

fn operation_target(name: &str) -> GraphControlTarget {
    GraphControlTarget::Operation(name.to_owned())
}

#[test]
fn cooperative_domain_tick_step_admits_one_outer_tick_and_not_independent_work() {
    runtime().block_on(async {
        let ticks = Arc::new(AtomicUsize::new(0));
        let independent = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("tick"),
                async_operation({
                    let ticks = Arc::clone(&ticks);
                    move || {
                        let ticks = Arc::clone(&ticks);
                        async move {
                            ticks.fetch_add(1, Ordering::SeqCst);
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
        registry
            .register(
                key("join"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("cooperative.domain", 1)
            .operation(
                OperationNode::new("tick-one", key("tick"))
                    .domain_tick("ecs.world")
                    .on_signal("first"),
            )
            .operation(
                OperationNode::new("tick-two", key("tick"))
                    .after("tick-one")
                    .domain_tick("ecs.world")
                    .on_signal("second"),
            )
            .operation(OperationNode::new("unrelated", key("independent")))
            .operation(
                OperationNode::new("join", key("join"))
                    .after("tick-two")
                    .after("unrelated"),
            )
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(3).unwrap())
            .unwrap();
        let observer = runner.observer();
        let control = runner.controller();
        let signal = runner.signal_sender();
        let target = domain_ticks("ecs.world");
        control.pause(target.clone()).unwrap();
        assert_eq!(
            observer.snapshot().controls[&target],
            GraphControlMode::Paused
        );
        signal.fire("first").unwrap();
        signal.fire("second").unwrap();
        let task = tokio::spawn(runner.execute());

        tokio::time::timeout(Duration::from_secs(3), async {
            while independent.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let paused = observer.snapshot();
        assert_eq!(paused.operations["tick-one"].state, OperationState::Pending);
        assert_eq!(paused.operations["tick-two"].state, OperationState::Pending);
        assert_eq!(ticks.load(Ordering::SeqCst), 0);
        assert!(!task.is_finished());

        control.step(target.clone()).unwrap();
        assert_eq!(
            observer.snapshot().controls[&target],
            GraphControlMode::StepArmed
        );
        assert!(matches!(
            control.step(target.clone()),
            Err(GraphControlError::StepAlreadyArmed)
        ));
        tokio::time::timeout(Duration::from_secs(3), async {
            while observer.snapshot().operations["tick-one"].state != OperationState::Completed {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let after_one = observer.snapshot();
        assert_eq!(after_one.controls[&target], GraphControlMode::Paused);
        assert_eq!(
            after_one.operations["tick-two"].state,
            OperationState::Pending
        );
        assert_eq!(ticks.load(Ordering::SeqCst), 1);
        assert!(!task.is_finished());

        control.step(target.clone()).unwrap();
        let report = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(ticks.load(Ordering::SeqCst), 2);
        assert_eq!(independent.load(Ordering::SeqCst), 1);
        assert_eq!(
            observer.snapshot().controls[&target],
            GraphControlMode::Paused
        );
        assert!(
            observer
                .changes_since(0)
                .unwrap()
                .events
                .iter()
                .any(|event| {
                    matches!(
                        &event.change,
                        GraphRunChange::Control {
                            target: event_target,
                            mode: GraphControlMode::StepArmed
                        } if event_target == &target
                    )
                })
        );
        assert!(matches!(
            control.resume(target),
            Err(GraphControlError::RunClosed)
        ));
    });
}

#[test]
fn cooperative_run_wide_gate_steps_one_operation_then_resumes_all() {
    runtime().block_on(async {
        let called = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("counter"),
                async_operation({
                    let called = Arc::clone(&called);
                    move || {
                        let called = Arc::clone(&called);
                        async move {
                            called.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("cooperative.run", 1)
            .operation(OperationNode::new("one", key("counter")))
            .operation(OperationNode::new("two", key("counter")))
            .operation(OperationNode::new("three", key("counter")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(3).unwrap())
            .unwrap();
        let observer = runner.observer();
        let control = runner.controller();
        let run = GraphControlTarget::Run;
        assert_eq!(control.identity(), observer.snapshot().identity);
        assert!(matches!(
            control.step(run.clone()),
            Err(GraphControlError::NotPaused)
        ));
        control.pause(run.clone()).unwrap();
        let task = tokio::spawn(runner.execute());
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        assert_eq!(called.load(Ordering::SeqCst), 0);
        assert!(!task.is_finished());
        control.step(run.clone()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while called.load(Ordering::SeqCst) < 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        assert_eq!(called.load(Ordering::SeqCst), 1);
        assert_eq!(observer.snapshot().controls[&run], GraphControlMode::Paused);
        control.resume(run.clone()).unwrap();
        let report = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
        assert_eq!(report.started, 3);
        assert_eq!(called.load(Ordering::SeqCst), 3);
        assert_eq!(
            observer.snapshot().controls[&run],
            GraphControlMode::Running
        );
    });
}

#[test]
fn cooperative_controls_reject_delegated_internal_ticks_and_nonpreemptible_callbacks() {
    runtime().block_on(async {
        let release = Arc::new(tokio::sync::Notify::new());
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("delegated"),
                async_operation({
                    let release = Arc::clone(&release);
                    move || {
                        let release = Arc::clone(&release);
                        async move {
                            release.notified().await;
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("cooperative.delegated", 1)
            .operation(OperationNode::new("frame", key("delegated")).delegated_frame("ecs.world"))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let observer = runner.observer();
        let control = runner.controller();
        assert!(matches!(
            control.pause(domain_ticks("ecs.world")),
            Err(GraphControlError::UnsupportedDomainAuthority {
                owner: DomainAdmissionMode::DelegatedFrame,
                ..
            })
        ));
        assert!(matches!(
            control.pause(domain_ticks("unknown")),
            Err(GraphControlError::UnknownTarget { .. })
        ));
        assert!(matches!(
            control.pause(operation_target("internal.system")),
            Err(GraphControlError::UnknownTarget { .. })
        ));
        let outer = operation_target("frame");
        assert_eq!(
            observer.snapshot().controls.get(&domain_ticks("ecs.world")),
            None
        );
        control.pause(outer.clone()).unwrap();
        let task = tokio::spawn(runner.execute());
        for _ in 0..3 {
            tokio::task::yield_now().await;
        }
        assert_eq!(
            observer.snapshot().operations["frame"].state,
            OperationState::Pending
        );
        control.step(outer.clone()).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while observer.snapshot().operations["frame"].state != OperationState::Running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(matches!(
            control.pause(outer),
            Err(GraphControlError::NoPendingSafePoint { .. })
        ));
        // A previously admitted opaque callback stays in flight. No false
        // native/ECS preemption or fabricated tick is implied by this API.
        assert!(!task.is_finished());
        release.notify_one();
        let report = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.succeeded());
    });
}

#[test]
fn cooperative_step_waits_for_session_resource_release_without_losing_credit() {
    runtime().block_on(async {
        let release = Arc::new(tokio::sync::Notify::new());
        let admitted = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("held"),
                async_operation({
                    let release = Arc::clone(&release);
                    move || {
                        let release = Arc::clone(&release);
                        async move {
                            release.notified().await;
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        registry
            .register(
                key("waiting"),
                async_operation({
                    let admitted = Arc::clone(&admitted);
                    move || {
                        let admitted = Arc::clone(&admitted);
                        async move {
                            admitted.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let first = ExecutionPlanDraft::new("cooperative.first", 1)
            .operation(
                OperationNode::new("held", key("held")).access("resource", AccessMode::Write),
            )
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let second = ExecutionPlanDraft::new("cooperative.second", 1)
            .operation(
                OperationNode::new("waiting", key("waiting")).access("resource", AccessMode::Write),
            )
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let first = owner
            .execution_graph(first, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let first_view = first.observer();
        let first_task = tokio::spawn(first.execute());
        tokio::time::timeout(Duration::from_secs(3), async {
            while first_view.snapshot().operations["held"].state != OperationState::Running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let second = owner
            .execution_graph(second, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let control = second.controller();
        let observer = second.observer();
        control.pause(GraphControlTarget::Run).unwrap();
        control.step(GraphControlTarget::Run).unwrap();
        let second_task = tokio::spawn(second.execute());
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(admitted.load(Ordering::SeqCst), 0);
        assert_eq!(
            observer.snapshot().controls[&GraphControlTarget::Run],
            GraphControlMode::StepArmed
        );
        assert_eq!(observer.snapshot().operations["waiting"].attempt, 0);
        release.notify_one();
        assert!(
            tokio::time::timeout(Duration::from_secs(3), first_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(3), second_task)
                .await
                .unwrap()
                .unwrap()
                .succeeded()
        );
        assert_eq!(admitted.load(Ordering::SeqCst), 1);
        assert_eq!(
            observer.snapshot().controls[&GraphControlTarget::Run],
            GraphControlMode::Paused
        );
    });
}

#[test]
fn paused_graph_cancels_with_session_and_old_controller_cannot_modify_new_run() {
    runtime().block_on(async {
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("never"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("cooperative.restart", 1)
            .operation(OperationNode::new("never", key("never")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan.clone(), NonZeroUsize::new(1).unwrap())
            .unwrap();
        let old = runner.controller();
        old.pause(GraphControlTarget::Run).unwrap();
        let observer = runner.observer();
        let task = tokio::spawn(runner.execute());
        tokio::time::timeout(Duration::from_secs(3), async {
            while observer.snapshot().phase != GraphRunPhase::Running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        owner.session_token().invalidate();
        let report = tokio::time::timeout(Duration::from_secs(3), task)
            .await
            .unwrap()
            .unwrap();
        assert!(report.session_expired);
        assert_eq!(report.operations["never"], OperationState::Cancelled);
        assert!(matches!(
            old.step(GraphControlTarget::Run),
            Err(GraphControlError::SessionExpired)
        ));
        let restarted = application_session();
        let next = restarted
            .execution_graph(plan, NonZeroUsize::new(1).unwrap())
            .unwrap();
        assert_ne!(old.identity(), next.controller().identity());
        assert_eq!(
            next.observer().snapshot().controls[&GraphControlTarget::Run],
            GraphControlMode::Running
        );
        assert!(next.execute().await.succeeded());
    });
}

#[test]
fn control_transition_overflow_resnapshots_authoritative_gate_without_replay() {
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    registry
        .register(
            key("once"),
            async_operation(|| async { OperationOutcome::Completed }),
        )
        .unwrap();
    let plan = ExecutionPlanDraft::new("cooperative.overflow", 1)
        .operation(OperationNode::new("once", key("once")))
        .validate()
        .unwrap()
        .bind(&registry)
        .unwrap();
    let owner = application_session();
    let runner = owner
        .execution_graph(plan, NonZeroUsize::new(1).unwrap())
        .unwrap();
    let observer = runner.observer();
    let control = runner.controller();
    for _ in 0..MAX_GRAPH_TRANSITIONS {
        control.pause(GraphControlTarget::Run).unwrap();
        control.resume(GraphControlTarget::Run).unwrap();
    }
    let history = observer.changes_since(0).unwrap();
    assert!(history.truncated);
    assert_eq!(history.events.len(), MAX_GRAPH_TRANSITIONS);
    let snapshot = observer.snapshot();
    assert_eq!(
        snapshot.controls[&GraphControlTarget::Run],
        GraphControlMode::Running
    );
    assert_eq!(snapshot.operations["once"].state, OperationState::Pending);
    assert_eq!(snapshot.latest_sequence, (2 * MAX_GRAPH_TRANSITIONS) as u64);
    assert!(
        observer
            .changes_since(snapshot.latest_sequence)
            .unwrap()
            .events
            .is_empty()
    );
}

#[test]
fn paused_structured_background_graph_drains_on_owner_scope_stop() {
    runtime().block_on(async {
        let invoked = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("must-not-run"),
                async_operation({
                    let invoked = Arc::clone(&invoked);
                    move || {
                        let invoked = Arc::clone(&invoked);
                        async move {
                            invoked.fetch_add(1, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("cooperative.structured-shutdown", 1)
            .operation(OperationNode::new("pending", key("must-not-run")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let control = runner.controller();
        let observer = runner.observer();
        control.pause(GraphControlTarget::Run).unwrap();
        let tasks = owner.async_tasks();
        tasks.begin_scope().unwrap();
        tasks.spawn_graph("paused-graph", runner).unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while observer.snapshot().phase != GraphRunPhase::Running {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(observer.snapshot().operations["pending"].attempt, 0);
        tokio::time::timeout(Duration::from_secs(3), tasks.finish_scope())
            .await
            .expect("paused graph must not block structured owner shutdown")
            .expect("planned shutdown must not become a background graph failure");
        assert_eq!(invoked.load(Ordering::SeqCst), 0);
        assert_eq!(observer.snapshot().phase, GraphRunPhase::Finished);
        assert_eq!(
            observer.snapshot().operations["pending"].state,
            OperationState::Cancelled
        );
        assert!(matches!(
            control.step(GraphControlTarget::Run),
            Err(GraphControlError::SessionExpired)
        ));
    });
}
