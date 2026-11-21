use super::*;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn noop() -> Operation {
    operation(|| async { Ok(()) })
}

#[test]
fn rejects_invalid_graph_before_executing_code() {
    assert!(matches!(Plan::validate(vec![Node::new("", &[], vec![], noop())]), Err(InvalidPlan::EmptyId)));
    assert!(matches!(Plan::validate(vec![
        Node::new("a", &[], vec![], noop()), Node::new("a", &[], vec![], noop())
    ]), Err(InvalidPlan::DuplicateId("a"))));
    assert!(matches!(Plan::validate(vec![
        Node::new("a", &["missing"], vec![], noop())
    ]), Err(InvalidPlan::MissingDependency("missing"))));
    assert!(matches!(Plan::validate(vec![
        Node::new("a", &["b"], vec![], noop()),
        Node::new("b", &["a"], vec![], noop()),
    ]), Err(InvalidPlan::Cycle)));
    assert!(matches!(Plan::validate(vec![
        Node::new("a", &[], vec![], noop()),
        Node::new("b", &["a", "a"], vec![], noop())
    ]), Err(InvalidPlan::DuplicateDependency("a"))));
}

#[tokio::test(flavor = "current_thread")]
async fn parallel_eligibility_join_and_access_exclusion() {
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let parallel = Arc::new(Plan::validate(vec![
        Node::new("begin", &[], vec![], noop()),
        Node::new("left", &["begin"], vec![Access::Read("resource")], {
            let b = Arc::clone(&barrier);
            operation(move || {
                let b = Arc::clone(&b);
                async move { b.wait().await; Ok(()) }
            })
        }),
        Node::new("right", &["begin"], vec![Access::Read("resource")], {
            let b = Arc::clone(&barrier);
            operation(move || {
                let b = Arc::clone(&b);
                async move { b.wait().await; Ok(()) }
            })
        }),
        Node::new("join", &["left", "right"], vec![], noop())
    ]).unwrap());
    assert_eq!(parallel.describe()[3].1, vec!["left", "right"]);
    let mut run = Run::new(parallel, 100, 10, 2, Trace::new(true, 100));
    tokio::time::timeout(Duration::from_secs(2), run.advance(usize::MAX)).await.unwrap();
    assert_eq!(run.state("join"), Some(Status::Completed));
    assert_eq!(run.peak_in_flight, 2);
    let events: Vec<_> = run.trace.retained.iter().collect();
    assert!(events.iter().position(|e| e.node == "begin" && e.status == Status::Completed).unwrap()
        < events.iter().position(|e| e.node == "left" && e.status == Status::Running).unwrap());

    let exclusive = Arc::new(Plan::validate(vec![
        Node::new("a", &[], vec![Access::Write("world")], operation(|| async {
            tokio::time::sleep(Duration::from_millis(2)).await;
            Ok(())
        })),
        Node::new("b", &[], vec![Access::Read("world")], operation(|| async {
            tokio::time::sleep(Duration::from_millis(2)).await;
            Ok(())
        })),
    ]).unwrap());
    let mut run = Run::new(exclusive, 100, 11, 2, Trace::new(false, 0));
    run.advance(usize::MAX).await;
    assert_eq!(run.peak_in_flight, 1);
    assert_eq!(run.state("b"), Some(Status::Completed));
}

