//! Issue #83 live-control experiment. This is test-only evidence, not a public API.
//! A separately owned coordinator offers snapshots/control while Run::advance(&mut self)
//! is in progress. It does NOT preempt Rust, authorize a remote caller, or own a provider.
use super::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Pending,
    ParkedAtSafePoint,
    Executing,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LiveSnapshot {
    session_generation: u64,
    revision: u64,
    phase: Phase,
    waiting: usize,
    admitted: u64,
}

#[derive(Debug, Clone, Copy)]
enum Control {
    PauseAtNextBoundary,
    AdvanceOneBoundary,
    Resume,
    Cancel,
}

enum Message {
    Boundary {
        generation: u64,
        reply: oneshot::Sender<Result<(), &'static str>>,
    },
    Finished {
        generation: u64,
        reply: oneshot::Sender<Result<(), &'static str>>,
    },
    Command {
        generation: u64,
        action: Control,
        reply: oneshot::Sender<Result<(), &'static str>>,
    },
}

/// A closed, app-session-local channel, not an independently authenticated
/// service. The caller's generation is only a *fencing check*, not a credential.
#[derive(Clone)]
struct Controller {
    messages: mpsc::Sender<Message>,
    snapshots: watch::Receiver<LiveSnapshot>,
}

fn publish(tx: &watch::Sender<LiveSnapshot>, state: &mut LiveSnapshot) {
    state.revision += 1;
    tx.send_replace(state.clone());
}

fn controller(generation: u64) -> Controller {
    let (messages, mut inbound) = mpsc::channel::<Message>(8);
    let initial = LiveSnapshot {
        session_generation: generation,
        revision: 0,
        phase: Phase::Pending,
        waiting: 0,
        admitted: 0,
    };
    let (snap_tx, snapshots) = watch::channel(initial.clone());
    tokio::spawn(async move {
        let mut state = initial;
        let mut paused = true;
        let mut waiting: VecDeque<oneshot::Sender<Result<(), &'static str>>> = VecDeque::new();
        while let Some(message) = inbound.recv().await {
            match message {
                Message::Boundary { generation: provided, reply } => {
                    if provided != generation {
                        let _ = reply.send(Err("stale session generation"));
                    } else if matches!(state.phase, Phase::Cancelled | Phase::Completed) {
                        let _ = reply.send(Err("session is terminal"));
                    } else if paused {
                        waiting.push_back(reply);
                        state.waiting = waiting.len();
                        state.phase = Phase::ParkedAtSafePoint;
                        publish(&snap_tx, &mut state);
                    } else {
                        state.admitted += 1;
                        state.phase = Phase::Executing;
                        publish(&snap_tx, &mut state);
                        let _ = reply.send(Ok(()));
                    }
                }
                Message::Finished { generation: provided, reply } => {
                    if provided != generation {
                        let _ = reply.send(Err("stale session generation"));
                    } else if matches!(state.phase, Phase::Cancelled | Phase::Completed) {
                        let _ = reply.send(Err("session is terminal"));
                    } else {
                        state.phase = Phase::Completed;
                        publish(&snap_tx, &mut state);
                        let _ = reply.send(Ok(()));
                    }
                }
                Message::Command { generation: provided, action, reply } => {
                    if provided != generation {
                        let _ = reply.send(Err("stale session generation"));
                        continue;
                    }
                    if matches!(state.phase, Phase::Completed | Phase::Cancelled) {
                        let _ = reply.send(Err("session is terminal"));
                        continue;
                    }
                    let outcome = match action {
                        Control::PauseAtNextBoundary => {
                            paused = true;
                            // An already executing operation remains executing.
                            Ok(())
                        }
                        Control::AdvanceOneBoundary => {
                            if !paused {
                                Err("step requires paused state")
                            } else if let Some(next) = waiting.pop_front() {
                                state.waiting = waiting.len();
                                state.admitted += 1;
                                state.phase = Phase::Executing;
                                let _ = next.send(Ok(()));
                                Ok(())
                            } else {
                                Err("no parked safe point")
                            }
                        }
                        Control::Resume => {
                            paused = false;
                            let mut released = 0;
                            while let Some(next) = waiting.pop_front() {
                                released += 1;
                                let _ = next.send(Ok(()));
                            }
                            state.waiting = 0;
                            state.admitted += released;
                            if released > 0 {
                                state.phase = Phase::Executing;
                            }
                            Ok(())
                        }
                        Control::Cancel => {
                            state.phase = Phase::Cancelled;
                            while let Some(next) = waiting.pop_front() {
                                let _ = next.send(Err("cancelled at safe point"));
                            }
                            state.waiting = 0;
                            // An in-flight, already-admitted Rust operation is not
                            // preempted; cancellation becomes terminal for future gates.
                            Ok(())
                        }
                    };
                    if outcome.is_ok() {
                        publish(&snap_tx, &mut state);
                    }
                    let _ = reply.send(outcome);
                }
            }
        }
    });
    Controller { messages, snapshots }
}

