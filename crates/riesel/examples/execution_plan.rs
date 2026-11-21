//! Headless versioned operation-plan authoring and inspection.
//!
//! This example does not start a provider, session or graph runner:
//! `cargo run -p riesel --example execution_plan`
use riesel::{AccessMode, ExecutionPlanDraft, OperationKey, OperationNode, OperationRegistry, PlanRevisionCatalog};
use std::sync::Arc;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let print = OperationKey::new("example.print", 1)?;
    let write = OperationKey::new("example.write", 1)?;
    let mut registry = OperationRegistry::<dyn Fn() + Send + Sync>::new();
    registry.register(print.clone(), Arc::new(|| println!("print ran")))?;
    registry.register(write.clone(), Arc::new(|| println!("write ran")))?;

    let draft = ExecutionPlanDraft::new("example.headless", 1)
        .operation(OperationNode::new("write", write).after("print")
            .access("records", AccessMode::Write))
        .operation(OperationNode::new("print", print).on_signal("ready"));
    let validated = draft.validate()?;
    let bound = validated.bind(&registry)?;
    let mut catalog = PlanRevisionCatalog::new();
    catalog.publish(validated)?;

    // Read-only metadata: no registered Rust function runs during validation.
    println!("{}", serde_json::to_string_pretty(bound.plan().description())?);
    for operation in bound.plan().dependency_order() {
        println!("validated operation: {}", operation.id());
    }
    // Only an explicit caller invocation executes code in this foundation.
    // The session-owned graph runner is tracked separately in Issue #112.
    Ok(())
}
