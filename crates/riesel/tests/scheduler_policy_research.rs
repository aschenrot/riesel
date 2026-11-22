//! Issue #226: research-only probes against the production session-owned runner.
//!
//! This branch is deliberately not an accepted clock, fairness or scale policy.
//! Timings are diagnostic samples from a shared CI runner, not acceptance SLAs.
#![cfg(feature = "async-tcp")]

use riesel::{
    App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft,
    FixedStepAdmission, FixedStepBacklog, FixedStepPolicy, GraphRunError, GraphRunPhase,
    OperationKey, OperationNode, OperationOutcome, OperationRegistry, OperationState, RunningApp,
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

fn application_session() -> RunningApp {
    let app = App::from_schema(
        r#"{"version":1,"app":{"name":"SchedulerPolicyProbe"},"models":{},"requires":[]}"#,
    )
    .unwrap();
    app.start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
    .unwrap()
}

#[test]
fn repeated_signal_messages_do_not_reexecute_one_shot_node() {
    runtime().block_on(async {
        let invoked = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("tick"),
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
        let plan = ExecutionPlanDraft::new("signal-one-shot-research", 1)
            .operation(OperationNode::new("tick", key("tick")).on_signal("frame"))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let runner = owner
            .execution_graph(plan, NonZeroUsize::new(1).unwrap())
            .unwrap();
        let signals = runner.signal_sender();
        signals.fire("frame").unwrap();
        signals.fire("frame").unwrap();
        let report = runner.execute().await;

        assert!(report.succeeded());
        assert_eq!(report.started, 1);
        assert_eq!(invoked.load(Ordering::SeqCst), 1);
        assert!(matches!(
            signals.fire("frame"),
            Err(GraphRunError::SignalQueueClosed)
        ));
        eprintln!("research #226: two pre-run signals -> one admitted operation, no recurring run");
    });
}

#[test]
fn diagnostic_graph_size_samples_preserve_authoritative_results() {
    runtime().block_on(async {
        let owner = application_session();
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("noop"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();

        for nodes in [64usize, 512, 2_048] {
            let mut draft = ExecutionPlanDraft::new(format!("scale-{nodes}"), 1);
            for index in 0..nodes {
                draft = draft.operation(OperationNode::new(format!("op-{index}"), key("noop")));
            }
            let validation_start = Instant::now();
            let bound = draft.validate().unwrap().bind(&registry).unwrap();
            let validation_elapsed = validation_start.elapsed();
            let runner = owner
                .execution_graph(bound, NonZeroUsize::new(32).unwrap())
                .unwrap();
            let observer = runner.observer();
            let execution_start = Instant::now();
            let report = tokio::time::timeout(Duration::from_secs(15), runner.execute())
                .await
                .unwrap();
            let execution_elapsed = execution_start.elapsed();
            let snapshot_start = Instant::now();
            let snapshot = observer.snapshot();
            let snapshot_elapsed = snapshot_start.elapsed();

            assert!(report.succeeded());
            assert_eq!(report.started, nodes);
            assert_eq!(report.operations.len(), nodes);
            assert_eq!(snapshot.operations.len(), nodes);
            assert_eq!(snapshot.phase, GraphRunPhase::Finished);
            eprintln!(
                "research #226 nodes={nodes} validate_us={} execute_us={} snapshot_us={} peak={} (single CI sample, not SLA)",
                validation_elapsed.as_micros(),
                execution_elapsed.as_micros(),
                snapshot_elapsed.as_micros(),
                report.peak_in_flight
            );
        }
    });
}


#[test]
fn external_fixed_step_proposals_are_distinct_one_shot_runs_not_replay() {
    runtime().block_on(async {
        let invocations = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("tick"),
                async_operation({
                    let invocations = Arc::clone(&invocations);
                    move || {
                        let invocations = Arc::clone(&invocations);
                        async move {
                            if invocations.fetch_add(1, Ordering::SeqCst) == 0 {
                                OperationOutcome::Completed
                            } else {
                                // This represents a reported unknown result.
                                // It must not be retried by an implicit tick loop.
                                OperationOutcome::OutcomeUnknown
                            }
                        }
                    }
                }),
            )
            .unwrap();
        let plan = ExecutionPlanDraft::new("externally-clocked-tick", 1)
            .operation(OperationNode::new("tick", key("tick")))
            .validate()
            .unwrap()
            .bind(&registry)
            .unwrap();
        let owner = application_session();
        let mut clock = FixedStepAdmission::new(
            FixedStepPolicy::new(
                Duration::from_millis(10),
                NonZeroUsize::new(2).unwrap(),
                FixedStepBacklog::DiscardExcess,
            )
            .unwrap(),
        );
        let frame = clock.advance_frame(Duration::from_millis(35));
        assert_eq!(frame.eligible, 2);
        assert_eq!(frame.discarded, 1);
        assert_eq!(clock.claim(frame.eligible), 2);
        let mut reports = Vec::new();
        for _ in 0..frame.eligible {
            reports.push(
                owner
                    .execution_graph(plan.clone(), NonZeroUsize::new(1).unwrap())
                    .unwrap()
                    .execute()
                    .await,
            );
        }
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].operations["tick"], OperationState::Completed);
        assert_eq!(reports[1].operations["tick"], OperationState::OutcomeUnknown);
        assert_ne!(reports[0].identity, reports[1].identity);
        assert_eq!(clock.advance_frame(Duration::ZERO).eligible, 0);
        assert_eq!(invocations.load(Ordering::SeqCst), 2);
        eprintln!(
            "research #226: 35ms external clock -> 2 distinct one-shot runs, 1 discarded tick, no implicit replay"
        );
    });
}