impl Controller {
    fn snapshot(&self) -> LiveSnapshot {
        self.snapshots.borrow().clone()
    }

    fn observe(&self) -> watch::Receiver<LiveSnapshot> {
        self.snapshots.clone()
    }

    async fn exchange(&self, request: Message, response: oneshot::Receiver<Result<(), &'static str>>)
        -> Result<(), &'static str>
    {
        self.messages.send(request).await.map_err(|_| "control coordinator closed")?;
        response.await.map_err(|_| "control coordinator lost reply")?
    }

    async fn boundary(&self, generation: u64) -> Result<(), &'static str> {
        let (tx, rx) = oneshot::channel();
        self.exchange(Message::Boundary { generation, reply: tx }, rx).await
    }

    async fn finish(&self, generation: u64) -> Result<(), &'static str> {
        let (tx, rx) = oneshot::channel();
        self.exchange(Message::Finished { generation, reply: tx }, rx).await
    }

    async fn control(&self, generation: u64, action: Control) -> Result<(), &'static str> {
        let (tx, rx) = oneshot::channel();
        self.exchange(Message::Command { generation, action, reply: tx }, rx).await
    }
}

async fn until<F>(view: &mut watch::Receiver<LiveSnapshot>, predicate: F) -> LiveSnapshot
where
    F: Fn(&LiveSnapshot) -> bool,
{
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let value = view.borrow_and_update().clone();
            if predicate(&value) {
                return value;
            }
            view.changed().await.expect("coordinator unexpectedly closed");
        }
    }).await.expect("snapshot never reached expected state")
}

#[tokio::test(flavor = "current_thread")]
async fn independent_live_observer_and_operation_stepping_while_runner_is_borrowed() {
    let control = controller(101);
    let mut observer = control.observe();
    let steps = Arc::new(AtomicUsize::new(0));
    let step_count = Arc::clone(&steps);
    let worker_control = control.clone();

    let plan = Arc::new(Plan::validate(vec![
        Node::new("cooperative_operation", &[], vec![], operation(move || {
            let control = worker_control.clone();
            let count = Arc::clone(&step_count);
            async move {
                control.boundary(101).await?;
                count.fetch_add(1, Ordering::SeqCst);
                control.boundary(101).await?;
                count.fetch_add(1, Ordering::SeqCst);
                control.finish(101).await?;
                Ok(())
            }
        })),
    ]).unwrap());
    let running = tokio::spawn(async move {
        let mut run = Run::new(plan, 101, 1, 1, Trace::new(true, 16));
        run.advance(usize::MAX).await;
        run.state("cooperative_operation")
    });
    let first = until(&mut observer, |s| s.phase == Phase::ParkedAtSafePoint).await;
    assert_eq!(first.waiting, 1);
    assert_eq!(first.admitted, 0);
    assert_eq!(steps.load(Ordering::SeqCst), 0);
    assert!(!running.is_finished(), "run must remain in flight");
    assert_eq!(control.snapshot().revision, first.revision);

    assert_eq!(control.control(102, Control::AdvanceOneBoundary).await,
        Err("stale session generation"));
    assert_eq!(control.snapshot().revision, first.revision,
        "stale control must not mutate authoritative state");
    control.control(101, Control::AdvanceOneBoundary).await.unwrap();
    let second = until(&mut observer, |s|
        s.phase == Phase::ParkedAtSafePoint && s.admitted == 1).await;
    assert_eq!(steps.load(Ordering::SeqCst), 1);
    assert!(second.revision > first.revision);
    assert!(!running.is_finished());

    control.control(101, Control::AdvanceOneBoundary).await.unwrap();
    let done = until(&mut observer, |s| s.phase == Phase::Completed).await;
    assert_eq!(done.admitted, 2);
    assert_eq!(steps.load(Ordering::SeqCst), 2);
    assert_eq!(running.await.unwrap(), Some(Status::Completed));
    assert_eq!(control.control(101, Control::Resume).await,
        Err("session is terminal"));
}

