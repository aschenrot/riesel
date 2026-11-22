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
use std::time::Duration;

#[path = "../../../research/gui-boundary/live_server.rs"]
mod live_server;
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

enum Mode {
    File(std::path::PathBuf),
    Live(u64),
}

fn main() -> riesel::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.as_slice() {
        [flag] if flag == "--live" => Mode::Live(45),
        [flag, seconds] if flag == "--live" => match seconds.parse::<u64>() {
            Ok(value) if (1..=600).contains(&value) => Mode::Live(value),
            _ => {
                eprintln!("live duration must be 1..=600 seconds");
                std::process::exit(2);
            }
        },
        [path] if !path.starts_with("--") => Mode::File(path.into()),
        _ => {
            eprintln!(
                "usage: execution_graph_inspection_export OUTPUT.json | --live [HOLD_SECONDS]"
            );
            std::process::exit(2);
        }
    };
    let live = matches!(mode, Mode::Live(_));
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
                        if live {
                            tokio::time::sleep(Duration::from_secs(3)).await;
                        }
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
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let report = match &mode {
            Mode::File(_) => {
                signal.fire("start")?;
                executor.block_on(runner.execute())
            }
            Mode::Live(seconds) => {
                let live_observer = observer.clone();
                let live_plan = inspected_plan.clone();
                let live_initial = initial.clone();
                let source: live_server::SnapshotSource = Arc::new(move || {
                    inspection_document(&live_plan, &live_initial, &live_observer)
                });
                executor.block_on(live_server::run(runner, signal, source, *seconds))?
            }
        };

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
        if let Mode::File(output) = &mode {
            let document = inspection_document(&inspected_plan, &initial, &observer)?;
            std::fs::write(output, serde_json::to_vec_pretty(&document)?)?;
        }
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

fn inspection_document(
    plan: &riesel::ValidatedExecutionPlan,
    initial: &riesel::GraphRunSnapshot,
    observer: &riesel::GraphRunObserver,
) -> std::io::Result<serde_json::Value> {
    // This is a research-only, read-only projection; neither a supported wire
    // protocol nor a credential. Snapshot wins over the bounded event history.
    let snapshot = observer.snapshot();
    let changes = observer.changes_since(0).map_err(std::io::Error::other)?;
    let nodes: Vec<_> = plan
        .description()
        .operations()
        .iter()
        .map(|node| {
            let before = initial.operations.get(node.id());
            let now = snapshot.operations.get(node.id());
            serde_json::json!({
                "id": node.id(),
                "depends_on": node.dependencies(),
                "operation": node.binding().name(),
                "operation_version": node.binding().version(),
                "initial": before.map(|state| state_name(&state.state)),
                "final": now.map(|state| state_name(&state.state)),
                "attempt": now.map(|state| state.attempt).unwrap_or(0),
                "error": now.and_then(|state| match &state.state {
                    OperationState::Failed(message) => Some(message.as_str()),
                    _ => None,
                }),
                "confirmed_prefixes": now.map(|state| &state.confirmed_prefixes),
            })
        })
        .collect();
    Ok(serde_json::json!({
        "schema": "riesel.research.graph-inspection.v1",
        "plan_id": snapshot.identity.plan_id(),
        "revision": snapshot.identity.revision(),
        "session_generation": snapshot.identity.session_generation().to_string(),
        "run_sequence": snapshot.identity.run_sequence(),
        "phase": format!("{:?}", snapshot.phase),
        "session_expired": snapshot.session_expired,
        "initial_sequence": initial.latest_sequence,
        "latest_sequence": snapshot.latest_sequence,
        "trace_truncated": changes.truncated,
        "nodes": nodes,
        "transitions": changes.events.iter()
            .filter(|event| event.sequence <= snapshot.latest_sequence)
            .map(|event| serde_json::json!({
                "sequence": event.sequence,
                "change": format!("{:?}", event.change),
            }))
            .collect::<Vec<_>>(),
    }))
}
