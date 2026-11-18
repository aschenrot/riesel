//! Bounded JSON-line TCP transport for generated capability RPC.
//! This first transport is **loopback-only** and has no authentication.

use crate::{RpcError, RpcHandler, RpcTransport};
use riesel_schema::{CapabilityContract, CapabilityRef, ProviderDefinition};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

const WIRE_VERSION: u32 = 1;
const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
const DESCRIBE: &str = "$describe";
const PROVIDER: &str = "$provider";

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRequest {
    version: u32,
    capability: CapabilityRef,
    operation: String,
    arguments: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireResponse {
    version: u32,
    result: Result<Value, RpcError>,
}

/// Client for the transport-neutral generated RPC API.
/// Each call opens one connection; connection pooling can be added later.
pub struct TcpRpcTransport {
    address: SocketAddr,
}

impl TcpRpcTransport {
    pub fn new(address: SocketAddr) -> Self {
        Self { address }
    }

    /// Inspect the server's contract before making typed calls.
    /// Structural compatibility is not cryptographic authentication.
    pub fn describe(&self, capability: &CapabilityRef) -> Result<CapabilityContract, RpcError> {
        let value = self.call(capability, DESCRIBE, serde_json::json!({}))?;
        let contract: CapabilityContract =
            serde_json::from_value(value).map_err(|error| RpcError::Codec(error.to_string()))?;
        contract
            .validate()
            .map_err(|error| RpcError::Contract(error.to_string()))?;
        Ok(contract)
    }

    /// Request provider identity; only an explicitly hosted service can
    /// advertise it. The identity is not cryptographic authentication.
    pub fn provider(&self, capability: &CapabilityRef) -> Result<ProviderDefinition, RpcError> {
        let value = self.call(capability, PROVIDER, serde_json::json!({}))?;
        let provider: ProviderDefinition =
            serde_json::from_value(value).map_err(|error| RpcError::Codec(error.to_string()))?;
        provider
            .validate()
            .map_err(|error| RpcError::Contract(error.to_string()))?;
        if !provider.capabilities.contains(capability) {
            return Err(RpcError::Contract(
                "provider does not implement the requested capability".into(),
            ));
        }
        Ok(provider)
    }

    pub fn verify(&self, expected: &CapabilityContract) -> Result<(), RpcError> {
        let actual = self.describe(&expected.capability)?;
        if actual.has_same_api_shape(expected) {
            Ok(())
        } else {
            Err(RpcError::Contract(
                "remote capability API differs from the expected contract".into(),
            ))
        }
    }
}

impl RpcTransport for TcpRpcTransport {
    fn call(
        &self,
        capability: &CapabilityRef,
        operation: &str,
        arguments: Value,
    ) -> Result<Value, RpcError> {
        if !self.address.ip().is_loopback() {
            return Err(RpcError::Transport(
                "plaintext RPC is restricted to loopback addresses".into(),
            ));
        }
        let mut stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(3))
            .map_err(|error| RpcError::Transport(error.to_string()))?;
        configure_timeouts(&stream)?;
        write_json_line(
            &mut stream,
            &WireRequest {
                version: WIRE_VERSION,
                capability: capability.clone(),
                operation: operation.to_owned(),
                arguments,
            },
        )?;
        let response: WireResponse = read_json_line(&mut stream)?;
        if response.version != WIRE_VERSION {
            return Err(RpcError::Transport(format!(
                "unsupported RPC response protocol version: {}",
                response.version
            )));
        }
        response.result
    }
}

/// Run a generated RPC host with the standard managed-service lifecycle.
/// The caller supplies only a typed handler and bind address; the shared
/// transport owns loopback enforcement, ready frames and private shutdown.
pub fn run_tcp_rpc_host<H: RpcHandler>(address: &str, handler: &H) -> io::Result<()> {
    let address: SocketAddr = address.parse().map_err(io::Error::other)?;
    if !address.ip().is_loopback() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unauthenticated RPC services must listen on loopback",
        ));
    }
    let listener = TcpListener::bind(address)?;
    let controlled = std::env::var_os("RIESEL_CONTROL_STDIN").is_some();
    let stopping = Arc::new(AtomicBool::new(false));

    if controlled {
        listener.set_nonblocking(true)?;
        let stopping = Arc::clone(&stopping);
        thread::spawn(move || {
            let mut request = String::new();
            if io::stdin().read_line(&mut request).is_ok()
                && request.trim_end() == "RIESEL_SHUTDOWN"
            {
                stopping.store(true, Ordering::Release);
            }
        });
    }

    let ready = serde_json::json!({
        "protocol": 1,
        "address": listener.local_addr()?.to_string(),
        "shutdown": controlled.then_some("stdin_line")
    });
    println!("RIESEL_READY {}", ready);
    io::stdout().flush()?;

    let serve = |connection| match connection {
        Ok(mut stream) => {
            if let Err(error) = handle_tcp_connection(handler, &mut stream) {
                eprintln!("RPC request failed: {error}");
            }
        }
        Err(error) => eprintln!("RPC connection failed: {error}"),
    };

    if controlled {
        while !stopping.load(Ordering::Acquire) {
            match listener.accept() {
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                other => serve(other.map(|(stream, _)| stream)),
            }
        }
    } else {
        for connection in listener.incoming() {
            serve(connection);
        }
    }

    Ok(())
}

