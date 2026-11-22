//! Research #187: observed S2-only production graph-runner cost.
//! Actual Application/session/ExecutionPlanDraft/BoundExecutionPlan/run path.
//! No S3 dataflow code; no claims about scheduler SLA or GPU/real-time systems.
#![forbid(unsafe_code)]

use riesel::{
    Application, AsyncOperation, ExecutionPlanDraft, OperationKey, OperationNode,
    OperationOutcome, OperationRegistry, async_operation,
};
use std::hint::black_box;
use std::io;
use std::num::NonZeroUsize;
use std::time::Instant;

fn main() -> riesel::Result<()> {
    println!("GRAPH_COSTS method=median_of_5_batches sample=8_per_batch warmup=2 actual_riesel_S2_no_data_edges single_application_session");
    Application::new("Research #187 S2 Control Baseline").run(|context| {
        let key = OperationKey::new("research.noop_async", 1)?;
        let mut registry = OperationRegistry::<AsyncOperation>::new();
        registry.register(key.clone(), async_operation(|| async {
            OperationOutcome::Completed
        }))?;

        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all().build()?;

        for (nodes, chained) in [(1usize, false), (16, false), (16, true), (64, false), (64, true)] {
            let mut draft = ExecutionPlanDraft::new(
                format!("research.187.s2.{}.{}", nodes, if chained { "chain" } else { "fanout" }),
                1,
            );
            for i in 0..nodes {
                let mut node = OperationNode::new(format!("node{i}"), key.clone());
                if chained && i > 0 {
                    node = node.after(format!("node{}", i-1));
                }
                draft = draft.operation(node);
            }
            let bound = draft.validate()?.bind(&registry)?;
            let mut times = [0u128; 5];
            for (batch, slot) in times.iter_mut().enumerate() {
                let start = Instant::now();
                for _ in 0..10 {
                    let runner = context.execution_graph(
                        bound.clone(),
                        NonZeroUsize::new(64).unwrap(),
                    )?;
                    let report = executor.block_on(runner.execute());
                    if !report.succeeded() || report.operations.len() != nodes {
                        return Err(io::Error::other(format!(
                            "S2 runner failed: batch={batch}, report={:?}", report.operations,
                        )).into());
                    }
                    black_box(report.identity);
                }
                *slot = start.elapsed().as_nanos() / 10;
            }
            times.sort_unstable();
            println!(
                "GRAPH_COSTS nodes={} shape={} median_total_ns={} median_per_node_ns={} batches=5 runs_per_batch=10",
                nodes,
                if chained { "chain" } else { "independent" },
                times[2],
                times[2] / nodes as u128,
            );
        }
        Ok(())
    })
}
