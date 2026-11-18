//! Independent capability example. No handwritten request enum, client
//! proxy, dispatcher, or per-operation protocol table.

use riesel_macros::{RieselType, capability};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize, RieselType)]
pub enum CounterError {
    #[error("increment must be greater than zero")]
    ZeroIncrement,
    #[error("counter would overflow")]
    Overflow,
}

#[capability(id = "riesel.counter.operations", version = 1)]
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

#[derive(Default)]
pub struct MemoryCounter {
    value: Mutex<u64>,
}

impl Counter for MemoryCounter {
    fn value(&self) -> Result<u64, CounterError> {
        Ok(*self.value.lock().expect("counter mutex poisoned"))
    }

    fn add(&self, amount: u64) -> Result<u64, CounterError> {
        if amount == 0 {
            return Err(CounterError::ZeroIncrement);
        }
        let mut value = self.value.lock().expect("counter mutex poisoned");
        *value = value.checked_add(amount).ok_or(CounterError::Overflow)?;
        Ok(*value)
    }

    fn label(&self, prefix: &str) -> Result<String, CounterError> {
        Ok(format!("{prefix}: {}", self.value()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riesel_protocol::{
        CallError, LocalRpc, RpcError, RpcHandler, RpcTransport, TcpRpcTransport,
        handle_tcp_connection,
    };
    use std::net::TcpListener;
    use std::thread;
    use std::time::{Duration, Instant};
    use serde_json::json;

    #[test]
    fn generated_client_and_dispatcher_support_commands_and_queries() {
        let handler = CounterRpcDispatcher::new(MemoryCounter::default());
        let client = CounterRpcClient::new(LocalRpc::new(handler));

        assert_eq!(client.value().unwrap(), 0);
        assert_eq!(client.add(3).unwrap(), 3);
        assert_eq!(client.label("count").unwrap(), "count: 3");
        assert!(matches!(
            client.add(0),
            Err(CallError::Domain(CounterError::ZeroIncrement))
        ));
        assert_eq!(client.value().unwrap(), 3);
    }

    #[test]
    fn generated_dispatcher_validates_operations_and_argument_shapes() {
        let handler = CounterRpcDispatcher::new(MemoryCounter::default());
        assert_eq!(
            handler.contract().unwrap().capability,
            CounterContract::definition().unwrap().capability
        );
        assert!(matches!(
            handler.dispatch("unknown", json!({})),
            Err(RpcError::UnknownOperation(_))
        ));
        assert!(matches!(
            handler.dispatch("add", json!({"amount": "not a number"})),
            Err(RpcError::InvalidArguments(_))
        ));
        assert!(matches!(
            handler.dispatch("value", json!({"extra": true})),
            Err(RpcError::InvalidArguments(_))
        ));
    }

    #[test]
    fn local_transport_rejects_a_different_capability_or_version() {
        let transport = LocalRpc::new(CounterRpcDispatcher::new(MemoryCounter::default()));
        let mut capability = CounterContract::definition().unwrap().capability;
        capability.version += 1;
        assert!(matches!(
            transport.call(&capability, "value", json!({})),
            Err(RpcError::WrongCapability { .. })
        ));
    }

    #[test]
    fn domain_overflow_is_not_a_transport_error() {
        let handler = CounterRpcDispatcher::new(MemoryCounter::default());
        let client = CounterRpcClient::new(LocalRpc::new(handler));
        assert_eq!(client.add(u64::MAX).unwrap(), u64::MAX);
        assert!(matches!(
            client.add(1),
            Err(CallError::Domain(CounterError::Overflow))
        ));
    }
    #[test]
    fn generated_client_and_dispatcher_work_over_real_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();

        let server = thread::spawn(move || {
            let handler = CounterRpcDispatcher::new(MemoryCounter::default());
            let deadline = Instant::now() + Duration::from_secs(8);
            let mut handled = 0;
            while handled < 6 {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        handle_tcp_connection(&handler, &mut stream).unwrap();
                        handled += 1;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        assert!(Instant::now() < deadline, "timed out waiting for RPC requests");
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(error) => panic!("could not accept RPC connection: {error}"),
                }
            }
        });

        let transport = TcpRpcTransport::new(address);
        let contract = CounterContract::definition().unwrap();
        transport.verify(&contract).unwrap();
        assert!(matches!(
            transport.call(&contract.capability, "not_an_operation", json!({})),
            Err(RpcError::UnknownOperation(_))
        ));

        let client = CounterRpcClient::new(transport);
        assert_eq!(client.value().unwrap(), 0);
        assert_eq!(client.add(7).unwrap(), 7);
        assert_eq!(client.label("value").unwrap(), "value: 7");
        assert!(matches!(
            client.add(0),
            Err(CallError::Domain(CounterError::ZeroIncrement))
        ));
        server.join().unwrap();
    }

}