/// Handle exactly one framed call on an already accepted TCP connection.
/// Host code owns listener lifetime, concurrency, and process management.
pub fn handle_tcp_connection(
    handler: &impl RpcHandler,
    stream: &mut TcpStream,
) -> Result<(), RpcError> {
    configure_timeouts(stream)?;
    let request: WireRequest = read_json_line(stream)?;
    let result = if request.version != WIRE_VERSION {
        Err(RpcError::Transport(format!(
            "unsupported RPC request protocol version: {}",
            request.version
        )))
    } else {
        let contract = handler.contract()?;
        if request.capability != contract.capability {
            Err(RpcError::WrongCapability {
                expected: format!("{}@{}", contract.capability.id, contract.capability.version),
                received: format!("{}@{}", request.capability.id, request.capability.version),
            })
        } else if request.operation == PROVIDER {
            if request.arguments != serde_json::json!({}) {
                Err(RpcError::InvalidArguments(
                    "provider inspection takes no arguments".into(),
                ))
            } else {
                handler
                    .provider()
                    .ok_or_else(|| RpcError::Contract("service has no registered provider".into()))
                    .and_then(|provider| {
                        serde_json::to_value(provider)
                            .map_err(|error| RpcError::Codec(error.to_string()))
                    })
            }
        } else if request.operation == DESCRIBE {
            if request.arguments != serde_json::json!({}) {
                Err(RpcError::InvalidArguments(
                    "contract inspection takes no arguments".into(),
                ))
            } else {
                serde_json::to_value(contract).map_err(|error| RpcError::Codec(error.to_string()))
            }
        } else {
            handler.dispatch(&request.operation, request.arguments)
        }
    };
    write_json_line(
        stream,
        &WireResponse {
            version: WIRE_VERSION,
            result,
        },
    )
}

fn configure_timeouts(stream: &TcpStream) -> Result<(), RpcError> {
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| RpcError::Transport(error.to_string()))?;
    stream
        .set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| RpcError::Transport(error.to_string()))
}

fn write_json_line<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<(), RpcError> {
    let mut data = serde_json::to_vec(value).map_err(|error| RpcError::Codec(error.to_string()))?;
    if data.len() + 1 > MAX_FRAME_BYTES {
        return Err(RpcError::Transport(
            "RPC request or response too large".into(),
        ));
    }
    data.push(b'\n');
    stream
        .write_all(&data)
        .map_err(|error| RpcError::Transport(error.to_string()))
}

fn read_json_line<T: DeserializeOwned>(stream: &mut TcpStream) -> Result<T, RpcError> {
    let mut reader = BufReader::new(stream);
    let mut bytes = Vec::new();
    loop {
        let buffer = reader
            .fill_buf()
            .map_err(|error| RpcError::Transport(error.to_string()))?;
        if buffer.is_empty() {
            return Err(RpcError::Transport(
                "RPC connection closed before complete frame".into(),
            ));
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let length = newline.map(|index| index + 1).unwrap_or(buffer.len());
        if bytes.len() + length > MAX_FRAME_BYTES {
            return Err(RpcError::Transport("RPC frame is too large".into()));
        }
        bytes.extend_from_slice(&buffer[..length]);
        reader.consume(length);
        if newline.is_some() {
            break;
        }
    }
    serde_json::from_slice(&bytes).map_err(|error| RpcError::Codec(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_loopback_plaintext_clients() {
        let client = TcpRpcTransport::new("8.8.8.8:443".parse().unwrap());
        let capability = CapabilityRef {
            id: "example.counter".into(),
            version: 1,
        };
        assert!(matches!(
            client.call(&capability, "value", serde_json::json!({})),
            Err(RpcError::Transport(_))
        ));
    }
}
