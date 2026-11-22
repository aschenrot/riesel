//! Trusted-local, optional read-only projection of delegated domain work.
//!
//! The domain owns the actual execution and the single-writer snapshot. Riesel
//! only binds a published source to a runner-issued *active attempt*. This is
//! deliberately not an external capability exporter, ECS debugger, controller,
//! wire schema or authoritative external-effect reconciliation protocol.

use crate::{GraphRunIdentity, SessionToken};
use std::{
    any::Any,
    collections::VecDeque,
    sync::{
        Arc, Mutex, PoisonError, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use thiserror::Error;
use uuid::Uuid;

/// Per nested attachment; event records are cursor notifications, not effects.
pub const MAX_NESTED_TRANSITIONS: usize = 64;

/// Identity from the real graph recorder, not a UI-supplied operation label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedParent {
    pub run: GraphRunIdentity,
    pub operation: String,
    pub attempt: u64,
}

/// A discoverable, versioned, owner-attributed view; no execution privileges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedDescription {
    pub parent: NestedParent,
    pub domain: String,
    pub owner_generation: Uuid,
    pub view_version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NestedStatus {
    Current,
    OwnerClosed,
    Superseded,
    SessionExpired,
}

#[derive(Debug, Clone)]
pub struct NestedSnapshot<T> {
    pub description: NestedDescription,
    pub sequence: u64,
    pub status: NestedStatus,
    pub payload: Arc<T>,
}

/// The owner generation is part of every cursor; numbers from other owners
/// must never silently point into a new owner's unrelated event history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NestedCursor {
    pub owner_generation: Uuid,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedTransitions {
    pub latest: NestedCursor,
    pub truncated: bool,
    pub status: NestedStatus,
    /// Bounded snapshot-revision notifications; not owner effect attestations.
    pub sequences: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NestedObservationError {
    #[error("nested observation requires a live delegated-frame operation attempt")]
    InactiveAttempt,
    #[error("nested observation is unsupported at this graph boundary")]
    UnsupportedBoundary,
    #[error("nested domain {expected:?} does not match owner {actual:?}")]
    WrongDomain { expected: String, actual: String },
    #[error("the supplied domain owner is no longer active")]
    OwnerClosed,
    #[error("the nested observer has been superseded by a different owner generation")]
    Superseded,
    #[error("a nested view is already attached to this operation attempt")]
    AlreadyAttached,
    #[error("nested view version must be nonzero")]
    InvalidVersion,
    #[error("nested view version {actual} is unsupported (expected {expected})")]
    UnsupportedVersion { expected: u32, actual: u32 },
    #[error("nested view payload type is incompatible")]
    WrongPayloadType,
    #[error("cursor belongs to a different domain owner generation")]
    StaleCursor,
    #[error("cursor {requested} is ahead of owner sequence {latest}")]
    FutureCursor { requested: u64, latest: u64 },
    #[error("nested observation sequence has exhausted its range")]
    SequenceExhausted,
    #[error("the owning graph application session expired")]
    SessionExpired,
}

pub(crate) struct DomainLifetime {
    domain: String,
    generation: Uuid,
    live: AtomicBool,
}

/// The *domain coordinator* holds this non-cloneable owner for its own lifetime.
/// It may stay next to a non-Send ECS World on a dedicated native thread.
pub struct NestedDomainOwner {
    lifetime: Arc<DomainLifetime>,
}

impl NestedDomainOwner {
    pub fn new(domain: impl Into<String>) -> Self {
        Self {
            lifetime: Arc::new(DomainLifetime {
                domain: domain.into(),
                generation: Uuid::new_v4(),
                live: AtomicBool::new(true),
            }),
        }
    }

    /// A weak local registration ticket. Holding this never keeps the domain
    /// owner alive and does not authenticate or authorize an external caller.
    pub fn registration(&self) -> NestedDomainRegistration {
        NestedDomainRegistration {
            lifetime: Arc::downgrade(&self.lifetime),
        }
    }

    pub fn generation(&self) -> Uuid {
        self.lifetime.generation
    }
}

impl Drop for NestedDomainOwner {
    fn drop(&mut self) {
        self.lifetime.live.store(false, Ordering::Release);
    }
}

#[derive(Clone)]
pub struct NestedDomainRegistration {
    lifetime: Weak<DomainLifetime>,
}

impl NestedDomainRegistration {
    pub(crate) fn live_owner(
        &self,
    ) -> Result<(String, Uuid, Weak<DomainLifetime>), NestedObservationError> {
        let owner = self
            .lifetime
            .upgrade()
            .ok_or(NestedObservationError::OwnerClosed)?;
        if !owner.live.load(Ordering::Acquire) {
            return Err(NestedObservationError::OwnerClosed);
        }
        Ok((
            owner.domain.clone(),
            owner.generation,
            Arc::downgrade(&owner),
        ))
    }
}

pub(crate) struct NestedAttachment {
    pub(crate) description: NestedDescription,
    erased: Arc<dyn Any + Send + Sync>,
    retired: Arc<AtomicBool>,
}

impl NestedAttachment {
    pub(crate) fn retire(&self) {
        self.retired.store(true, Ordering::Release);
    }

    pub(crate) fn observer<T: Send + Sync + 'static>(
        &self,
        session: SessionToken,
    ) -> Result<NestedObserver<T>, NestedObservationError> {
        let source = Arc::clone(&self.erased)
            .downcast::<Mutex<NestedStore<T>>>()
            .map_err(|_| NestedObservationError::WrongPayloadType)?;
        Ok(NestedObserver { source, session })
    }
}

struct NestedStore<T> {
    description: NestedDescription,
    owner: Weak<DomainLifetime>,
    retired: Arc<AtomicBool>,
    sequence: u64,
    payload: Arc<T>,
    transitions: VecDeque<u64>,
}

impl<T> NestedStore<T> {
    fn status(&self, session: &SessionToken) -> NestedStatus {
        if !session.is_active() {
            NestedStatus::SessionExpired
        } else if self.retired.load(Ordering::Acquire) {
            NestedStatus::Superseded
        } else if !self
            .owner
            .upgrade()
            .is_some_and(|owner| owner.live.load(Ordering::Acquire))
        {
            NestedStatus::OwnerClosed
        } else {
            NestedStatus::Current
        }
    }
}

/// Owner-only publication, scoped to exactly one graph operation attempt. It
/// contains no outer graph effect recorder or controller; publication never
/// changes the graph's own terminal classification or confirmed effects.
pub struct NestedPublisher<T> {
    source: Arc<Mutex<NestedStore<T>>>,
}

impl<T: Send + Sync + 'static> NestedPublisher<T> {
    pub(crate) fn attach(
        description: NestedDescription,
        owner: Weak<DomainLifetime>,
        initial: T,
    ) -> (NestedAttachment, Self) {
        let retired = Arc::new(AtomicBool::new(false));
        let source = Arc::new(Mutex::new(NestedStore {
            description: description.clone(),
            owner,
            retired: Arc::clone(&retired),
            sequence: 0,
            payload: Arc::new(initial),
            transitions: VecDeque::new(),
        }));
        let erased: Arc<dyn Any + Send + Sync> = source.clone();
        (
            NestedAttachment {
                description,
                erased,
                retired,
            },
            Self { source },
        )
    }

    /// Publish a domain-owned immutable copy. No queue sends or awaits occur
    /// while the native ECS world is borrowed. Old events are discarded.
    pub fn publish(&self, snapshot: T) -> Result<NestedCursor, NestedObservationError> {
        let mut state = self.source.lock().unwrap_or_else(PoisonError::into_inner);
        if state.retired.load(Ordering::Acquire) {
            return Err(NestedObservationError::Superseded);
        }
        if !state
            .owner
            .upgrade()
            .is_some_and(|owner| owner.live.load(Ordering::Acquire))
        {
            return Err(NestedObservationError::OwnerClosed);
        }
        state.sequence = state
            .sequence
            .checked_add(1)
            .ok_or(NestedObservationError::SequenceExhausted)?;
        state.payload = Arc::new(snapshot);
        let sequence = state.sequence;
        state.transitions.push_back(sequence);
        if state.transitions.len() > MAX_NESTED_TRANSITIONS {
            state.transitions.pop_front();
        }
        Ok(NestedCursor {
            owner_generation: state.description.owner_generation,
            sequence,
        })
    }
}

