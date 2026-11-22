//! #226 research-only allocator accounting for actual RunningApp graph runs.
//! This is not an accepted memory limit, heap profiler or production API.
//! Each invocation is one isolated Linux test binary with --test-threads=1.
#![cfg(feature = "async-tcp")]

use riesel::{
    App, AsyncOperation, ConnectorRegistry, DeploymentConfig, ExecutionPlanDraft, GraphRunError,
    OperationKey, OperationNode, OperationOutcome, OperationRegistry, RunningApp, async_operation,
};
use std::alloc::{GlobalAlloc, Layout, System};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};

static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);
static PEAK_BYTES: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn track_alloc(size: usize) {
    let current = LIVE_BYTES.fetch_add(size, Ordering::SeqCst) + size;
    PEAK_BYTES.fetch_max(current, Ordering::SeqCst);
}

fn track_dealloc(size: usize) {
    LIVE_BYTES.fetch_sub(size, Ordering::SeqCst);
}

// The wrapped operations call the System allocator exactly once and only
// adjust counts on success. This is test-local diagnostic instrumentation.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            track_alloc(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            track_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        track_dealloc(layout.size());
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let next = unsafe { System.realloc(ptr, layout, new_size) };
        if !next.is_null() {
            if new_size >= layout.size() {
                track_alloc(new_size - layout.size());
            } else {
                track_dealloc(layout.size() - new_size);
            }
        }
        next
    }
}

fn owner() -> RunningApp {
    App::from_schema(
        r#"{"version":1,"app":{"name":"ReadyIndexAllocatorResearch"},"models":{},"requires":[]}"#,
    )
    .unwrap()
    .start(
        &DeploymentConfig::new(Default::default()),
        &ConnectorRegistry::new(),
    )
    .unwrap()
}

fn key(name: &str) -> OperationKey {
    OperationKey::new(name, 1).unwrap()
}

#[test]
fn sixteen_live_runs_have_accounted_retained_allocator_cost() {
    let owner = owner();
    let mut registry = OperationRegistry::<AsyncOperation>::new();
    registry
        .register(
            key("noop"),
            async_operation(|| async { OperationOutcome::Completed }),
        )
        .unwrap();

    for nodes in [64usize, 512, 2_048] {
        for chain in [false, true] {
            let mut draft = ExecutionPlanDraft::new(
                format!("alloc-{}-{nodes}", if chain { "chain" } else { "wide" }),
                1,
            );
            for index in 0..nodes {
                let mut op = OperationNode::new(format!("op-{index}"), key("noop"));
                if chain && index > 0 {
                    op = op.after(format!("op-{}", index - 1));
                }
                draft = draft.operation(op);
            }
            let plan = draft.validate().unwrap().bind(&registry).unwrap();
            let mut running = Vec::with_capacity(16);
            let before = LIVE_BYTES.load(Ordering::SeqCst);
            PEAK_BYTES.store(before, Ordering::SeqCst);
            let mut first_bytes = 0;
            for i in 0..16 {
                running.push(
                    owner
                        .execution_graph(plan.clone(), NonZeroUsize::new(1).unwrap())
                        .unwrap(),
                );
                if i == 0 {
                    first_bytes = LIVE_BYTES.load(Ordering::SeqCst).saturating_sub(before);
                }
            }
            let retained = LIVE_BYTES.load(Ordering::SeqCst).saturating_sub(before);
            let peak = PEAK_BYTES.load(Ordering::SeqCst).saturating_sub(before);
            assert!(matches!(
                owner.execution_graph(plan.clone(), NonZeroUsize::new(1).unwrap()),
                Err(GraphRunError::SessionRunLimitExceeded { limit: 16 })
            ));
            drop(running);
            let after_drop = LIVE_BYTES.load(Ordering::SeqCst);
            assert!(retained >= first_bytes);
            eprintln!(
                "research #226 alloc nodes={nodes} shape={} index={} one_live_bytes={} sixteen_live_bytes={} creation_peak_bytes={} after_drop_delta_bytes={} (test-local allocator accounting; no RSS/SLA)",
                if chain { "chain" } else { "wide" },
                std::env::var_os("RIESEL_RESEARCH_READY_INDEX").is_some(),
                first_bytes,
                retained,
                peak,
                after_drop.saturating_sub(before),
            );
        }
    }
}
