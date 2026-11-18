use super::*;

/// Client for the transport-neutral generated RPC API. Runtime sessions
/// retain a small pool of verified TCP sockets; standalone clients use one
/// connection per call for compatibility with one-request hosts.
struct IdleSocket {
    stream: TcpStream,
    last_used: std::time::Instant,
}

pub struct TcpRpcTransport {
    address: SocketAddr,
    session: SessionToken,
    idle: Option<Mutex<Vec<IdleSocket>>>,
}

const MAX_IDLE_SOCKETS: usize = 4;
const MAX_IDLE_AGE: Duration = Duration::from_millis(750);

impl TcpRpcTransport {
    pub fn new(address: SocketAddr) -> Self {
        Self {
            address,
            session: SessionToken::new(),
            idle: None,
        }
    }

    /// Application-owned, revocable TCP session with bounded connection reuse.
    /// A failed request is never replayed; commands may have already executed.
    pub fn with_session(address: SocketAddr, session: SessionToken) -> Self {
        Self {
            address,
            session,
            idle: Some(Mutex::new(Vec::new())),
        }
    }

    fn connect(&self) -> Result<TcpStream, RpcError> {
        if let Some(idle) = &self.idle {
            let mut sockets = idle.lock().unwrap_or_else(PoisonError::into_inner);
            while let Some(entry) = sockets.pop() {
                if entry.last_used.elapsed() <= MAX_IDLE_AGE {
                    return Ok(entry.stream);
                }
            }
        }
        let stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(3))
            .map_err(|error| RpcError::Transport(error.to_string()))?;
        configure_timeouts(&stream)?;
        Ok(stream)
    }

    fn recycle(&self, stream: TcpStream) {
        if !self.session.is_active() {
            return;
        }
        if let Some(idle) = &self.idle {
            let mut sockets = idle.lock().unwrap_or_else(PoisonError::into_inner);
            if sockets.len() < MAX_IDLE_SOCKETS {
                sockets.push(IdleSocket {
                    stream,
                    last_used: std::time::Instant::now(),
                });
            }
        }
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
        self.session.check()?;
        if !self.address.ip().is_loopback() {
            return Err(RpcError::Transport(
                "plaintext RPC is restricted to loopback addresses".into(),
            ));
        }
        let mut stream = self.connect()?;
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
        self.recycle(stream);
        response.result
    }
}