#[test]
fn diagnostic_repeated_wide_and_chain_graph_samples() {
    runtime().block_on(async {
        const REPEATS: usize = 5;
        let owner = application_session();
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("noop"),
                async_operation(|| async { OperationOutcome::Completed }),
            )
            .unwrap();

        for nodes in [128usize, 512, 1_024] {
            for chain in [false, true] {
                let mut samples_us = Vec::with_capacity(REPEATS);
                for repeat in 0..REPEATS {
                    let mut draft = ExecutionPlanDraft::new(
                        format!("shape-{}-{nodes}-{repeat}", if chain { "chain" } else { "wide" }),
                        1,
                    );
                    for index in 0..nodes {
                        let mut operation = OperationNode::new(format!("op-{index}"), key("noop"));
                        if chain && index != 0 {
                            operation = operation.after(format!("op-{}", index - 1));
                        }
                        draft = draft.operation(operation);
                    }
                    let plan = draft.validate().unwrap().bind(&registry).unwrap();
                    let runner = owner
                        .execution_graph(plan, NonZeroUsize::new(32).unwrap())
                        .unwrap();
                    let started = Instant::now();
                    let report = tokio::time::timeout(Duration::from_secs(20), runner.execute())
                        .await
                        .unwrap();
                    samples_us.push(started.elapsed().as_micros());
                    assert!(report.succeeded());
                    assert_eq!(report.started, nodes);
                    assert_eq!(report.peak_in_flight, if chain { 1 } else { 32 });
                }
                samples_us.sort_unstable();
                eprintln!(
                    "research #226 repeat_shape={} nodes={} samples_us={:?} median_us={} (debug, fixed case order, not benchmark)",
                    if chain { "chain" } else { "wide" },
                    nodes,
                    samples_us,
                    samples_us[REPEATS / 2]
                );
            }
        }
    });
}

#[test]
fn diagnostic_hot_and_cold_runs_share_budget_without_fairness_assertion() {
    runtime().block_on(async {
        const HOT_NODES: usize = 64;
        const SESSION_CAPACITY: usize = 32;

        let owner = application_session();
        let hot_started = Arc::new(AtomicUsize::new(0));
        let cold_rank = Arc::new(AtomicUsize::new(0));
        let dispatch_sequence = Arc::new(AtomicUsize::new(0));
        let permits = Arc::new(Semaphore::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry
            .register(
                key("hot"),
                async_operation({
                    let hot_started = Arc::clone(&hot_started);
                    let dispatch_sequence = Arc::clone(&dispatch_sequence);
                    let permits = Arc::clone(&permits);
                    move || {
                        let hot_started = Arc::clone(&hot_started);
                        let dispatch_sequence = Arc::clone(&dispatch_sequence);
                        let permits = Arc::clone(&permits);
                        async move {
                            dispatch_sequence.fetch_add(1, Ordering::SeqCst);
                            hot_started.fetch_add(1, Ordering::SeqCst);
                            let permit = permits.acquire().await.unwrap();
                            drop(permit);
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
                    let cold_rank = Arc::clone(&cold_rank);
                    let dispatch_sequence = Arc::clone(&dispatch_sequence);
                    move || {
                        let cold_rank = Arc::clone(&cold_rank);
                        let dispatch_sequence = Arc::clone(&dispatch_sequence);
                        async move {
                            let rank = dispatch_sequence.fetch_add(1, Ordering::SeqCst) + 1;
                            cold_rank.store(rank, Ordering::SeqCst);
                            OperationOutcome::Completed
                        }
                    }
                }),
            )
            .unwrap();

        let mut hot_draft = ExecutionPlanDraft::new("hot-run", 1);
        for index in 0..HOT_NODES {
            hot_draft = hot_draft.operation(OperationNode::new(format!("hot-{index}"), key("hot")));
        }
        let hot = owner
            .execution_graph(
                hot_draft.validate().unwrap().bind(&registry).unwrap(),
                NonZeroUsize::new(SESSION_CAPACITY).unwrap(),
            )
            .unwrap();
        let hot_task = tokio::spawn(hot.execute());

        // All session slots are held by the first hot wave before the cold
        // competitor starts. The exact subsequent admission order is observed
        // but not interpreted as a fairness guarantee.
        tokio::time::timeout(Duration::from_secs(5), async {
            while hot_started.load(Ordering::SeqCst) < SESSION_CAPACITY {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let cold = owner
            .execution_graph(
                ExecutionPlanDraft::new("cold-run", 1)
                    .operation(OperationNode::new("cold", key("cold")))
                    .validate()
                    .unwrap()
                    .bind(&registry)
                    .unwrap(),
                NonZeroUsize::new(1).unwrap(),
            )
            .unwrap();
        let cold_task = tokio::spawn(cold.execute());
        tokio::task::yield_now().await;
        assert_eq!(cold_rank.load(Ordering::SeqCst), 0);

        let start = Instant::now();
        permits.add_permits(SESSION_CAPACITY);
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
        assert_eq!(hot_started.load(Ordering::SeqCst), HOT_NODES);
        let rank = cold_rank.load(Ordering::SeqCst);
        assert!((SESSION_CAPACITY + 1..=HOT_NODES + 1).contains(&rank));
        eprintln!(
            "research #226 hot={HOT_NODES} cap={SESSION_CAPACITY} cold_rank={rank} of {} post_release_us={} (one run, no fairness guarantee)",
            HOT_NODES + 1,
            start.elapsed().as_micros()
        );
    });
}
