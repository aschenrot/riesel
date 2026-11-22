//! Session validity shared by generated local and remote capability handles.
//! A recovered application always begins with a new session token.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(feature = "internal-publication")]
use std::sync::{Mutex, PoisonError};
#[cfg(feature = "internal-publication")]
use std::time::Duration;
#[cfg(feature = "internal-publication")]
use uuid::Uuid;

#[cfg(feature = "async-tcp")]
use tokio::sync::Notify;

// The opt-in, crate-internal adapter contract is NOT an application exporter,
// peer credential, authorization implementation, or supported stable SDK API.
// It is isolated behind an explicit feature until #60 accepts publication.
#[cfg(feature = "internal-publication")]
const INTERNAL_MAX_IN_FLIGHT: usize = 32;

#[cfg(feature = "internal-publication")]
#[derive(Debug)]
struct InternalPublicationState {
    // Minted at actual SessionToken construction by Riesel startup, not by
    // an adapter, GUI, provider or OS-process owner.
    generation: Uuid,
    gate: Mutex<InternalIngressState>,
    drained: Notify,
}

#[cfg(feature = "internal-publication")]
#[derive(Debug)]
struct InternalIngressState {
    closed: bool,
    in_flight: usize,
}

/// Opt-in internal publication ingress for one *actual* Riesel session.
/// A generation is a nonsecret lifetime fence, NOT caller authorization.
/// No network listener, client principal or public application interface exists.
#[cfg(feature = "internal-publication")]
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct InternalPublicationSession {
    state: Arc<InternalPublicationState>,
}

/// A request successfully admitted before session revocation. This permit
/// holds an in-flight count, never a blocking lock across Rust/async effects.
/// Its drop says the adapter finished *tracking* the request, not that an
/// external mutation rolled back or that its result reached the caller.
#[cfg(feature = "internal-publication")]
#[doc(hidden)]
#[derive(Debug)]
pub struct InternalPublicationPermit {
    state: Arc<InternalPublicationState>,
}

#[cfg(feature = "internal-publication")]
impl Drop for InternalPublicationPermit {
    fn drop(&mut self) {
        let mut gate = self
            .state
            .gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        debug_assert!(gate.in_flight > 0);
        gate.in_flight -= 1;
        if gate.in_flight == 0 {
            // A notifier has no authority over a callback still executing.
            self.state.drained.notify_waiters();
        }
    }
}

/// Admission errors only. Once admitted, an operation may have effects even
/// after timeout, caller disconnect, cancellation or session invalidation.
#[cfg(feature = "internal-publication")]
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalAdmissionError {
    WrongGeneration,
    Expired,
    AtCapacity,
}

/// A bounded *observation* of currently tracked admitted work. A timeout
/// never cancels the work or proves the absence of external side effects.
#[cfg(feature = "internal-publication")]
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalDrain {
    Drained,
    TimedOut { in_flight: usize },
}

#[cfg(feature = "internal-publication")]
impl InternalPublicationState {
    fn revoke(&self, active: &AtomicBool) {
        // Linearization with admit(): revoke permission and invalidate the
        // existing runtime's typed handles while holding the small gate.
        // Never wait for user callbacks here.
        let mut gate = self.gate.lock().unwrap_or_else(PoisonError::into_inner);
        gate.closed = true;
        active.store(false, Ordering::Release);
        drop(gate);
        self.drained.notify_waiters();
    }

    fn in_flight(&self) -> usize {
        self.gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .in_flight
    }
}

#[cfg(feature = "internal-publication")]
impl InternalPublicationSession {
    /// For correlation only: this is not a credential or verified server ID.
    pub fn generation(&self) -> String {
        self.state.generation.to_string()
    }

    /// The caller MUST separately check its chosen application's published
    /// contract, verified principal, operation and grant. This checks ONLY
    /// runtime-session freshness and the finite ingress budget.
    pub fn try_admit(
        &self,
        expected_generation: &str,
    ) -> Result<InternalPublicationPermit, InternalAdmissionError> {
        let mut gate = self
            .state
            .gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if gate.closed {
            return Err(InternalAdmissionError::Expired);
        }
        if expected_generation != self.state.generation.to_string() {
            return Err(InternalAdmissionError::WrongGeneration);
        }
        if gate.in_flight >= INTERNAL_MAX_IN_FLIGHT {
            return Err(InternalAdmissionError::AtCapacity);
        }
        gate.in_flight += 1;
        Ok(InternalPublicationPermit {
            state: Arc::clone(&self.state),
        })
    }

    pub fn in_flight(&self) -> usize {
        self.state.in_flight()
    }

