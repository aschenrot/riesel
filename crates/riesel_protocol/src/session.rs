//! Session validity shared by generated local and remote capability handles.
//! A recovered application always begins with a new session token.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
#[cfg(feature = "research-external-epoch")]
use std::sync::{PoisonError, RwLock, RwLockReadGuard};


#[cfg(feature = "async-tcp")]
use tokio::sync::Notify;

// Only compiled by #75 research: this is not an accepted public/remote identity
// or authorization primitive. A run-time session's epoch is created when the
// existing runtime startup coordinator creates its SessionToken.
#[cfg(feature = "research-external-epoch")]
#[derive(Debug)]
struct ResearchExternalEpoch {
    generation: uuid::Uuid,
    dispatch_gate: RwLock<()>,
}

/// Test-only ingress permit. Holding it prevents an application session from
/// completing revocation while an admitted short synchronous operation runs.
/// Long/blocked calls can delay teardown; no general-purpose protocol or
/// production cancellation guarantee follows from this prototype.
#[cfg(feature = "research-external-epoch")]
#[derive(Debug)]
pub struct ResearchDispatchPermit<'a> {
    _gate: RwLockReadGuard<'a, ()>,
}

#[derive(Clone, Debug)]
pub struct SessionToken {
    active: Arc<AtomicBool>,
    #[cfg(feature = "research-external-epoch")]
    research_epoch: Arc<ResearchExternalEpoch>,
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
            #[cfg(feature = "research-external-epoch")]
            research_epoch: Arc::new(ResearchExternalEpoch {
                generation: uuid::Uuid::new_v4(),
                dispatch_gate: RwLock::new(()),
            }),
            #[cfg(feature = "async-tcp")]
            changed: Arc::new(Notify::new()),
        }
    }

    /// Called before an application session begins provider teardown.
    pub fn invalidate(&self) {
        // A test-only external ingress gate shares the authoritative runtime
        // token's invalidation path. Reads admitted before teardown may finish;
        // no new read can start once this write lock completes.
        #[cfg(feature = "research-external-epoch")]
        let _gate = self
            .research_epoch
            .dispatch_gate
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        self.active.store(false, Ordering::Release);
        #[cfg(feature = "async-tcp")]
        self.changed.notify_waiters();
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

    /// Research-only, runtime-token-issued public generation. It is NOT
    /// secret, trusted caller proof, server authentication or accepted API.
    #[cfg(feature = "research-external-epoch")]
    pub fn research_generation(&self) -> String {
        self.research_epoch.generation.to_string()
    }

    /// Research-only atomic admission against THIS runtime session's
    /// invalidation. Hold the returned permit through the short operation.
    /// This is an in-process lock, NOT a transport authentication boundary.
    #[cfg(feature = "research-external-epoch")]
    pub fn research_admit(
        &self,
        expected_generation: &str,
    ) -> Option<ResearchDispatchPermit<'_>> {
        let guard = self
            .research_epoch
            .dispatch_gate
            .read()
            .unwrap_or_else(PoisonError::into_inner);
        if expected_generation != self.research_epoch.generation.to_string() || !self.is_active() {
            return None;
        }
        Some(ResearchDispatchPermit { _gate: guard })
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

#[cfg(all(test, feature = "research-external-epoch"))]
mod external_epoch_research_tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn runtime_session_tokens_own_distinct_generations() {
        let first = SessionToken::new();
        let same = first.clone();
        let second = SessionToken::new();
        let generation = first.research_generation();
        assert_eq!(same.research_generation(), generation);
        assert_ne!(generation, second.research_generation());
        assert!(first.research_admit(&generation).is_some());
        assert!(first.research_admit(&second.research_generation()).is_none());
        first.invalidate();
        assert!(first.research_admit(&generation).is_none());
        assert!(same.research_admit(&generation).is_none());
        assert!(second.research_admit(&second.research_generation()).is_some());
    }

    #[test]
    fn revocation_waits_for_admitted_effect_and_excludes_late_calls() {
        let token = SessionToken::new();
        let old = token.clone();
        let generation = token.research_generation();
        let permit = token.research_admit(&generation).expect("must admit");
        let (entered_tx, entered_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            entered_tx.send(()).unwrap();
            old.invalidate();
            finished_tx.send(()).unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(
            finished_rx.recv_timeout(Duration::from_millis(30)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        drop(permit);
        finished_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        worker.join().unwrap();
        assert!(!token.is_active());
        assert!(token.research_admit(&generation).is_none());
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
