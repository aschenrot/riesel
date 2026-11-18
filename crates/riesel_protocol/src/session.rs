//! Session validity shared by generated local and remote capability handles.
//! A recovered application always begins with a new session token.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Debug)]
pub struct SessionToken {
    active: Arc<AtomicBool>,
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
        }
    }

    /// Called before an application session begins provider teardown.
    pub fn invalidate(&self) {
        self.active.store(false, Ordering::Release);
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