#[tokio::test(flavor = "current_thread")]
async fn step_operation_failure_cancel_and_trace_loss_are_explicit() {
    let plan = Arc::new(Plan::validate(vec![
        Node::new("one", &[], vec![], noop()),
        Node::new("bad", &["one"], vec![], operation(|| async { Err("expected failure") })),
        Node::new("unreached", &["bad"], vec![], noop()),
    ]).unwrap());
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    drop(receiver);
    let mut trace = Trace::new(true, 2);
    trace.subscribe(sender);
    let mut run = Run::new(Arc::clone(&plan), 7, 9, 2, trace);
    run.advance(1).await;
    assert_eq!(run.state("one"), Some(Status::Completed));
    assert_eq!(run.state("bad"), Some(Status::Pending));
    run.advance(1).await;
    assert_eq!(run.state("bad"), Some(Status::Failed));
    assert_eq!(run.state("unreached"), Some(Status::Blocked));
    assert_eq!(run.launched, 2);
    assert!(run.trace.evicted > 0);
    assert_eq!(run.trace.subscriber_disconnected, 1);
    assert_eq!(run.trace.retained.len(), 2);
    let last = run.trace.retained.back().unwrap();
    assert_eq!((last.session, last.run, last.node), (7, 9, "unreached"));
    let authoritative = run.snapshot();
    assert_eq!(authoritative.len(), 3);
    assert_ne!(run.trace.retained.len(), authoritative.len());

    let mut cancelled = Run::new(plan, 7, 10, 2, Trace::new(true, 16));
    cancelled.cancel();
    cancelled.advance(usize::MAX).await;
    assert_eq!(cancelled.launched, 0);
    assert!(cancelled.snapshot().iter().all(|(_, status)| *status == Status::Cancelled));
}

#[derive(Default)]
struct World {
    tick: u64,
    input: u64,
    position: u64,
    snapshots: Vec<(u64, u64)>,
}

#[tokio::test(flavor = "current_thread")]
async fn fixed_tick_game_with_nested_domain_operation_and_bounded_catch_up() {
    let world = Arc::new(Mutex::new(World::default()));
    let input = Arc::clone(&world);
    let simulation = Arc::clone(&world);
    let snapshot = Arc::clone(&world);
    // The domain owns its world. Riesel's hypothetical runner sees only three
    // outer operations, never the internal ECS/system borrow schedule.
    let plan = Arc::new(Plan::validate(vec![
        Node::new("input", &[], vec![Access::Write("world")], operation(move || {
            let w = Arc::clone(&input);
            async move { w.lock().unwrap().input = 2; Ok(()) }
        })),
        Node::new("simulation", &["input"], vec![Access::Write("world")], operation(move || {
            let w = Arc::clone(&simulation);
            async move {
                let mut world = w.lock().unwrap();
                world.tick += 1;
                world.position += world.input;
                Ok(())
            }
        })),
        Node::new("snapshot", &["simulation"], vec![Access::Read("world")], operation(move || {
            let w = Arc::clone(&snapshot);
            async move {
                let mut world = w.lock().unwrap();
                let current = (world.tick, world.position);
                world.snapshots.push(current);
                Ok(())
            }
        })),
    ]).unwrap());
    let mut next = 0;
    // First display frame consumes no simulation ticks.
    for proposed_ticks in [0_usize, 1, 4] {
        for _ in 0..proposed_ticks.min(2) {
            next += 1;
            let mut tick = Run::new(Arc::clone(&plan), 42, next, 3, Trace::new(false, 0));
            tick.advance(1).await;
            assert_eq!(tick.state("simulation"), Some(Status::Pending));
            tick.advance(1).await;
            assert_eq!(tick.state("simulation"), Some(Status::Completed));
            tick.advance(1).await;
            assert_eq!(tick.state("snapshot"), Some(Status::Completed));
        }
    }
    let result = world.lock().unwrap();
    assert_eq!(result.tick, 3);
    assert_eq!(result.position, 6);
    assert_eq!(result.snapshots, vec![(1, 2), (2, 4), (3, 6)]);
}

