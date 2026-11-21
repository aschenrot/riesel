//! Public SDK proof: plan validation and code binding are headless and do not
//! execute an operation, open a provider, or require a GUI/Tokio runtime.
use riesel::{ExecutionPlanDraft, OperationKey, OperationNode, OperationRegistry};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[test]
fn headless_sdk_authoring_inspects_before_running_any_rust_handler() {
    let visits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&visits);
    let binding = OperationKey::new("example.record", 1).unwrap();

    let mut registered = OperationRegistry::<dyn Fn() + Send + Sync>::new();
    registered.register(binding.clone(), Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
    })).unwrap();

    let plan = ExecutionPlanDraft::new("example.headless", 1)
        .operation(OperationNode::new("record", binding))
        .validate()
        .unwrap();

    let description = serde_json::to_value(plan.description()).unwrap();
    assert_eq!(description["id"], "example.headless");
    assert_eq!(description["revision"], 1);

    let bound = plan.bind(&registered).unwrap();
    assert_eq!(bound.plan().dependency_order().count(), 1);
    assert_eq!(visits.load(Ordering::SeqCst), 0);

    // Invoking a handler is an explicit, separate action; the upcoming
    // session-owned graph runner will own this invocation boundary.
    bound.handler("record").unwrap()();
    assert_eq!(visits.load(Ordering::SeqCst), 1);
}
