use crate::storage::{RecordStore, RecordStoreContract, TcpStore};
use riesel_app::{
    CapabilityConnector, ConnectedProvider, ConnectorError, ServiceEndpoint,
};
use riesel_schema::CapabilityContract;
use std::io;
use std::sync::Arc;

/// TCP adapter for the versioned RecordStore capability.
/// Riesel's generic runtime does not depend directly on Storage.
pub struct TcpRecordStoreConnector;

impl CapabilityConnector for TcpRecordStoreConnector {
    fn contract(&self) -> Result<CapabilityContract, ConnectorError> {
        Ok(RecordStoreContract::definition()?)
    }

    fn connect(&self, endpoint: &ServiceEndpoint) -> Result<ConnectedProvider, ConnectorError> {
        let ServiceEndpoint::Tcp { address } = endpoint;
        if address.port() == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "remote Storage endpoint cannot use port zero",
            )
            .into());
        }
        let store = TcpStore::new(*address);
        let info = store.describe()?;
        Ok(ConnectedProvider {
            definition: info.provider,
            contract: info.contract,
            service: Box::new(Arc::new(store) as Arc<dyn RecordStore + Send + Sync>),
        })
    }
}
