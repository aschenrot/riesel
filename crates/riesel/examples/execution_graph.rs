//! Headless, session-owned executable workflow: two concurrent branches,
//! one dependency join, explicit signal admission, and no provider resolver.
//!
//! cargo run --locked -p riesel --features async-tcp --example execution_graph
use riesel::{
    Application, AsyncOperation, ExecutionPlanDraft, OperationKey, OperationNode, OperationOutcome,
    OperationRegistry, async_operation,
};
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn main() -> riesel::Result<()> {
    Application::new("Headless Execution Graph").run(|context| {
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
        let transitions = observer
            .changes_since(0)
            .map_err(std::io::Error::other)?;
        if transitions.truncated || transitions.events.is_empty() {
            return Err(std::io::Error::other("short example lost its transition history").into());
        }
        if joined.load(Ordering::SeqCst) != 2 {
            return Err(std::io::Error::other("expected both branches to complete").into());
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