    /// Bounded, cancellation-safe async wait, never a lock held across user
    /// callbacks. Only call after external admission has been withdrawn or
    /// the whole runtime session has been revoked: a zero count by itself
    /// does not prevent a new request from arriving later.
    pub async fn drain_for(&self, timeout: Duration) -> InternalDrain {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.state.drained.notified();
            let mut notified = std::pin::pin!(notified);
            notified.as_mut().enable();
            if self.in_flight() == 0 {
                return InternalDrain::Drained;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                // A permit may be released concurrently with the deadline.
                // Do not report unresolved work when the final observation
                // already proves the tracked ingress count is zero.
                let remaining = self.in_flight();
                return if remaining == 0 {
                    InternalDrain::Drained
                } else {
                    InternalDrain::TimedOut {
                        in_flight: remaining,
                    }
                };
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionToken {
    active: Arc<AtomicBool>,
    #[cfg(feature = "internal-publication")]
    internal_publication: Arc<InternalPublicationState>,
    #[cfg(feature = "async-tcp")]
    changed: Arc<Notify>,
}

impl Default for SessionToken {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionToken {
    pub fn new() -> Self {
        Self {
            active: Arc::new(AtomicBool::new(true)),
            #[cfg(feature = "internal-publication")]
            internal_publication: Arc::new(InternalPublicationState {
                generation: Uuid::new_v4(),
                gate: Mutex::new(InternalIngressState {
                    closed: false,
                    in_flight: 0,
                }),
                drained: Notify::new(),
            }),
            #[cfg(feature = "async-tcp")]
            changed: Arc::new(Notify::new()),
        }
    }

    /// Called before an application session begins provider teardown.
    pub fn invalidate(&self) {
        #[cfg(feature = "internal-publication")]
        self.internal_publication.revoke(&self.active);
        #[cfg(not(feature = "internal-publication"))]
        self.active.store(false, Ordering::Release);
        #[cfg(feature = "async-tcp")]
        self.changed.notify_waiters();
    }

    /// Internal opt-in application-publication bridge. For one actual runtime
    /// session, not an approved SDK exporter, network listener or credential.
    #[cfg(feature = "internal-publication")]
    #[doc(hidden)]
    pub fn internal_publication_session(&self) -> InternalPublicationSession {
        InternalPublicationSession {
            state: Arc::clone(&self.internal_publication),
        }
    }

    /// Wait without busy-polling until this application session expires.
    /// Register before checking the state to avoid losing concurrent shutdown.
    #[cfg(feature = "async-tcp")]
    pub async fn cancelled(&self) {
        loop {
            let notified = self.changed.notified();
            let mut notified = std::pin::pin!(notified);
            notified.as_mut().enable();
            if !self.is_active() {
                return;
            }
            notified.await;
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// True only for clones of this exact in-process runtime session.
    /// This compares shared revocation ownership, not a process-global ID
    /// or proof of external caller identity. Never use it as authentication
    /// for remote management or privileged commands.
    pub fn shares_session_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.active, &other.active)
    }

    pub fn check(&self) -> Result<(), crate::RpcError> {
        if self.is_active() {
            Ok(())
        } else {
            Err(crate::RpcError::SessionExpired)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloned_sessions_share_identity_but_restarted_session_does_not() {
        let session = SessionToken::new();
        let clone = session.clone();
        let next = SessionToken::new();
        assert!(session.shares_session_with(&clone));
        assert!(clone.shares_session_with(&session));
        assert!(!session.shares_session_with(&next));
        session.invalidate();
        assert!(session.shares_session_with(&clone));
        assert!(!clone.shares_session_with(&next));
    }

    #[test]
    fn cloned_tokens_share_revocation() {
        let old = SessionToken::new();
        let clone = old.clone();
        assert!(clone.check().is_ok());
        old.invalidate();
        assert!(matches!(
            clone.check(),
            Err(crate::RpcError::SessionExpired)
        ));
        assert!(SessionToken::new().is_active());
    }
}

#[cfg(all(test, feature = "async-tcp"))]
mod async_tests {
    use super::*;
    use std::time::Duration;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn waiters_wake_and_late_subscribers_see_cancellation() {
        runtime().block_on(async {
            let session = SessionToken::new();
            let pending = session.clone();
            let waiting = tokio::spawn(async move {
                pending.cancelled().await;
            });
            tokio::task::yield_now().await;
            session.invalidate();
            tokio::time::timeout(Duration::from_millis(200), waiting)
                .await
                .unwrap()
                .unwrap();
            tokio::time::timeout(Duration::from_millis(200), session.cancelled())
                .await
                .unwrap();
        });
    }

    #[test]
    fn invalidation_racing_with_registration_is_not_lost() {
        runtime().block_on(async {
            for _ in 0..256 {
                let token = SessionToken::new();
                let clone = token.clone();
                let waiter = tokio::spawn(async move {
                    clone.cancelled().await;
                });
                token.invalidate();
                tokio::time::timeout(Duration::from_millis(200), waiter)
                    .await
                    .unwrap()
                    .unwrap();
            }
        });
    }
}

#[cfg(all(test, feature = "internal-publication"))]
mod internal_publication_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    #[test]
    fn runtime_issued_generation_changes_and_never_grants_another_session() {
        let first = SessionToken::new();
        let cloned = first.clone();
        let second = SessionToken::new();
        let one = first.internal_publication_session();
        let same = cloned.internal_publication_session();
        let two = second.internal_publication_session();
        let generation = one.generation();
        assert_eq!(same.generation(), generation);
        assert_ne!(generation, two.generation());
        assert!(one.try_admit(&generation).is_ok());
        assert_eq!(
            two.try_admit(&generation).unwrap_err(),
            InternalAdmissionError::WrongGeneration
        );
        first.invalidate();
        assert_eq!(
            same.try_admit(&generation).unwrap_err(),
            InternalAdmissionError::Expired
        );
        assert!(two.try_admit(&two.generation()).is_ok());
    }

    #[test]
    fn invalidation_closes_admission_immediately_without_waiting_for_effect() {
        let runtime = SessionToken::new();
        let publication = runtime.internal_publication_session();
        let generation = publication.generation();
        let permit = publication.try_admit(&generation).unwrap();
        let observed = Arc::new(AtomicUsize::new(0));
        let effects = Arc::clone(&observed);
        let (done_tx, done_rx) = mpsc::channel();
        let invalidating = runtime.clone();
        let thread = std::thread::spawn(move || {
            invalidating.invalidate();
            done_tx.send(()).unwrap();
        });
        done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!runtime.is_active());
        assert_eq!(
            publication.try_admit(&generation).unwrap_err(),
            InternalAdmissionError::Expired
        );
        // An operation admitted *before* revocation remains capable of
        // causing an effect: it cannot be described as rolled back.
        effects.fetch_add(1, AtomicOrdering::SeqCst);
        assert_eq!(observed.load(AtomicOrdering::SeqCst), 1);
        assert_eq!(publication.in_flight(), 1);
        drop(permit);
        thread.join().unwrap();
        assert_eq!(publication.in_flight(), 0);
    }

    #[test]
    fn finite_budget_refuses_overload_before_calling_application() {
        let runtime = SessionToken::new();
        let publication = runtime.internal_publication_session();
        let generation = publication.generation();
        let mut permits: Vec<_> = (0..INTERNAL_MAX_IN_FLIGHT)
            .map(|_| publication.try_admit(&generation).unwrap())
            .collect();
        assert_eq!(
            publication.try_admit(&generation).unwrap_err(),
            InternalAdmissionError::AtCapacity
        );
        permits.pop();
        assert!(publication.try_admit(&generation).is_ok());
        drop(permits);
    }

    #[test]
    fn drain_is_finite_and_reports_unresolved_work_without_cancelling_it() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let runtime = SessionToken::new();
            let publication = runtime.internal_publication_session();
            let permit = publication.try_admit(&publication.generation()).unwrap();
            runtime.invalidate();
            assert_eq!(
                publication.drain_for(Duration::from_millis(10)).await,
                InternalDrain::TimedOut { in_flight: 1 }
            );
            // Permit may still represent a dispatched unknown-effect call.
            drop(permit);
            assert_eq!(
                publication.drain_for(Duration::from_secs(1)).await,
                InternalDrain::Drained
            );
        });
    }

    #[test]
    fn async_admission_held_across_await_does_not_block_revocation() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let runtime = SessionToken::new();
            let publication = runtime.internal_publication_session();
            let expected = publication.generation();
            let effect = Arc::new(AtomicUsize::new(0));
            let worker_effect = Arc::clone(&effect);
            let worker_publication = publication.clone();
            let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let worker = tokio::spawn(async move {
                let _permit = worker_publication.try_admit(&expected).unwrap();
                entered_tx.send(()).unwrap();
                release_rx.await.unwrap();
                // Already dispatched, so this effect is not rolled back by
                // runtime invalidation. The tracked permit spans the await.
                worker_effect.fetch_add(1, AtomicOrdering::SeqCst);
            });

            entered_rx.await.unwrap();
            runtime.invalidate();
            assert_eq!(
                publication
                    .try_admit(&publication.generation())
                    .unwrap_err(),
                InternalAdmissionError::Expired
            );
            assert_eq!(
                publication.drain_for(Duration::from_millis(20)).await,
                InternalDrain::TimedOut { in_flight: 1 }
            );
            release_tx.send(()).unwrap();
            worker.await.unwrap();
            assert_eq!(effect.load(AtomicOrdering::SeqCst), 1);
            assert_eq!(
                publication.drain_for(Duration::from_secs(1)).await,
                InternalDrain::Drained
            );
        });
    }

    #[test]
    fn lost_reply_is_unknown_even_when_real_effect_happened() {
        let runtime = SessionToken::new();
        let publication = runtime.internal_publication_session();
        let value = AtomicUsize::new(0);
        {
            let _permit = publication.try_admit(&publication.generation()).unwrap();
            value.fetch_add(3, AtomicOrdering::SeqCst);
            // Model a dropped response *after* dispatch. It is forbidden to
            // infer failure and retry merely because the reply is absent.
        }
        runtime.invalidate();
        assert_eq!(value.load(AtomicOrdering::SeqCst), 3);
        assert_eq!(
            publication
                .try_admit(&publication.generation())
                .unwrap_err(),
            InternalAdmissionError::Expired
        );
    }
}
