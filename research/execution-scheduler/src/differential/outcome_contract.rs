//! Issue #83 Gate I: bounded outcome/identity/authority contract research.
//!
//! Independent from production API: names and variants are NOT accepted.
//! Deliberately uses the existing real RunenECS owner and a small typed
//! envelope; it does not patch the research runner's unit/Err interface.
//! A complete product must not infer effects from a lost response or trace.

use super::*;
use std::collections::BTreeMap;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanStamp {
    logical_id: String,
    revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunAddress {
    session_generation: u64,
    run_id: u64,
    plan: PlanStamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CorrelatedEvent {
    owner: RunAddress,
    node: String,
    attempt: u64,
    sequence: u64,
    status: Status,
}

#[derive(Debug, PartialEq, Eq)]
enum AddressError {
    DuplicateRun {
        session_generation: u64,
        run_id: u64,
    },
    StaleGeneration,
    MissingRun,
}

/// Research-only session-owned map, NOT a plan registry, credential or
/// durable trace index. Nothing here makes an external caller trustworthy.
struct RunLedger {
    generation: u64,
    registered: BTreeMap<u64, PlanStamp>,
}

impl RunLedger {
    fn new(generation: u64) -> Self {
        Self { generation, registered: BTreeMap::new() }
    }

    fn register(&mut self, run_id: u64, plan: PlanStamp) -> Result<RunAddress, AddressError> {
        if self.registered.contains_key(&run_id) {
            return Err(AddressError::DuplicateRun {
                session_generation: self.generation, run_id,
            });
        }
        self.registered.insert(run_id, plan.clone());
        Ok(RunAddress { session_generation: self.generation, run_id, plan })
    }

    fn correlate(&self, event: &Event) -> Result<CorrelatedEvent, AddressError> {
        if event.session != self.generation {
            return Err(AddressError::StaleGeneration);
        }
        let plan = self.registered.get(&event.run).ok_or(AddressError::MissingRun)?;
        Ok(CorrelatedEvent {
            owner: RunAddress {
                session_generation: self.generation,
                run_id: event.run,
                plan: plan.clone(),
            },
            node: event.node.clone(),
            attempt: event.attempt,
            sequence: event.sequence,
            status: event.status,
        })
    }
}

/// Observable already-committed effects and the final result are independent.
/// In an actual API, this would require a domain-authoritative state/version
/// handle and a defined failure taxonomy, not a tuple of integers alone.
#[derive(Debug, Clone, PartialEq, Eq)]
enum BoundaryTerminal {
    Completed,
    Cancelled,
    Failed { reason: &'static str },
    OutcomeUnknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BoundaryOutcome {
    identity: RunAddress,
    operation: String,
    /// Confirmed *by the owner* from the completed frame result, not a lossy
    /// diagnostics stream, and never assumed to be rollbackable.
    committed_ticks: Vec<EcsSnapshot>,
    allowed_ticks: usize,
    terminal: BoundaryTerminal,
}

#[derive(Debug, PartialEq, Eq)]
enum OutcomeError {
    InvalidCommittedPrefix,
    IncompleteSuccessfulFrame,
}

impl BoundaryOutcome {
    fn from_domain(
        identity: RunAddress,
        operation: &str,
        frame: SteppedFrameOutcome,
        allowed_ticks: usize,
    ) -> Result<Self, OutcomeError> {
        if frame.ticks.len() > allowed_ticks {
            return Err(OutcomeError::InvalidCommittedPrefix);
        }
        if !frame.cancelled && frame.ticks.len() != allowed_ticks {
            return Err(OutcomeError::IncompleteSuccessfulFrame);
        }
        Ok(Self {
            identity,
            operation: operation.to_owned(),
            committed_ticks: frame.ticks,
            allowed_ticks,
            terminal: if frame.cancelled {
                BoundaryTerminal::Cancelled
            } else {
                BoundaryTerminal::Completed
            },
        })
    }

    /// A response lost after dispatch cannot be classified as Failed or
    /// Cancelled merely because its reply was not observed. Any confirmed
    /// prefix here would need an independent authoritative resnapshot.
    fn lost_reply(identity: RunAddress, operation: &str, allowed_ticks: usize) -> Self {
        Self {
            identity, operation: operation.to_owned(), committed_ticks: Vec::new(),
            allowed_ticks, terminal: BoundaryTerminal::OutcomeUnknown,
        }
    }

    fn remaining_unconfirmed(&self) -> usize {
        self.allowed_ticks - self.committed_ticks.len()
    }

    /// Current Status has NO honest representation for OutcomeUnknown.
    /// This projection is intentionally partial, not a proposed production API.
    fn status_projection(&self) -> Option<Status> {
        match self.terminal {
            BoundaryTerminal::Completed => Some(Status::Completed),
            BoundaryTerminal::Cancelled => Some(Status::Cancelled),
            BoundaryTerminal::Failed { .. } => Some(Status::Failed),
            BoundaryTerminal::OutcomeUnknown => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdmissionOwner { Riesel, Domain }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Boundary { OuterOperation, DomainTick, EcsSystem }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlError { StaleSession, WrongOwner, UnsupportedBoundary }

struct Authority {
    generation: u64,
    outer: AdmissionOwner,
    tick: AdmissionOwner,
    system: Option<AdmissionOwner>,
}

impl Authority {
    /// Fencing and boundary routing, not authentication or OS scheduling.
    fn can_step(&self, generation: u64, who: AdmissionOwner, target: Boundary)
        -> Result<(), ControlError>
    {
        if generation != self.generation {
            return Err(ControlError::StaleSession);
        }
        let owner = match target {
            Boundary::OuterOperation => Some(self.outer),
            Boundary::DomainTick => Some(self.tick),
            Boundary::EcsSystem => self.system,
        }.ok_or(ControlError::UnsupportedBoundary)?;
        if who != owner {
            return Err(ControlError::WrongOwner);
        }
        Ok(())
    }
}

/// Actual pinned RunenECS, with !Send state owned by its own thread.
/// The resulting owner completion is authoritative even if a separate
/// observation channel drops messages.
async fn real_frame(cancel_after_first: bool) -> SteppedFrameOutcome {
    let ecs = EcsOwner::start(TickOwner::Domain);
    let (permits, permit_rx) = mpsc::sync_channel::<DomainPermit>(1);
    let (observation, _unread) = tokio::sync::mpsc::channel::<EcsSnapshot>(1);
    let (tx, result_rx) = oneshot::channel();
    ecs.sender.try_send(EcsCommand::SteppedFrame {
        generation: 940,
        proposed: 3,
        catch_up_cap: if cancel_after_first { 3 } else { 2 },
        input: 2,
        permits: permit_rx,
        observation,
        completion: tx,
    }).expect("single owner frame command");
    let send = |cancel: bool| -> (oneshot::Receiver<Result<(), &'static str>>, DomainPermit) {
        let (tx, rx) = oneshot::channel();
        let cmd = if cancel {
            DomainPermit::Cancel { generation: 940, reply: tx }
        } else {
            DomainPermit::Advance { generation: 940, reply: tx }
        };
        (rx, cmd)
    };

    let (ack, cmd) = send(false);
    permits.send(cmd).expect("owner still alive");
    assert_eq!(ack.await.unwrap(), Ok(()));
    let (ack, cmd) = send(cancel_after_first);
    permits.send(cmd).expect("owner still alive");
    assert_eq!(ack.await.unwrap(), Ok(()));
    if !cancel_after_first {
        // Two allowed ticks complete this frame (proposal 3, cap 2).
    }
    tokio::time::timeout(Duration::from_secs(2), result_rx)
        .await.expect("domain completion deadline")
        .expect("domain owner did not lose the response")
        .expect("domain scheduler result")
}

#[tokio::test(flavor = "current_thread")]
async fn committed_prefix_and_partial_cancellation_stay_distinct_from_success() {
    let id = RunAddress {
        session_generation: 940,
        run_id: 31,
        plan: PlanStamp { logical_id: "simulation".into(), revision: 4 },
    };

    let complete = BoundaryOutcome::from_domain(
        id.clone(), "frame", real_frame(false).await, 2,
    ).unwrap();
    assert_eq!(complete.terminal, BoundaryTerminal::Completed);
    assert_eq!(complete.status_projection(), Some(Status::Completed));
    assert_eq!(complete.committed_ticks.len(), 2);
    assert_eq!(complete.remaining_unconfirmed(), 0);
    assert_eq!(complete.committed_ticks[0].tick, 1);
    assert_eq!(complete.committed_ticks[1].tick, 2);

    let partial = BoundaryOutcome::from_domain(
        id, "frame", real_frame(true).await, 3,
    ).unwrap();
    assert_eq!(partial.terminal, BoundaryTerminal::Cancelled);
    assert_eq!(partial.status_projection(), Some(Status::Cancelled));
    assert_eq!(partial.committed_ticks.len(), 1);
    assert_eq!(partial.committed_ticks[0].tick, 1);
    assert_eq!(partial.remaining_unconfirmed(), 2);
    assert_ne!(partial.terminal, complete.terminal);
}

#[test]
fn uncertain_reply_and_invalid_success_do_not_masquerade_as_completed() {
    let id = RunAddress {
        session_generation: 940,
        run_id: 32,
        plan: PlanStamp { logical_id: "simulation".into(), revision: 4 },
    };
    let unknown = BoundaryOutcome::lost_reply(id.clone(), "frame", 2);
    assert_eq!(unknown.terminal, BoundaryTerminal::OutcomeUnknown);
    assert_eq!(unknown.status_projection(), None);
    assert_eq!(unknown.remaining_unconfirmed(), 2);
    let incomplete = SteppedFrameOutcome {
        ticks: vec![EcsSnapshot {
            tick: 1, positions: vec![2, 12], invoker_thread_calls: 1,
        }],
        cancelled: false,
    };
    assert_eq!(
        BoundaryOutcome::from_domain(id, "frame", incomplete, 2),
        Err(OutcomeError::IncompleteSuccessfulFrame)
    );
}

#[test]
fn plan_revision_and_session_generation_are_not_inferred_from_old_trace_fields() {
    // Gate G demonstrated this ambiguity: the current Event has no plan ID/
    // revision. This locally owned ledger rejects run reuse and associates
    // events with the exact document known before execution.
    let mut ledger = RunLedger::new(940);
    let rev4 = PlanStamp { logical_id: "simulation".into(), revision: 4 };
    let rev5 = PlanStamp { logical_id: "simulation".into(), revision: 5 };
    let first = ledger.register(3, rev4.clone()).unwrap();
    assert_eq!(ledger.register(3, rev5),
        Err(AddressError::DuplicateRun { session_generation: 940, run_id: 3 }));
    let event = Event {
        sequence: 1, session: 940, run: 3,
        node: "frame".into(), attempt: 1, status: Status::Running,
    };
    let combined = ledger.correlate(&event).unwrap();
    assert_eq!(combined.owner, first);
    assert_eq!(combined.owner.plan, rev4);
    assert_eq!(combined.node, "frame");
    assert_eq!(combined.attempt, 1);
    assert_eq!(combined.sequence, 1);
    let mut stale = event.clone();
    stale.session = 939;
    assert_eq!(ledger.correlate(&stale), Err(AddressError::StaleGeneration));
    let mut absent = event.clone();
    absent.run = 111;
    assert_eq!(ledger.correlate(&absent), Err(AddressError::MissingRun));

    // A new application generation legitimately has its own distinct run 3.
    let mut restarted = RunLedger::new(941);
    let after_restart = restarted.register(3, PlanStamp {
        logical_id: "simulation".into(), revision: 6,
    }).unwrap();
    assert_ne!(first, after_restart);
    assert_eq!(restarted.correlate(&event), Err(AddressError::StaleGeneration));
}

#[test]
fn nested_control_has_one_owner_per_boundary_and_unavailable_system_stepping() {
    let central = Authority {
        generation: 940, outer: AdmissionOwner::Riesel,
        tick: AdmissionOwner::Riesel, system: None,
    };
    let domain = Authority {
        generation: 940, outer: AdmissionOwner::Riesel,
        tick: AdmissionOwner::Domain, system: None,
    };
    assert_eq!(central.can_step(940, AdmissionOwner::Riesel, Boundary::DomainTick), Ok(()));
    assert_eq!(central.can_step(940, AdmissionOwner::Domain, Boundary::DomainTick),
        Err(ControlError::WrongOwner));
    assert_eq!(domain.can_step(940, AdmissionOwner::Domain, Boundary::DomainTick), Ok(()));
    assert_eq!(domain.can_step(940, AdmissionOwner::Riesel, Boundary::DomainTick),
        Err(ControlError::WrongOwner));
    assert_eq!(domain.can_step(940, AdmissionOwner::Riesel, Boundary::OuterOperation), Ok(()));
    assert_eq!(domain.can_step(940, AdmissionOwner::Domain, Boundary::OuterOperation),
        Err(ControlError::WrongOwner));
    assert_eq!(domain.can_step(940, AdmissionOwner::Domain, Boundary::EcsSystem),
        Err(ControlError::UnsupportedBoundary));
    assert_eq!(domain.can_step(939, AdmissionOwner::Domain, Boundary::DomainTick),
        Err(ControlError::StaleSession));
}
