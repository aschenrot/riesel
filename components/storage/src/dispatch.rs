use riesel_storage::{
    PROTOCOL_VERSION, RecordStore, StorageMessage, StorageRequest, StorageResponse, StorageResult,
    StorageServiceInfo, StoreError,
};

pub(crate) fn dispatch(
    store: &impl RecordStore,
    message: StorageMessage,
    info: &StorageServiceInfo,
) -> StorageResponse {
    if message.version != PROTOCOL_VERSION {
        return StorageResponse::Failure(StoreError::Failed(format!(
            "unsupported storage protocol version: {}",
            message.version
        )));
    }

    let result = match message.request {
        StorageRequest::Describe => Ok(StorageResult::ServiceInfo(info.clone())),
        StorageRequest::Create { model, data } => {
            store.create(&model, data).map(StorageResult::Record)
        }

        StorageRequest::Get { model, id } => {
            store.get(&model, id).map(StorageResult::OptionalRecord)
        }

        StorageRequest::List { model } => store.list(&model).map(StorageResult::Records),

        StorageRequest::Update {
            model,
            id,
            expected_revision,
            data,
        } => store
            .update(&model, id, expected_revision, data)
            .map(StorageResult::Record),
    };

    match result {
        Ok(value) => StorageResponse::Success(value),
        Err(error) => StorageResponse::Failure(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MemoryStore, memory_service_info};
    use serde_json::json;

    #[test]
    fn dispatches_create_request() {
        let store = MemoryStore::new();

        let message = StorageMessage {
            version: PROTOCOL_VERSION,
            request: StorageRequest::Create {
                model: "task".into(),
                data: json!({
                    "title": "Build Riesel",
                    "completed": false
                }),
            },
        };

        let response = dispatch(&store, message, &memory_service_info().unwrap());

        let StorageResponse::Success(StorageResult::Record(record)) = response else {
            panic!("expected a created record");
        };

        assert_eq!(record.revision, 1);
        assert_eq!(record.data["title"], "Build Riesel");

        let stored = store.get("task", record.id).unwrap();

        assert!(stored.is_some());
    }

    #[test]
    fn rejects_unsupported_protocol_version() {
        let store = MemoryStore::new();

        let message = StorageMessage {
            version: 999,
            request: StorageRequest::List {
                model: "task".into(),
            },
        };

        let response = dispatch(&store, message, &memory_service_info().unwrap());

        assert!(matches!(
            response,
            StorageResponse::Failure(StoreError::Failed(_))
        ));
    }
    #[test]
    fn describes_generated_provider_and_contract() {
        let store = MemoryStore::new();
        let message = StorageMessage {
            version: PROTOCOL_VERSION,
            request: StorageRequest::Describe,
        };
        let result = dispatch(&store, message, &memory_service_info().unwrap());
        let StorageResponse::Success(StorageResult::ServiceInfo(info)) = result else {
            panic!("expected advertised service info");
        };
        assert_eq!(info.provider.id, "riesel.storage.memory");
        assert_eq!(info.contract.capability, info.provider.capabilities[0]);
        info.contract.validate().unwrap();
    }
}
