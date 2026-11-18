//! True nonblocking Tokio TCP implementation of generated capability RPC.
//! Each request owns a socket and can be cancelled by dropping its future.
//! No implicit retries: a timed-out command may already have executed.

use super::*;
use crate::AsyncRpcTransport;
use std::future::Future;
use std::pin::Pin;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream as AsyncTcpStream;
use tokio::time::timeout;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct AsyncTcpRpcTransport {
    address: SocketAddr,
    session: SessionToken,
}

impl AsyncTcpRpcTransport {
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            session: SessionToken::new(),
        }
    }

    /// Attach the transport to the application session. Clones share
    /// revocation, including across managed service recovery.
    pub fn with_session(address: SocketAddr, session: SessionToken) -> Self {
        Self { address, session }
    }

    async fn exchange(
        &self,
        capability: &CapabilityRef,
        operation: &str,
        arguments: Value,
    ) -> Result<Value, RpcError> {
        self.session.check()?;
        if !self.address.ip().is_loopback() {
            return Err(RpcError::Transport(
                "plaintext RPC is restricted to loopback addresses".into(),
            ));
        }

        let mut request = serde_json::to_vec(&WireRequest {
            version: WIRE_VERSION,
            capability: capability.clone(),
            operation: operation.to_owned(),
            arguments,
        })
        .map_err(|error| RpcError::Codec(error.to_string()))?;
        if request.len() + 1 > wire::MAX_FRAME_BYTES {
            return Err(RpcError::Transport("RPC request is too large".into()));
        }
        request.push(b'\n');

        let mut stream = timeout(CONNECT_TIMEOUT, AsyncTcpStream::connect(self.address))
            .await
            .map_err(|_| RpcError::Transport("RPC connection timed out".into()))?
            .map_err(|error| RpcError::Transport(error.to_string()))?;
        self.session.check()?;

        // The socket is nonblocking and dropped on timeout/cancellation.
        // The bounded response buffer prevents untrusted large frames.
        let response: WireResponse = timeout(REQUEST_TIMEOUT, async {
            stream
                .write_all(&request)
                .await
                .map_err(|error| RpcError::Transport(error.to_string()))?;

            let mut frame = Vec::new();
            let mut buffer = [0_u8; 8192];
            loop {
                let read = stream
                    .read(&mut buffer)
                    .await
                    .map_err(|error| RpcError::Transport(error.to_string()))?;
                if read == 0 {
                    return Err(RpcError::Transport(
                        "RPC connection closed before complete frame".into(),
                    ));
                }
                let newline = buffer[..read].iter().position(|byte| *byte == b'\n');
                let length = newline.map_or(read, |index| index + 1);
                if frame.len() + length > wire::MAX_FRAME_BYTES {
                    return Err(RpcError::Transport("RPC frame is too large".into()));
                }
                frame.extend_from_slice(&buffer[..length]);
                if newline.is_some() {
                    return serde_json::from_slice::<WireResponse>(&frame)
                        .map_err(|error| RpcError::Codec(error.to_string()));
                }
            }
        })
        .await
        .map_err(|_| RpcError::Transport("RPC request timed out".into()))??;

        // Cancellation/shutdown might have happened after the command was
        // sent. Never replay it, and don't expose results from dead sessions.
        self.session.check()?;
        if response.version != WIRE_VERSION {
            return Err(RpcError::Transport(format!(
                "unsupported RPC response protocol version: {}",
                response.version
            )));
        }
        response.result
    }

    /// Inspect remote contract shape (not a service authentication step).
    pub async fn describe(
        &self,
        capability: &CapabilityRef,
    ) -> Result<CapabilityContract, RpcError> {
        let value = self.call(capability, DESCRIBE, serde_json::json!({})).await?;
        let contract: CapabilityContract =
            serde_json::from_value(value).map_err(|error| RpcError::Codec(error.to_string()))?;
        contract
            .validate()
            .map_err(|error| RpcError::Contract(error.to_string()))?;
        Ok(contract)
    }

    pub async fn verify(&self, expected: &CapabilityContract) -> Result<(), RpcError> {
        let actual = self.describe(&expected.capability).await?;
        if actual.has_same_api_shape(expected) {
            Ok(())
        } else {
            Err(RpcError::Contract(
                "remote capability API differs from the expected contract".into(),
            ))
        }
    }
}

impl AsyncRpcTransport for AsyncTcpRpcTransport {
    fn call<'a>(
        &'a self,
        capability: &'a CapabilityRef,
        operation: &'a str,
        arguments: Value,
    ) -> Pin<Box<dyn Future<Output = Result<Value, RpcError>> + Send + 'a>> {
        Box::pin(self.exchange(capability, operation, arguments))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
    }

    #[test]
    fn rejects_public_network_for_unauthenticated_transport() {
        let cap = CapabilityRef {
            id: "example.counter".into(),
            version: 1,
        };
        let transport = AsyncTcpRpcTransport::new("8.8.8.8:443".parse().unwrap());
        assert!(matches!(
            runtime().block_on(transport.call(&cap, "value", serde_json::json!({}))),
            Err(RpcError::Transport(_))
        ));
    }

    #[test]
    fn request_future_can_be_cancelled_without_blocking_the_executor() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            // Accept the request, deliberately send no response.
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0_u8; 512];
            let _ = stream.read(&mut buf);
            thread::sleep(Duration::from_millis(150));
        });
        let cap = CapabilityRef {
            id: "example.counter".into(),
            version: 1,
        };
        let transport = AsyncTcpRpcTransport::new(address);
        let cancelled = runtime().block_on(async {
            tokio::time::timeout(
                Duration::from_millis(30),
                transport.call(&cap, "value", serde_json::json!({})),
            )
            .await
        });
        assert!(cancelled.is_err(), "pending network request did not yield");
        server.join().unwrap();
    }

    #[test]
    fn expired_session_rejects_before_connecting() {
        let session = SessionToken::new();
        let transport = AsyncTcpRpcTransport::with_session(
            "127.0.0.1:1".parse().unwrap(),
            session.clone(),
        );
        session.invalidate();
        let cap = CapabilityRef {
            id: "example.counter".into(),
            version: 1,
        };
        assert!(matches!(
            runtime().block_on(transport.call(&cap, "value", serde_json::json!({}))),
            Err(RpcError::SessionExpired)
        ));
    }
}