#[tokio::test(flavor = "current_thread")]
async fn non_game_signal_branch_join_and_ambiguous_side_effect_is_not_retried() {
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let external_commits = Arc::new(AtomicUsize::new(0));
    let side_effect = Arc::clone(&external_commits);
    let plan = Arc::new(Plan::validate(vec![
        Node::new("on_event", &[], vec![], noop()).triggered("message"),
        Node::new("branch_a", &["on_event"], vec![], {
            let b = Arc::clone(&barrier);
            operation(move || { let b = Arc::clone(&b); async move { b.wait().await; Ok(()) } })
        }),
        Node::new("branch_b", &["on_event"], vec![], {
            let b = Arc::clone(&barrier);
            operation(move || { let b = Arc::clone(&b); async move {
                tokio::time::sleep(Duration::from_millis(1)).await;
                b.wait().await;
                Ok(())
            } })
        }),
        Node::new("join", &["branch_a", "branch_b"], vec![], noop()),
        Node::new("external_commit", &["join"], vec![], operation(move || {
            let c = Arc::clone(&side_effect);
            async move {
                c.fetch_add(1, Ordering::SeqCst);
                // Lost acknowledgment: effect may have occurred.
                Err("unknown external outcome")
            }
        })),
        Node::new("no_implicit_retry", &["external_commit"], vec![], noop()),
    ]).unwrap());
    let mut run = Run::new(plan, 88, 1, 2, Trace::new(true, 5));
    run.advance(usize::MAX).await;
    assert!(run.snapshot().iter().all(|(_, s)| *s == Status::Pending));
    run.fire("message");
    tokio::time::timeout(Duration::from_secs(2), run.advance(usize::MAX)).await.unwrap();
    assert_eq!(run.state("join"), Some(Status::Completed));
    assert_eq!(run.state("external_commit"), Some(Status::Failed));
    assert_eq!(run.state("no_implicit_retry"), Some(Status::Blocked));
    assert_eq!(external_commits.load(Ordering::SeqCst), 1);
    assert_eq!(run.peak_in_flight, 2);
    assert!(run.trace.evicted > 0);
}

#[test]
fn typed_riesel_capability_runs_inside_existing_application_session() {
    use riesel::{Application, AsyncCapability};
    use riesel_counter::AsyncCounterContract;
    use riesel_counter_component::AsyncMemoryCounter;

    let application = Application::new("SchedulerResearch")
        .async_handler_with_aliases(
            |counter: AsyncCapability<AsyncCounterContract>| async move {
                let capability = counter.clone();
                let plan = Arc::new(Plan::validate(vec![
                    Node::new("plain_rust", &[], vec![], noop()),
                    Node::new("typed_capability", &["plain_rust"], vec![Access::Write("counter")], operation(move || {
                        let capability = capability.clone();
                        async move {
                            capability.add(3).await.map(|_| ()).map_err(|_| "typed capability invocation failed")
                        }
                    })),
                ]).unwrap());
                let mut run = Run::new(plan, 1, 1, 2, Trace::new(true, 32));
                run.advance(usize::MAX).await;
                assert_eq!(run.state("typed_capability"), Some(Status::Completed));
                assert_eq!(counter.value().await.expect("capability must remain valid"), 3);
                Ok::<(), std::io::Error>(())
            },
            ["counter"],
        )
        .unwrap()
        .component::<AsyncMemoryCounter>()
        .unwrap();
    application.run().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn bounded_instrumentation_overhead_observation_not_a_performance_sla() {
    let nodes: Vec<_> = (0..16).map(|_| Node::new("same", &[], vec![], noop())).collect();
    // Bench must use unique IDs. Construct a fixed, explicit uniform workload.
    assert!(Plan::validate(nodes).is_err());
    let ids = ["n00", "n01", "n02", "n03", "n04", "n05", "n06", "n07",
        "n08", "n09", "n10", "n11", "n12", "n13", "n14", "n15"];
    let plan = Arc::new(Plan::validate(ids.iter().map(|&id| Node::new(id, &[], vec![], noop())).collect()).unwrap());
    for enabled in [false, true] {
        let start = Instant::now();
        for round in 0..80 {
            let mut run = Run::new(Arc::clone(&plan), 101, round, 4, Trace::new(enabled, 64));
            run.advance(usize::MAX).await;
            assert_eq!(run.launched, 16);
            assert_eq!(run.trace.retained.len(), if enabled { 32 } else { 0 });
        }
        eprintln!("research-only debug-build elapsed: instrumentation_enabled={enabled}, operations=1280, elapsed_ns={}", start.elapsed().as_nanos());
    }
}
