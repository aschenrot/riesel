//! Issue #83 Gate H: SAME mixed-domain workload with two admission owners.
//! This is a research-only authority differential, not an SDK/API decision.
//! It reuses the actual pinned RunenECS owner and the single generic Riesel
//! research graph runner; neither invents a second ECS World authority.
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const GENERATION: u64 = 903;
const PROPOSED_TICKS: usize = 4;
const CATCH_UP_CAP: usize = 2;
const INPUT: i32 = 2;

fn workflow_nodes(joined: Arc<AtomicUsize>) -> Vec<Node> {
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let left = Arc::clone(&barrier);
    let right = barrier;
    vec![
        Node::new("message", &[], vec![], noop()).triggered("message"),
        Node::new("async_left", &["message"], vec![], operation(move || {
            let wait = Arc::clone(&left);
            async move {
                wait.wait().await;
                Ok(())
            }
        })),
        Node::new("async_right", &["message"], vec![], operation(move || {
            let wait = Arc::clone(&right);
            async move {
                wait.wait().await;
                Ok(())
            }
        })),
        Node::new("async_join", &["async_left", "async_right"], vec![], operation(move || {
            let joined = Arc::clone(&joined);
            async move {
                joined.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })),
    ]
}

fn expected_first() -> EcsSnapshot {
    EcsSnapshot { tick: 1, positions: vec![2, 12], invoker_thread_calls: 1 }
}

fn expected_second() -> EcsSnapshot {
    EcsSnapshot { tick: 2, positions: vec![4, 12, 14], invoker_thread_calls: 2 }
}

#[derive(Debug)]
struct MixedOutcome {
    ticks: Vec<EcsSnapshot>,
    async_joined: usize,
    outer_completed: bool,
    tick_nodes_visible: usize,
    frame_nodes_visible: usize,
    domain_cancelled: bool,
}

/// Policy C: Riesel's outer graph admits each tick as a separate operation.
/// Each operation invokes the domain's tick WITHOUT scheduling ECS systems.
async fn run_kernel_owned_mixed() -> MixedOutcome {
    let ecs = EcsOwner::start(TickOwner::Kernel);
    let saved = Arc::new(Mutex::new(Vec::<EcsSnapshot>::new()));
    let joined = Arc::new(AtomicUsize::new(0));
    let sender_1 = ecs.sender.clone();
    let sender_2 = ecs.sender.clone();
    let ticks_1 = Arc::clone(&saved);
    let ticks_2 = Arc::clone(&saved);

    let mut nodes = vec![
        Node::new("read_input", &[], vec![], noop()),
        Node::new("tick_1", &["read_input"], vec![Access::Write("ecs-world")],
            operation(move || {
                let sender = sender_1.clone();
                let saved = Arc::clone(&ticks_1);
                async move {
                    let snapshot = domain_tick(sender, INPUT).await?;
                    saved.lock().unwrap().push(snapshot);
                    Ok(())
                }
            })),
        Node::new("tick_2", &["tick_1"], vec![Access::Write("ecs-world")],
            operation(move || {
                let sender = sender_2.clone();
                let saved = Arc::clone(&ticks_2);
                async move {
                    let snapshot = domain_tick(sender, INPUT).await?;
                    saved.lock().unwrap().push(snapshot);
                    Ok(())
                }
            })),
        Node::new("publish", &["tick_2"], vec![Access::Read("ecs-world")], noop()),
    ];
    nodes.extend(workflow_nodes(Arc::clone(&joined)));
    nodes.push(Node::new("final", &["publish", "async_join"], vec![], noop()));
    let plan = Arc::new(Plan::validate(nodes).unwrap());
    let mut run = Run::new(plan, GENERATION, 21, 3, Trace::new(true, 64));
    run.fire("message");

    // A display clock proposes four ticks, but the declared catch-up policy
    // admits only two; each admitted tick is an independently visible node.
    assert_eq!(PROPOSED_TICKS.min(CATCH_UP_CAP), 2);
    run.advance(1).await; // first the input node
    assert_eq!(run.state("tick_1"), Some(Status::Pending));
    assert!(saved.lock().unwrap().is_empty());
    run.advance(1).await; // admits exactly the first ECS tick
    assert_eq!(run.state("tick_1"), Some(Status::Completed));
    assert_eq!(run.state("tick_2"), Some(Status::Pending));
    assert_eq!(*saved.lock().unwrap(), vec![expected_first()]);
    assert_eq!(run.state("final"), Some(Status::Pending));

    tokio::time::timeout(Duration::from_secs(2), run.advance(usize::MAX))
        .await.expect("independent async and ECS operations must complete");
    let tick_nodes_visible = run.trace.retained.iter().filter(|event|
        event.node.starts_with("tick_") && event.status == Status::Completed
    ).count();
    assert_eq!(run.state("final"), Some(Status::Completed));
    assert_eq!(run.peak_in_flight >= 2, true, "the workflow requires overlapping async branches");
    MixedOutcome {
        ticks: saved.lock().unwrap().clone(),
        async_joined: joined.load(Ordering::SeqCst),
        outer_completed: run.state("final") == Some(Status::Completed),
        tick_nodes_visible,
        frame_nodes_visible: 0,
        domain_cancelled: false,
    }
}

/// Policy H: the *domain* owns tick admission inside one outer graph node.
/// Other async branches remain Riesel-owned and can complete while the frame
/// waits for its first or second domain-owned safe-point permit.
///
/// Deliberately expose a known semantic hole: a partially cancelled domain
/// frame returns Ok to the outer graph here, which marks the frame Completed.
/// That is NOT the correct production cancellation mapping.
async fn run_domain_owned_mixed(cancel_after_first: bool) -> MixedOutcome {
    let ecs = EcsOwner::start(TickOwner::Domain);
    let joined = Arc::new(AtomicUsize::new(0));
    let (permits, permit_rx) = mpsc::sync_channel::<DomainPermit>(1);
    let permits_once = Arc::new(Mutex::new(Some(permit_rx)));
    let (observation_tx, mut observations) = tokio::sync::mpsc::channel::<EcsSnapshot>(4);
    let outcome = Arc::new(Mutex::new(None::<SteppedFrameOutcome>));
    let saved = Arc::clone(&outcome);
    let sender = ecs.sender.clone();

    let mut nodes = vec![
        Node::new("read_input", &[], vec![], noop()),
        Node::new("domain_frame", &["read_input"], vec![Access::Write("ecs-world")],
            operation(move || {
                let sender = sender.clone();
                let permits = permits_once.lock().unwrap()
                    .take().expect("one frame operation invocation");
                let observation = observation_tx.clone();
                let saved = Arc::clone(&saved);
                async move {
                    let (completion, reply) = oneshot::channel();
                    sender.try_send(EcsCommand::SteppedFrame {
                        generation: GENERATION,
                        proposed: PROPOSED_TICKS,
                        catch_up_cap: CATCH_UP_CAP,
                        input: INPUT,
                        permits,
                        observation,
                        completion,
                    }).map_err(|_| "domain-owned frame queue full or stopped")?;
                    let frame = reply.await.map_err(|_| "lost domain frame response")??;
                    *saved.lock().unwrap() = Some(frame);
                    Ok(()) // deliberately loses partial cancellation classification
                }
            })),
    ];
    nodes.extend(workflow_nodes(Arc::clone(&joined)));
    nodes.push(Node::new("final", &["domain_frame", "async_join"], vec![], noop()));
    let plan = Arc::new(Plan::validate(nodes).unwrap());

    let running = tokio::spawn(async move {
        let mut run = Run::new(plan, GENERATION, 22, 3, Trace::new(true, 64));
        run.fire("message");
        run.advance(usize::MAX).await;
        (
            run.state("final") == Some(Status::Completed),
            run.trace.retained.iter().filter(|e|
                e.node.starts_with("tick_") && e.status == Status::Completed
            ).count(),
            run.trace.retained.iter().filter(|e|
                e.node == "domain_frame" && e.status == Status::Completed
            ).count(),
        )
    });

    // A stale owner generation cannot admit any tick.
    let (wrong_tx, wrong_rx) = oneshot::channel();
    permits.try_send(DomainPermit::Advance {
        generation: GENERATION - 1, reply: wrong_tx,
    }).unwrap();
    assert_eq!(tokio::time::timeout(Duration::from_secs(2), wrong_rx)
        .await.unwrap().unwrap(), Err("stale session generation"));

    // Distinct async branches join independently while the simulation waits
    // at a domain-owned safe point. This demonstrates policy separation.
    tokio::time::timeout(Duration::from_secs(2), async {
        while joined.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    }).await.expect("async join must not be held by a paused ECS frame");
    assert!(!running.is_finished(), "outer final still depends on the domain frame");
    assert!(observations.try_recv().is_err(), "stale permit must not execute a tick");

    let (first_tx, first_rx) = oneshot::channel();
    permits.try_send(DomainPermit::Advance {
        generation: GENERATION, reply: first_tx,
    }).unwrap();
    assert_eq!(first_rx.await.unwrap(), Ok(()));
    let first = tokio::time::timeout(Duration::from_secs(2), observations.recv())
        .await.unwrap().unwrap();
    assert_eq!(first, expected_first());
    assert!(!running.is_finished(), "first permit cannot complete a two-tick frame");

    let (last_tx, last_rx) = oneshot::channel();
    if cancel_after_first {
        permits.try_send(DomainPermit::Cancel {
            generation: GENERATION, reply: last_tx,
        }).unwrap();
    } else {
        permits.try_send(DomainPermit::Advance {
            generation: GENERATION, reply: last_tx,
        }).unwrap();
    }
    assert_eq!(last_rx.await.unwrap(), Ok(()));
    if !cancel_after_first {
        let second = tokio::time::timeout(Duration::from_secs(2), observations.recv())
            .await.unwrap().unwrap();
        assert_eq!(second, expected_second());
    }
    let (outer_completed, tick_nodes_visible, frame_nodes_visible) =
        tokio::time::timeout(Duration::from_secs(2), running)
            .await.expect("outer graph must join").unwrap();
    let completed = outcome.lock().unwrap().take().expect("domain frame outcome");
    MixedOutcome {
        ticks: completed.ticks,
        async_joined: joined.load(Ordering::SeqCst),
        outer_completed,
        tick_nodes_visible,
        frame_nodes_visible,
        domain_cancelled: completed.cancelled,
    }
}

#[tokio::test(flavor = "current_thread")]
async fn central_and_domain_admission_agree_on_mixed_async_and_tick_outcome() {
    let central = run_kernel_owned_mixed().await;
    let domain = run_domain_owned_mixed(false).await;
    assert_eq!(central.ticks, vec![expected_first(), expected_second()]);
    assert_eq!(central.ticks, domain.ticks,
        "authority partition cannot change committed RunenECS tick semantics");
    assert_eq!(central.async_joined, 1);
    assert_eq!(domain.async_joined, 1);
    assert!(central.outer_completed && domain.outer_completed);
    assert_eq!(central.tick_nodes_visible, 2,
        "C exposes each Riesel-admitted tick as an outer graph node");
    assert_eq!(domain.tick_nodes_visible, 0,
        "H only exposes domain-internal ticks through its own observer");
    assert_eq!(domain.frame_nodes_visible, 1,
        "H's outer graph owns the coarse domain-frame operation");
    assert!(!domain.domain_cancelled);
}

#[tokio::test(flavor = "current_thread")]
async fn partial_domain_cancellation_is_currently_misclassified_as_graph_completion() {
    let domain = run_domain_owned_mixed(true).await;
    assert_eq!(domain.ticks, vec![expected_first()],
        "the already committed tick must not be rolled back");
    assert!(domain.domain_cancelled, "domain returns explicit partial cancellation");
    assert_eq!(domain.async_joined, 1,
        "unrelated async work can still join while the domain is cancelled");
    assert!(domain.outer_completed,
        "NEGATIVE PROOF: this research adapter currently maps partial domain cancellation to Completed");
    assert_eq!(domain.frame_nodes_visible, 1);
    // Production should not adopt this adapter mapping: a domain cancellation
    // has a committed prefix and a distinct terminal reason, not plain success.
}


/// Bias control: a *centralized* graph can also admit unrelated async work
/// while ECS ticks wait for cooperative gate permits. A global one-operation
/// debugger budget from the previous test intentionally postpones unrelated
/// work, but it is NOT an inherent C scheduling limitation.
#[tokio::test(flavor = "current_thread")]
async fn central_graph_can_process_unrelated_async_while_tick_admission_is_paused() {
    let ecs = EcsOwner::start(TickOwner::Kernel);
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let sender_1 = ecs.sender.clone();
    let sender_2 = ecs.sender.clone();
    let permit_1 = Arc::clone(&gate);
    let permit_2 = Arc::clone(&gate);
    let seen = Arc::new(Mutex::new(Vec::<EcsSnapshot>::new()));
    let seen_1 = Arc::clone(&seen);
    let seen_2 = Arc::clone(&seen);
    let joined = Arc::new(AtomicUsize::new(0));

    let mut nodes = vec![
        Node::new("read_input", &[], vec![], noop()),
        Node::new("tick_1", &["read_input"], vec![Access::Write("ecs-world")],
            operation(move || {
                let sender = sender_1.clone();
                let gate = Arc::clone(&permit_1);
                let seen = Arc::clone(&seen_1);
                async move {
                    // Gate is separate from the domain's internal ECS system
                    // admission. Never claim control of RunenECS's scheduler.
                    gate.acquire_owned().await.map_err(|_| "tick admission closed")?
                        .forget();
                    let tick = domain_tick(sender, INPUT).await?;
                    seen.lock().unwrap().push(tick);
                    Ok(())
                }
            })),
        Node::new("tick_2", &["tick_1"], vec![Access::Write("ecs-world")],
            operation(move || {
                let sender = sender_2.clone();
                let gate = Arc::clone(&permit_2);
                let seen = Arc::clone(&seen_2);
                async move {
                    gate.acquire_owned().await.map_err(|_| "tick admission closed")?
                        .forget();
                    let tick = domain_tick(sender, INPUT).await?;
                    seen.lock().unwrap().push(tick);
                    Ok(())
                }
            })),
    ];
    nodes.extend(workflow_nodes(Arc::clone(&joined)));
    nodes.push(Node::new("final", &["tick_2", "async_join"], vec![], noop()));
    let plan = Arc::new(Plan::validate(nodes).unwrap());

    let running = tokio::spawn(async move {
        let mut run = Run::new(plan, GENERATION, 23, 3, Trace::new(true, 64));
        run.fire("message");
        run.advance(usize::MAX).await;
        (
            run.state("final"),
            run.trace.retained.iter().filter(|e|
                e.node.starts_with("tick_") && e.status == Status::Completed
            ).count(),
        )
    });

    // Both IO branches must join before ANY simulation tick is permitted.
    tokio::time::timeout(Duration::from_secs(2), async {
        while joined.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    }).await.expect("centralized admission must not block independent async work");
    assert!(seen.lock().unwrap().is_empty());
    assert!(!running.is_finished());
    gate.add_permits(1); // admit exactly the first tick
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let count = seen.lock().unwrap().len();
            if count == 1 { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("one permitted kernel-owned tick did not complete");
    assert_eq!(*seen.lock().unwrap(), vec![expected_first()]);
    assert!(!running.is_finished(), "second tick has not been admitted");

    gate.add_permits(1); // admit the remaining bounded catch-up tick
    let (status, tick_events) =
        tokio::time::timeout(Duration::from_secs(2), running)
            .await.expect("central graph should finish").unwrap();
    assert_eq!(status, Some(Status::Completed));
    assert_eq!(tick_events, 2);
    assert_eq!(*seen.lock().unwrap(), vec![expected_first(), expected_second()]);
    assert_eq!(joined.load(Ordering::SeqCst), 1);
}