/// Clonable read-only local view. It holds no domain owner, publisher, world,
/// control method or external credential. It may outlive the outer graph run.
pub struct NestedObserver<T> {
    source: Arc<Mutex<NestedStore<T>>>,
    session: SessionToken,
}

impl<T> Clone for NestedObserver<T> {
    fn clone(&self) -> Self {
        Self {
            source: Arc::clone(&self.source),
            session: self.session.clone(),
        }
    }
}

impl<T> NestedObserver<T> {
    pub fn snapshot(&self) -> NestedSnapshot<T> {
        let state = self.source.lock().unwrap_or_else(PoisonError::into_inner);
        NestedSnapshot {
            description: state.description.clone(),
            sequence: state.sequence,
            status: state.status(&self.session),
            payload: Arc::clone(&state.payload),
        }
    }

    pub fn changes_since(
        &self,
        cursor: NestedCursor,
    ) -> Result<NestedTransitions, NestedObservationError> {
        let state = self.source.lock().unwrap_or_else(PoisonError::into_inner);
        if cursor.owner_generation != state.description.owner_generation {
            return Err(NestedObservationError::StaleCursor);
        }
        if cursor.sequence > state.sequence {
            return Err(NestedObservationError::FutureCursor {
                requested: cursor.sequence,
                latest: state.sequence,
            });
        }
        let status = state.status(&self.session);
        if status == NestedStatus::SessionExpired {
            return Err(NestedObservationError::SessionExpired);
        }
        if status == NestedStatus::Superseded {
            return Err(NestedObservationError::Superseded);
        }
        let first = state
            .transitions
            .front()
            .copied()
            .unwrap_or(state.sequence.saturating_add(1));
        Ok(NestedTransitions {
            latest: NestedCursor {
                owner_generation: state.description.owner_generation,
                sequence: state.sequence,
            },
            truncated: cursor.sequence < first.saturating_sub(1),
            status,
            sequences: state
                .transitions
                .iter()
                .copied()
                .filter(|i| *i > cursor.sequence)
                .collect(),
        })
    }
}
