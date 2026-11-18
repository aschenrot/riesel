//! Generated-contract-first remote connector: no Storage-specific methods or
//! protocol adapter. Capability macros opt into RpcClientBinding once.
use riesel_app::{CapabilityConnector, ConnectedProvider, ConnectorError, ServiceEndpoint};
use riesel_protocol::{RpcClientBinding, TcpRpcTransport};
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

    fn connect(&self, endpoint: &ServiceEndpoint) -> Result<ConnectedProvider, ConnectorError> {
        let address = endpoint.address().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "RPC connector needs a TCP endpoint",
            )
        })?;
        let expected = C::definition()?;
        let transport = TcpRpcTransport::new(address);

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
        Ok(ConnectedProvider {
            definition: provider,
            contract,
            service: Box::new(C::bind(transport)),
        })
    }
}
