//! Headless repeated fixed-step application work with one clock owner.
//! The frame elapsed times are synthetic for repeatable demonstration:
//! a host can pass monotonic elapsed time from its actual event loop.
//!
//! cargo run --locked -p riesel --features async-tcp --example execution_fixed_step
use riesel::{
    Application, AsyncOperation, ExecutionPlanDraft, FixedStepAdmission, FixedStepBacklog,
    FixedStepPolicy, OperationKey, OperationNode, OperationOutcome, OperationRegistry,
    async_operation_with_effects,
};
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

fn main() -> riesel::Result<()> {
    Application::new("Headless Fixed Step").run(|context| {
        let ticks = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry.register(
            OperationKey::new("simulation.tick", 1)?,
            async_operation_with_effects({
                let ticks = Arc::clone(&ticks);
                move |effects| {
                    let ticks = Arc::clone(&ticks);
                    async move {
                        // Substitute a typed capability handle or a delegated
                        // domain-thread command here; the graph does not own
                        // an ECS world's internal system schedule.
                        ticks.fetch_add(1, Ordering::SeqCst);
                        // Only report after this local owner completed its
                        // mutation. A dispatched remote command without an
                        // acknowledgement would remain outcome-unknown.
                        effects.confirm_prefix("simulation.ticks", 1).unwrap();
                        OperationOutcome::Completed
                    }
                }
            }),
        )?;
        let bound = ExecutionPlanDraft::new("example.repeated-ticks", 1)
            .operation(
                OperationNode::new("tick", OperationKey::new("simulation.tick", 1)?)
                    .domain_tick("simulation"),
            )
            .validate()?
            .bind(&registry)?;
        let mut admission = FixedStepAdmission::new(FixedStepPolicy::new(
            Duration::from_millis(10),
            NonZeroUsize::new(2).unwrap(),
            FixedStepBacklog::RetainUpTo(NonZeroUsize::new(1).unwrap()),
        )?);
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;

        for elapsed in [Duration::from_millis(35), Duration::ZERO, Duration::ZERO] {
            let frame = admission.advance_frame(elapsed);
            println!(
                "frame: proposed={} deferred={} discarded={}",
                frame.eligible, frame.deferred, frame.discarded
            );
            for _ in 0..frame.eligible {
                // Claim one *proposal*, not a confirmed external effect.
                assert_eq!(admission.claim(1), 1);
                let runner =
                    context.execution_graph(bound.clone(), NonZeroUsize::new(1).unwrap())?;
                let observer = runner.observer();
                let report = executor.block_on(runner.execute());
                if report.confirmed_prefixes["tick"]["simulation.ticks"] != 1
                    || observer.snapshot().operations["tick"].confirmed_prefixes
                        ["simulation.ticks"]
                        != 1
                {
                    return Err(std::io::Error::other(
                        "authoritatively confirmed tick is missing from report or snapshot",
                    )
                    .into());
                }
                if !report.succeeded() {
                    return Err(std::io::Error::other(format!(
                        "tick outcome is not successful: {:?}",
                        report.operations
                    ))
                    .into());
                }
            }
        }
        if ticks.load(Ordering::SeqCst) != 3 {
            return Err(std::io::Error::other("expected three confirmed example ticks").into());
        }
        println!(
            "completed {} separate one-shot tick runs",
            ticks.load(Ordering::SeqCst)
        );
        Ok(())
    })
}
