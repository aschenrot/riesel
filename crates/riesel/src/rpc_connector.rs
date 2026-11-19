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
impl<C: AsyncRpcClientBinding> AsyncRpcConnector<C> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Inspect generated contract and provider metadata using the actual
    /// nonblocking Tokio transport, under the same application session as
    /// subsequent business calls. No synchronous TcpRpcTransport is involved.
    async fn inspect(
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
        session.check()?;
        let transport =
            std::sync::Arc::new(AsyncTcpRpcTransport::with_session(address, session.clone()));
        let contract = transport.describe(&expected.capability).await?;
        if !contract.has_same_api_shape(&expected) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "remote async capability contract differs from Rust declaration",
            )
            .into());
        }
        let provider = transport.provider(&expected.capability).await?;
        session.check()?;
        let client_transport: std::sync::Arc<dyn AsyncRpcTransport> = transport;
        Ok(ConnectedProvider {
            definition: provider,
            contract,
            service: Box::new(C::bind_async(client_transport)),
        })
    }
}

#[cfg(feature = "async-tcp")]
impl<C: AsyncRpcClientBinding> CapabilityConnector for AsyncRpcConnector<C> {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        Ok(C::definition()?)
    }

    fn local_binding_mode(&self) -> riesel_protocol::LocalBindingMode {
        riesel_protocol::LocalBindingMode::Async
    }

    fn supports_async_startup(&self) -> bool {
        true
    }

    fn connect(
        &self,
        endpoint: &ServiceEndpoint,
        session: &SessionToken,
    ) -> Result<ConnectedProvider, ConnectorError> {
        // The application builder is intentionally synchronous. A temporary
        // runtime drives real async inspection before the business executor
        // starts; no blocking socket is ever polled on a Tokio worker.
        // connect_async is available to the async startup orchestrator.
        if tokio::runtime::Handle::try_current().is_ok() {
            return Err(io::Error::other(
                "synchronous async-RPC startup cannot run inside a Tokio executor",
            )
            .into());
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        runtime.block_on(self.inspect(endpoint, session))
    }

    fn connect_async<'a>(
        &'a self,
        endpoint: &'a ServiceEndpoint,
        session: &'a SessionToken,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<ConnectedProvider, ConnectorError>> + Send + 'a,
        >,
    > {
        Box::pin(self.inspect(endpoint, session))
    }
}