#[tokio::test(flavor = "current_thread")]
async fn pause_only_acts_at_cooperative_boundary_not_during_blocked_work() {
    let control = controller(202);
    let mut observer = control.observe();
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let channels = Arc::new(std::sync::Mutex::new(Some((entered_tx, release_rx))));
    let channels_for_worker = Arc::clone(&channels);
    let worker_control = control.clone();
    let plan = Arc::new(Plan::validate(vec![
        Node::new("long_region", &[], vec![], operation(move || {
            let control = worker_control.clone();
            let (entered, release) = channels_for_worker.lock().unwrap()
                .take().expect("single invocation");
            async move {
                control.boundary(202).await?;
                let _ = entered.send(());
                release.await.map_err(|_| "region release lost")?;
                control.boundary(202).await?;
                control.finish(202).await?;
                Ok(())
            }
        })),
    ]).unwrap());
    let running = tokio::spawn(async move {
        let mut run = Run::new(plan, 202, 1, 1, Trace::new(true, 8));
        run.advance(usize::MAX).await;
        run.state("long_region")
    });
    until(&mut observer, |s| s.phase == Phase::ParkedAtSafePoint).await;
    control.control(202, Control::AdvanceOneBoundary).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), entered_rx).await.unwrap().unwrap();
    control.control(202, Control::PauseAtNextBoundary).await.unwrap();
    let running_snapshot = control.snapshot();
    assert_eq!(running_snapshot.phase, Phase::Executing);
    assert!(!running.is_finished(), "noncooperative region cannot be stopped");
    release_tx.send(()).unwrap();
    let parked = until(&mut observer, |s|
        s.phase == Phase::ParkedAtSafePoint && s.admitted == 1).await;
    assert_eq!(parked.waiting, 1);
    assert!(!running.is_finished());
    control.control(202, Control::Resume).await.unwrap();
    until(&mut observer, |s| s.phase == Phase::Completed).await;
    assert_eq!(running.await.unwrap(), Some(Status::Completed));
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_prevents_unstarted_effect_and_exposes_failure_classification_gap() {
    let control = controller(303);
    let mut observer = control.observe();
    let side_effect = Arc::new(AtomicUsize::new(0));
    let effect = Arc::clone(&side_effect);
    let worker_control = control.clone();
    let plan = Arc::new(Plan::validate(vec![
        Node::new("effect", &[], vec![], operation(move || {
            let control = worker_control.clone();
            let effect = Arc::clone(&effect);
            async move {
                control.boundary(303).await?;
                effect.fetch_add(1, Ordering::SeqCst);
                control.finish(303).await?;
                Ok(())
            }
        })),
    ]).unwrap());
    let running = tokio::spawn(async move {
        let mut run = Run::new(plan, 303, 1, 1, Trace::new(true, 16));
        run.advance(usize::MAX).await;
        run.state("effect")
    });
    until(&mut observer, |s| s.phase == Phase::ParkedAtSafePoint).await;
    control.control(303, Control::Cancel).await.unwrap();
    let cancelled = until(&mut observer, |s| s.phase == Phase::Cancelled).await;
    assert_eq!(cancelled.waiting, 0);
    assert_eq!(side_effect.load(Ordering::SeqCst), 0);
    // The current generic runner maps all operation Err outcomes to Failed;
    // a production semantic contract must distinguish controlled cancellation.
    assert_eq!(running.await.unwrap(), Some(Status::Failed));
    assert_eq!(control.control(303, Control::AdvanceOneBoundary).await,
        Err("session is terminal"));
}

#[tokio::test(flavor = "current_thread")]
async fn old_generation_cannot_control_new_generation_or_alias_new_observer_state() {
    let old = controller(401);
    let mut old_observer = old.observe();
    let new = controller(402);
    let first_new = new.snapshot();
    assert_eq!(new.control(401, Control::Resume).await,
        Err("stale session generation"));
    assert_eq!(new.snapshot(), first_new);
    assert_eq!(old.control(402, Control::Resume).await,
        Err("stale session generation"));
    old.control(401, Control::Cancel).await.unwrap();
    until(&mut old_observer, |s| s.phase == Phase::Cancelled).await;
    assert_eq!(new.snapshot().phase, Phase::Pending,
        "a new session must not inherit the old state");
    assert_eq!(old_observer.borrow().session_generation, 401);
    assert_eq!(new.snapshot().session_generation, 402);
}
