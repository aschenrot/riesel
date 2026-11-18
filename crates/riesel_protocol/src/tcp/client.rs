use super::*;

/// Client for the transport-neutral generated RPC API.
/// Each call opens one connection; connection pooling can be added later.
pub struct TcpRpcTransport {
    address: SocketAddr,
    session: SessionToken,
}

impl TcpRpcTransport {
    pub fn new(address: SocketAddr) -> Self {
        Self::with_session(address, SessionToken::new())
    }

    pub fn with_session(address: SocketAddr, session: SessionToken) -> Self {
        Self { address, session }
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

