//! Generated-contract-first remote connector: no Storage-specific methods or
//! protocol adapter. Capability macros opt into RpcClientBinding once.
use riesel_app::{CapabilityConnector, ConnectedProvider, ConnectorError, ServiceEndpoint};
#[cfg(feature = "async-tcp")]
use riesel_protocol::{AsyncRpcClientBinding, AsyncRpcTransport, AsyncTcpRpcTransport};
use riesel_protocol::{RpcClientBinding, SessionToken, TcpRpcTransport};
use riesel_schema::CapabilityContract;
use std::io;
use std::marker::PhantomData;

pub struct RpcConnector<C>(PhantomData<fn() -> C>);

impl<C> Default for RpcConnector<C> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<C> RpcConnector<C> {
    pub fn new() -> Self {
        Self::default()
    }
}

impl<C: RpcClientBinding> CapabilityConnector for RpcConnector<C> {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        Ok(C::definition()?)
    }

    fn connect(
        &self,
        endpoint: &ServiceEndpoint,
        session: &SessionToken,
    ) -> Result<ConnectedProvider, ConnectorError> {
        let address = endpoint.address().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "RPC connector needs a TCP endpoint",
            )
        })?;
        let expected = C::definition()?;
        let transport = TcpRpcTransport::with_session(address, session.clone());

        let contract = transport.describe(&expected.capability)?;
        if !contract.has_same_api_shape(&expected) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "remote capability contract differs from Rust declaration",
            )
            .into());
        }

        // An advertised ID is selection metadata, not authentication. An
        // untrusted remote requires authenticated transport before production.
        let provider = transport.provider(&expected.capability)?;
        let client = C::bind(std::sync::Arc::new(transport));
        Ok(ConnectedProvider {
            definition: provider,
            contract,
            service: Box::new(C::guard_local(client, session.clone())),
        })
    }
}

#[cfg(feature = "async-tcp")]
pub struct AsyncRpcConnector<C>(PhantomData<fn() -> C>);

#[cfg(feature = "async-tcp")]
impl<C> Default for AsyncRpcConnector<C> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

#[cfg(feature = "async-tcp")]
impl<C> AsyncRpcConnector<C> {
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(feature = "async-tcp")]
impl<C: AsyncRpcClientBinding> CapabilityConnector for AsyncRpcConnector<C> {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        Ok(C::definition()?)
    }

    fn accepts_local_service(&self) -> bool {
        false
    }

    fn connect(
        &self,
        endpoint: &ServiceEndpoint,
        session: &SessionToken,
    ) -> Result<ConnectedProvider, ConnectorError> {
        let address = endpoint.address().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "async RPC needs a TCP endpoint",
            )
        })?;
        let expected = C::definition()?;
        // Startup/selection is synchronous today; inspect the remote
        // contract before returning a nonblocking business-call transport.
        // Both paths use the same capability ID, provider and session.
        // Startup inspection is one-shot, so do not pool temporary TCP
        // sockets; single-request hosts and managed multiplexers both work.
        session.check()?;
        let inspection = TcpRpcTransport::new(address);
        let contract = inspection.describe(&expected.capability)?;
        if !contract.has_same_api_shape(&expected) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "remote async capability contract differs from Rust declaration",
            )
            .into());
        }
        let provider = inspection.provider(&expected.capability)?;
        session.check()?;
        let transport: std::sync::Arc<dyn AsyncRpcTransport> =
            std::sync::Arc::new(AsyncTcpRpcTransport::with_session(address, session.clone()));
        Ok(ConnectedProvider {
            definition: provider,
            contract,
            service: Box::new(C::bind_async(transport)),
        })
    }
}
