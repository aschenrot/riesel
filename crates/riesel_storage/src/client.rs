use crate::protocol::StorageServiceInfo;
use crate::protocol::{
    PROTOCOL_VERSION, StorageMessage, StorageRequest, StorageResponse, StorageResult,
};
use crate::storage::{RecordStore, RecordStoreContract, StoreError, StoredRecord};
use riesel_schema::CapabilityRef;

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;
use uuid::Uuid;

const MAX_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

pub struct TcpStore {
    address: SocketAddr,
}

impl TcpStore {
    pub fn new(address: SocketAddr) -> Self {
        Self { address }
    }

    /// Read the remote provider identity and generated capability definition.
    pub fn describe(&self) -> Result<StorageServiceInfo, StoreError> {
        match self.send(StorageRequest::Describe)? {
            StorageResult::ServiceInfo(info) => Ok(info),
            _ => Err(unexpected_response()),
        }
    }

    /// Reject endpoints whose advertised provider or contract is not the
    /// one selected by application resolution. Does not authenticate the server.
    pub fn verify(
        &self,
        expected_provider: &str,
        expected_capability: &CapabilityRef,
    ) -> Result<(), StoreError> {
        Self::verify_service_info(&self.describe()?, expected_provider, expected_capability)
    }

    /// Verify an already received service advertisement without a second
    /// network round trip. This checks compatibility, not authenticity.
    pub fn verify_service_info(
        info: &StorageServiceInfo,
        expected_provider: &str,
        expected_capability: &CapabilityRef,
    ) -> Result<(), StoreError> {
        info.provider
            .validate()
            .map_err(|e| failed("invalid provider advertisement", e))?;
        info.contract
            .validate()
            .map_err(|e| failed("invalid contract advertisement", e))?;

        if info.provider.id != expected_provider {
            return Err(StoreError::Failed(format!(
                "unexpected storage provider: expected {expected_provider}, got {}",
                info.provider.id
            )));
        }
        if !info.provider.capabilities.contains(expected_capability)
            || info.contract.capability != *expected_capability
        {
            return Err(StoreError::Failed(format!(
                "storage provider does not implement {}@{}",
                expected_capability.id, expected_capability.version
            )));
        }
        let local =
            RecordStoreContract::definition().map_err(|e| failed("local storage contract", e))?;
        if !info.contract.has_same_api_shape(&local) {
            return Err(StoreError::Failed(
                "remote storage API shape differs from the local Rust contract".into(),
            ));
        }
        Ok(())
    }

    fn send(&self, request: StorageRequest) -> Result<StorageResult, StoreError> {
        let mut stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(3))
            .map_err(|e| failed("connection", e))?;

        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| failed("read timeout setup", e))?;

        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| failed("write timeout setup", e))?;

        let message = StorageMessage {
            version: PROTOCOL_VERSION,
            request,
        };

        let mut bytes =
            serde_json::to_vec(&message).map_err(|e| failed("request serialization", e))?;

        bytes.push(b'\n');

        stream
            .write_all(&bytes)
            .map_err(|e| failed("sending request", e))?;

        let mut reader = BufReader::new(stream);
        let mut response_bytes = Vec::new();

        loop {
            let buffer = reader
                .fill_buf()
                .map_err(|e| failed("reading response", e))?;

            if buffer.is_empty() {
                return Err(StoreError::Failed(
                    "connection closed before complete response".into(),
                ));
            }

            let newline = buffer.iter().position(|&b| b == b'\n');

            let length = newline.map(|index| index + 1).unwrap_or(buffer.len());

            if response_bytes.len() + length > MAX_RESPONSE_BYTES {
                return Err(StoreError::Failed("storage response too large".into()));
            }

            response_bytes.extend_from_slice(&buffer[..length]);
            reader.consume(length);

            if newline.is_some() {
                break;
            }
        }

        let response: StorageResponse = serde_json::from_slice(&response_bytes)
            .map_err(|e| failed("response deserialization", e))?;

        match response {
            StorageResponse::Success(result) => Ok(result),
            StorageResponse::Failure(error) => Err(error),
        }
    }
}

impl RecordStore for TcpStore {
    fn create(&self, model: &str, data: Value) -> Result<StoredRecord, StoreError> {
        match self.send(StorageRequest::Create {
            model: model.into(),
            data,
        })? {
            StorageResult::Record(record) => Ok(record),
            _ => Err(unexpected_response()),
        }
    }

    fn get(&self, model: &str, id: Uuid) -> Result<Option<StoredRecord>, StoreError> {
        match self.send(StorageRequest::Get {
            model: model.into(),
            id,
        })? {
            StorageResult::OptionalRecord(record) => Ok(record),
            _ => Err(unexpected_response()),
        }
    }

    fn list(&self, model: &str) -> Result<Vec<StoredRecord>, StoreError> {
        match self.send(StorageRequest::List {
            model: model.into(),
        })? {
            StorageResult::Records(records) => Ok(records),
            _ => Err(unexpected_response()),
        }
    }

    fn update(
        &self,
        model: &str,
        id: Uuid,
        expected_revision: u64,
        data: Value,
    ) -> Result<StoredRecord, StoreError> {
        match self.send(StorageRequest::Update {
            model: model.into(),
            id,
            expected_revision,
            data,
        })? {
            StorageResult::Record(record) => Ok(record),
            _ => Err(unexpected_response()),
        }
    }
}

fn failed(context: &str, error: impl std::fmt::Display) -> StoreError {
    StoreError::Failed(format!("{context}: {error}"))
}

fn unexpected_response() -> StoreError {
    StoreError::Failed("unexpected storage response type".into())
}
