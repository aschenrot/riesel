//! Session validity shared by generated local and remote capability handles.
//! A recovered application always begins with a new session token.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[cfg(feature = "async-tcp")]
use tokio::sync::Notify;

#[derive(Clone, Debug)]
pub struct SessionToken {
    active: Arc<AtomicBool>,
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
            #[cfg(feature = "async-tcp")]
            changed: Arc::new(Notify::new()),
        }
    }

    /// Called before an application session begins provider teardown.
    pub fn invalidate(&self) {
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
            tokio::pin!(notified);
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
