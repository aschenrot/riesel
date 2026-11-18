use crate::{RieselModel, SdkError};
use riesel_storage::{MAX_PAGE_SIZE, RecordStore, StoreError, StoredRecord};
use serde::{Serialize, de::DeserializeOwned};
use std::marker::PhantomData;
use std::sync::Arc;
use uuid::Uuid;

/// A stored Rust value with Riesel-managed identity and optimistic revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record<T> {
    pub id: Uuid,
    pub value: T,
    pub revision: u64,
}

/// Bounded, typed record page. The cursor is opaque to application logic.
pub struct RecordPage<T> {
    pub records: Vec<Record<T>>,
    pub next_cursor: Option<Uuid>,
}

/// Type-safe record storage, independent of local or remote providers.
pub struct Records<T> {
    store: Arc<dyn RecordStore + Send + Sync>,
    _model: PhantomData<T>,
}

impl<T> Clone for Records<T> {
    fn clone(&self) -> Self {
        Self {
            store: Arc::clone(&self.store),
            _model: PhantomData,
        }
    }
}

impl<T> Records<T>
where
    T: RieselModel + Serialize + DeserializeOwned,
{
    /// Construct directly for testing, custom injection or legacy integration.
    pub fn from_store(store: Arc<dyn RecordStore + Send + Sync>) -> Self {
        Self {
            store,
            _model: PhantomData,
        }
    }

    fn checked_json(value: &T) -> Result<serde_json::Value, SdkError> {
        let data = serde_json::to_value(value)?;
        T::model_definition().validate_record(&data)?;
        Ok(data)
    }

    fn decode(record: StoredRecord) -> Result<Record<T>, SdkError> {
        T::model_definition().validate_record(&record.data)?;
        Ok(Record {
            id: record.id,
            value: serde_json::from_value(record.data)?,
            revision: record.revision,
        })
    }

    pub fn create(&self, value: T) -> Result<Record<T>, SdkError> {
        let record = self
            .store
            .create(T::MODEL_NAME, Self::checked_json(&value)?)?;
        Self::decode(record)
    }

    pub fn get(&self, id: Uuid) -> Result<Option<Record<T>>, SdkError> {
        self.store
            .get(T::MODEL_NAME, id)?
            .map(Self::decode)
            .transpose()
    }

    pub fn list_page(&self, after: Option<Uuid>, limit: u64) -> Result<RecordPage<T>, SdkError> {
        let page = self.store.list_page(T::MODEL_NAME, after, limit)?;
        Ok(RecordPage {
            records: page
                .records
                .into_iter()
                .map(Self::decode)
                .collect::<Result<_, _>>()?,
            next_cursor: page.next_cursor,
        })
    }

    /// Convenience collector for small datasets. Consumers with potentially
    /// large collections should use list_page to maintain bounded memory.
    pub fn list(&self) -> Result<Vec<Record<T>>, SdkError> {
        let mut records = Vec::new();
        let mut after = None;
        loop {
            let page = self.list_page(after, MAX_PAGE_SIZE)?;
            records.extend(page.records);
            match page.next_cursor {
                Some(cursor) if after.is_some_and(|previous| cursor <= previous) => {
                    return Err(SdkError::Store(StoreError::Failed(
                        "record provider did not advance pagination cursor".into(),
                    )));
                }
                Some(cursor) => after = Some(cursor),
                None => return Ok(records),
            }
        }
    }

    /// Delete only the revision observed by the caller.
    pub fn delete(&self, id: Uuid, expected_revision: u64) -> Result<(), SdkError> {
        Ok(self.store.delete(T::MODEL_NAME, id, expected_revision)?)
    }

    pub fn update(
        &self,
        id: Uuid,
        expected_revision: u64,
        value: T,
    ) -> Result<Record<T>, SdkError> {
        let record = self.store.update(
            T::MODEL_NAME,
            id,
            expected_revision,
            Self::checked_json(&value)?,
        )?;
        Self::decode(record)
    }
}
