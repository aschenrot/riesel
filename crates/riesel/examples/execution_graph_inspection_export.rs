//! Headless, session-owned executable workflow: two concurrent branches,
//! one dependency join, explicit signal admission, and no provider resolver.
//!
//! Research-only file handoff for a separate read-only graph GUI.
//!
//! cargo run --locked -p riesel --features async-tcp --example execution_graph_inspection_export -- graph.json
use riesel::{
    Application, AsyncOperation, ExecutionPlanDraft, OperationKey, OperationNode, OperationOutcome,
    OperationRegistry, OperationState, async_operation,
};
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn state_name(state: &OperationState) -> &'static str {
    match state {
        OperationState::Pending => "pending",
        OperationState::Running => "running",
        OperationState::Completed => "completed",
        OperationState::Failed(_) => "failed",
        OperationState::Cancelled => "cancelled",
        OperationState::OutcomeUnknown => "unknown",
        OperationState::Blocked => "blocked",
    }
}

fn main() -> riesel::Result<()> {
    let output = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .ok_or_else(|| std::io::Error::other("expected output JSON path"))?;
    Application::new("Headless Execution Graph").run(move |context| {
        let joined = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let mut registry = OperationRegistry::<AsyncOperation>::new();

        for name in ["left", "right"] {
            let barrier = Arc::clone(&barrier);
            let joined = Arc::clone(&joined);
            registry.register(
                OperationKey::new(name, 1)?,
                async_operation(move || {
                    let barrier = Arc::clone(&barrier);
                    let joined = Arc::clone(&joined);
                    async move {
                        // Both branches must be executing to pass this gate.
                        barrier.wait().await;
                        joined.fetch_add(1, Ordering::SeqCst);
                        OperationOutcome::Completed
                    }
                }),
            )?;
        }

        registry.register(
            OperationKey::new("join", 1)?,
            async_operation({
                let joined = Arc::clone(&joined);
                move || {
                    let joined = Arc::clone(&joined);
                    async move {
                        if joined.load(Ordering::SeqCst) == 2 {
                            OperationOutcome::Completed
                        } else {
                            OperationOutcome::Failed("dependency join executed prematurely".into())
                        }
                    }
                }
            }),
        )?;

        let plan = ExecutionPlanDraft::new("example.session-workflow", 1)
            .operation(OperationNode::new("left", OperationKey::new("left", 1)?).on_signal("start"))
            .operation(
                OperationNode::new("right", OperationKey::new("right", 1)?).on_signal("start"),
            )
            .operation(
                OperationNode::new("join", OperationKey::new("join", 1)?)
                    .after("left")
                    .after("right"),
            )
            .validate()?
            .bind(&registry)?;

        // Construct within the actual application session. The application
        // owns startup and teardown; the caller owns and awaits its Tokio work.
        let inspected_plan = plan.plan().clone();
        let runner = context.execution_graph(plan, NonZeroUsize::new(2).unwrap())?;
        let signal = runner.signal_sender();
        let observer = runner.observer();
        let initial = observer.snapshot();
        signal.fire("start")?;
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let report = executor.block_on(runner.execute());

        if !report.succeeded() {
            return Err(std::io::Error::other(format!(
                "graph did not complete: {:?}",
                report.operations
            ))
            .into());
        }
        let final_state = observer.snapshot();
        if final_state.identity != report.identity
            || final_state.phase != riesel::GraphRunPhase::Finished
            || final_state.operations.len() != report.operations.len()
        {
            return Err(std::io::Error::other("observed state diverged from run report").into());
        }
        let transitions = observer.changes_since(0).map_err(std::io::Error::other)?;
        if transitions.truncated || transitions.events.is_empty() {
            return Err(std::io::Error::other("short example lost its transition history").into());
        }
        if joined.load(Ordering::SeqCst) != 2 {
            return Err(std::io::Error::other("expected both branches to complete").into());
        }
        // Research-only projection built from the real validated plan and
        // authoritative run snapshots. No exporter, remote principal,
        // network listener, executable document or public wire ABI exists.
        let nodes: Vec<_> = inspected_plan
            .description()
            .operations()
            .iter()
            .map(|node| {
                let before = initial.operations.get(node.id());
                let after = final_state.operations.get(node.id());
                serde_json::json!({
                    "id": node.id(),
                    "depends_on": node.dependencies(),
                    "operation": node.binding().name(),
                    "operation_version": node.binding().version(),
                    "initial": before.map(|state| state_name(&state.state)),
                    "final": after.map(|state| state_name(&state.state)),
                    "attempt": after.map(|state| state.attempt).unwrap_or(0),
                    "error": after.and_then(|state| match &state.state {
                        OperationState::Failed(message) => Some(message.as_str()),
                        _ => None,
                    }),
                    "confirmed_prefixes": after.map(|state| &state.confirmed_prefixes),
                })
            })
            .collect();
        let document = serde_json::json!({
            "schema": "riesel.research.graph-inspection.v1",
            "plan_id": report.plan_id,
            "revision": report.revision,
            "session_generation": report.identity.session_generation().to_string(),
            "run_sequence": report.identity.run_sequence(),
            "phase": format!("{:?}", final_state.phase),
            "session_expired": final_state.session_expired,
            "initial_sequence": initial.latest_sequence,
            "latest_sequence": final_state.latest_sequence,
            "trace_truncated": transitions.truncated,
            "nodes": nodes,
            "transitions": transitions.events.iter().map(|event| {
                serde_json::json!({
                    "sequence": event.sequence,
                    "change": format!("{:?}", event.change),
                })
            }).collect::<Vec<_>>(),
        });
        std::fs::write(&output, serde_json::to_vec_pretty(&document)?)?;
        println!(
            "execution graph {} revision {}: {} operations completed",
            report.plan_id,
            report.revision,
            report.operations.len()
        );
        println!(
            "session={} run={} initial-seq={} final-seq={} transitions={}",
            report.identity.session_generation(),
            report.identity.run_sequence(),
            initial.latest_sequence,
            final_state.latest_sequence,
            transitions.events.len()
        );
        Ok(())
    })
}
