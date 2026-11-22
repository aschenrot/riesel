//! Headless cooperative graph control at an outer Riesel-owned domain tick.
//!
//! The tick gate does not suspend running Rust callbacks or step ECS internals.
//! An unrelated async branch remains eligible while simulation ticks are paused.
//!
//! cargo run --locked -p riesel --features async-tcp --example execution_control
use riesel::{
    Application, AsyncOperation, ExecutionPlanDraft, GraphControlMode, GraphControlTarget,
    OperationKey, OperationNode, OperationOutcome, OperationRegistry, OperationState,
    async_operation,
};
use std::num::NonZeroUsize;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

fn main() -> riesel::Result<()> {
    Application::new("Controlled Execution Graph").run(|context| {
        let ticks = Arc::new(AtomicUsize::new(0));
        let independent = Arc::new(AtomicUsize::new(0));
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry.register(
            OperationKey::new("simulation.tick", 1)?,
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
        )?;
        registry.register(
            OperationKey::new("background", 1)?,
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
        )?;
        let plan = ExecutionPlanDraft::new("example.controlled-ticks", 1)
            .operation(
                OperationNode::new("tick-one", OperationKey::new("simulation.tick", 1)?)
                    .domain_tick("world"),
            )
            .operation(
                OperationNode::new("tick-two", OperationKey::new("simulation.tick", 1)?)
                    .after("tick-one")
                    .domain_tick("world"),
            )
            .operation(OperationNode::new(
                "independent",
                OperationKey::new("background", 1)?,
            ))
            .validate()?
            .bind(&registry)?;

        let runner = context.execution_graph(plan, NonZeroUsize::new(3).unwrap())?;
        let observer = runner.observer();
        let controller = runner.controller();
        let target = GraphControlTarget::RieselDomainTicks("world".to_owned());
        controller.pause(target.clone())?;

        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let report = executor.block_on(async {
            let task = tokio::spawn(runner.execute());
            tokio::time::timeout(Duration::from_secs(3), async {
                while independent.load(Ordering::SeqCst) == 0 {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map_err(std::io::Error::other)?;
            if ticks.load(Ordering::SeqCst) != 0 {
                return Err(std::io::Error::other("paused tick dispatched unexpectedly"));
            }

            controller
                .step(target.clone())
                .map_err(std::io::Error::other)?;
            tokio::time::timeout(Duration::from_secs(3), async {
                while observer.snapshot().operations["tick-one"].state != OperationState::Completed
                {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map_err(std::io::Error::other)?;
            if ticks.load(Ordering::SeqCst) != 1
                || observer.snapshot().operations["tick-two"].state != OperationState::Pending
                || observer.snapshot().controls[&target] != GraphControlMode::Paused
            {
                return Err(std::io::Error::other("single domain step was not enforced"));
            }

            controller
                .step(target.clone())
                .map_err(std::io::Error::other)?;
            tokio::time::timeout(Duration::from_secs(3), task)
                .await
                .map_err(std::io::Error::other)?
                .map_err(std::io::Error::other)
        })?;
        if !report.succeeded() || ticks.load(Ordering::SeqCst) != 2 {
            return Err(std::io::Error::other("controlled graph did not complete").into());
        }
        let snapshot = observer.snapshot();
        if snapshot.identity != report.identity
            || snapshot.controls[&target] != GraphControlMode::Paused
            || independent.load(Ordering::SeqCst) != 1
        {
            return Err(std::io::Error::other("control snapshot does not match run").into());
        }
        println!(
            "run={} revision={} committed outer ticks={} independent async branches={}",
            report.identity.run_sequence(),
            report.identity.revision(),
            ticks.load(Ordering::SeqCst),
            independent.load(Ordering::SeqCst)
        );
        Ok(())
    })
}
