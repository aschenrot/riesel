//! Versioned Counter capability contract. The implementation lives in
//! components/counter and can be replaced without modifying callers.

use riesel_macros::{RieselType, capability};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize, RieselType)]
pub enum CounterError {
    #[error("increment must be greater than zero")]
    ZeroIncrement,
    #[error("counter would overflow")]
    Overflow,
    #[error("counter transport failed: {0}")]
    Transport(String),
    #[error("counter internal failure: {0}")]
    Internal(String),
}

impl From<riesel_protocol::RpcError> for CounterError {
    fn from(error: riesel_protocol::RpcError) -> Self {
        Self::Transport(error.to_string())
    }
}

#[capability(id = "riesel.counter.operations", version = 2, client_trait)]
pub trait Counter {
    /// Return the current count without modifying it.
    #[query]
    fn value(&self) -> Result<u64, CounterError>;

    /// Increment the counter with overflow protection.
    #[command]
    fn add(&self, amount: u64) -> Result<u64, CounterError>;

    /// Return a string label built from the current count.
    #[query]
    fn label(&self, prefix: &str) -> Result<String, CounterError>;
}

/// Independent read-only view over the same Counter instance. A managed
/// component can expose this alongside Counter without spawning another
/// process or creating a second copy of state.
#[capability(id = "riesel.counter.observer", version = 1, client_trait)]
pub trait CounterObserver {
    /// Observe the current count without changing it.
    #[query]
    fn observe(&self) -> Result<u64, CounterError>;
}

/// Native async version of Counter. It uses the same domain error model
/// but is independently versioned from the published synchronous contract.
#[capability(id = "riesel.counter.async.operations", version = 1)]
pub trait AsyncCounter {
    /// Read the current asynchronous counter value.
    #[query]
    async fn value(&self) -> Result<u64, CounterError>;

    /// Add a nonzero increment without wrapping on overflow.
    #[command]
    async fn add(&self, amount: u64) -> Result<u64, CounterError>;

    /// Render a prefix and the current counter value.
    #[query]
    async fn label(&self, prefix: &str) -> Result<String, CounterError>;
}

/// An independent native async read-only view over the same component state.
#[capability(id = "riesel.counter.async.observer", version = 1)]
pub trait AsyncCounterObserver {
    /// Observe the shared native async counter.
    #[query]
    async fn observe(&self) -> Result<u64, CounterError>;
}

#[cfg(test)]
#[allow(dead_code)]
mod private_identity_tests {
    use super::*;

    #[capability]
    pub trait PrivateCounter {
        /// Read the counter.
        #[query]
        fn value(&self) -> Result<u64, CounterError>;
    }

    struct PrivateImplementation;

    impl PrivateCounter for PrivateImplementation {
        fn value(&self) -> Result<u64, CounterError> {
            Ok(7)
        }
    }

    #[test]
    fn private_declaration_needs_no_id_and_supports_generated_dispatch() {
        use riesel_protocol::RpcHandler;

        let contract = PrivateCounterContract::definition().unwrap();
        assert_eq!(contract.capability.version, 1);
        assert_eq!(
            contract.capability.id,
            riesel_schema::CapabilityRef::private_rust_id(module_path!(), "PrivateCounter")
        );
        let handler = PrivateCounterRpcDispatcher::new(PrivateImplementation);
        assert_eq!(handler.contract().unwrap().capability, contract.capability);
        assert!(handler.dispatch("value", serde_json::json!({})).is_ok());
    }
}

#[cfg(test)]
mod native_async_capability_tests {
    use super::*;
    use riesel_protocol::{
        AsyncRpcClientBinding, AsyncRpcHandler, AsyncTcpRpcTransport, CallError, RpcError,
        SessionToken,
    };
    use std::future::Future;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll, Waker};

    #[capability]
    trait AsyncCounter {
        /// Read the current value without changing it.
        #[query]
        async fn value(&self) -> Result<u64, CounterError>;

        /// Add a nonzero increment.
        #[command]
        async fn add(&self, increment: u64) -> Result<u64, CounterError>;

        /// Label the value using a borrowed string.
        #[query]
        async fn label(&self, prefix: &str) -> Result<String, CounterError>;
    }

    #[derive(Default)]
    struct AsyncCounterState(Mutex<u64>);

    impl AsyncCounter for AsyncCounterState {
        async fn value(&self) -> Result<u64, CounterError> {
            Ok(*self.0.lock().unwrap())
        }

        async fn add(&self, increment: u64) -> Result<u64, CounterError> {
            if increment == 0 {
                return Err(CounterError::ZeroIncrement);
            }
            let mut value = self.0.lock().unwrap();
            *value += increment;
            Ok(*value)
        }

        async fn label(&self, prefix: &str) -> Result<String, CounterError> {
            Ok(format!("{prefix}: {}", self.value().await?))
        }
    }

    fn ready<F: Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("native async test unexpectedly requires a scheduler"),
        }
    }

    #[test]
    fn native_async_capability_uses_generated_local_handle() {
        let session = SessionToken::new();
        let state = Arc::new(AsyncCounterState::default());
        let handle = AsyncCounterLocalClient::new(state, session.clone());
        assert_eq!(ready(handle.value()).unwrap(), 0);
        assert_eq!(ready(handle.add(4)).unwrap(), 4);
        assert_eq!(ready(handle.label("count")).unwrap(), "count: 4");
        assert!(matches!(
            ready(handle.add(0)),
            Err(CallError::Domain(CounterError::ZeroIncrement))
        ));
        session.invalidate();
        assert!(matches!(
            ready(handle.value()),
            Err(CallError::Rpc(RpcError::SessionExpired))
        ));
    }

    #[test]
    fn generated_native_async_contract_runs_over_nonblocking_tokio_host() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let stopping = Arc::new(AtomicBool::new(false));
            let handler = Arc::new(AsyncCounterAsyncRpcDispatcher::new(
                AsyncCounterState::default(),
            ));
            let host = tokio::spawn(riesel_protocol::serve_async_tcp_rpc_host(
                listener,
                handler,
                Arc::clone(&stopping),
            ));

            let transport = Arc::new(AsyncTcpRpcTransport::new(address));
            let client = AsyncCounterContract::bind_async(transport);
            assert_eq!(client.value().await.unwrap(), 0);
            assert_eq!(client.add(9).await.unwrap(), 9);
            assert_eq!(client.label("async").await.unwrap(), "async: 9");
            assert!(matches!(
                client.add(0).await,
                Err(CallError::Domain(CounterError::ZeroIncrement))
            ));

            stopping.store(true, Ordering::Release);
            tokio::time::timeout(std::time::Duration::from_secs(2), host)
                .await
                .expect("native async host did not shut down")
                .unwrap()
                .unwrap();
        });
    }

    #[test]
    fn native_async_dispatcher_preserves_wire_contract_and_errors() {
        let handler = AsyncCounterAsyncRpcDispatcher::new(AsyncCounterState::default());
        let contract = AsyncCounterContract::definition().unwrap();
        assert_eq!(handler.contract().unwrap(), contract);
        let reference = contract.capability.clone();
        assert_eq!(
            ready(handler.dispatch_for(&reference, "add", serde_json::json!({"increment": 5})))
                .unwrap(),
            serde_json::json!({"Ok":5})
        );
        let result =
            ready(handler.dispatch_for(&reference, "value", serde_json::json!({}))).unwrap();
        assert_eq!(result, serde_json::json!({"Ok":5}));
        assert!(matches!(
            ready(handler.dispatch_for(&reference, "missing", serde_json::json!({}))),
            Err(RpcError::UnknownOperation(_))
        ));
    }
}
