use crate::database::{SqliteStore, failed};
use riesel_storage::{MAX_PAGE_SIZE, RecordPage, RecordStore, StoreError, StoredRecord};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;
use uuid::Uuid;

pub(crate) const MAX_RECORD_BYTES: usize = 64 * 1024;

fn revision_bytes(revision: u64) -> [u8; 8] {
    revision.to_be_bytes()
}

fn decode_revision(bytes: &[u8]) -> Result<u64, StoreError> {
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| StoreError::Failed("corrupt stored record revision".into()))?;
    Ok(u64::from_be_bytes(bytes))
}

fn decode_id(bytes: &[u8]) -> Result<Uuid, StoreError> {
    Uuid::from_slice(bytes)
        .map_err(|_| StoreError::Failed("corrupt stored record identifier".into()))
}

impl SqliteStore {
    fn validate_model(&self, model: &str) -> Result<(), StoreError> {
        if self.models.contains_key(model) {
            Ok(())
        } else {
            Err(StoreError::Failed(format!("unknown model: {model}")))
        }
    }

    fn serialize(&self, model: &str, data: &Value) -> Result<Vec<u8>, StoreError> {
        let definition = self
            .models
            .get(model)
            .ok_or_else(|| StoreError::Failed(format!("unknown model: {model}")))?;
        definition.validate_record(data).map_err(failed)?;
        let bytes = serde_json::to_vec(data).map_err(failed)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(StoreError::Failed("record exceeds 64 KiB".into()));
        }
        Ok(bytes)
    }

    fn decode(
        &self,
        model: &str,
        id: Uuid,
        revision: Vec<u8>,
        data: Vec<u8>,
    ) -> Result<StoredRecord, StoreError> {
        let data: Value = serde_json::from_slice(&data).map_err(failed)?;
        self.serialize(model, &data)?;
        Ok(StoredRecord {
            id,
            data,
            revision: decode_revision(&revision)?,
        })
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, StoreError> {
        let conn = self
            .connection
            .lock()
            .map_err(|_| StoreError::Failed("SQLite connection lock poisoned".into()))?;
        self.verify_owner()?;
        Ok(conn)
    }
}

fn current_revision(conn: &Connection, model: &str, id: Uuid) -> Result<u64, StoreError> {
    let raw: Option<Vec<u8>> = conn
        .query_row(
            "SELECT revision FROM records WHERE model=?1 AND id=?2",
            params![model, &id.as_bytes()[..]],
            |row| row.get(0),
        )
        .optional()
        .map_err(failed)?;
    decode_revision(&raw.ok_or(StoreError::NotFound)?)
}

impl RecordStore for SqliteStore {
    fn create(&self, model: &str, data: Value) -> Result<StoredRecord, StoreError> {
        let bytes = self.serialize(model, &data)?;
        let mut conn = self.lock()?;
        for _ in 0..8 {
            let id = Uuid::new_v4();
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(failed)?;
            self.verify_write_registration(&tx)?;
            let affected = tx
                .execute(
                    "INSERT OR IGNORE INTO records(model,id,revision,data)
                     VALUES(?1,?2,?3,?4)",
                    params![model, &id.as_bytes()[..], &revision_bytes(1)[..], &bytes],
                )
                .map_err(failed)?;
            if affected == 0 {
                continue;
            }
            tx.commit().map_err(failed)?;
            return Ok(StoredRecord {
                id,
                data,
                revision: 1,
            });
        }
        Err(StoreError::Failed(
            "could not generate unique record identifier".into(),
        ))
    }

    fn get(&self, model: &str, id: Uuid) -> Result<Option<StoredRecord>, StoreError> {
        self.validate_model(model)?;
        let conn = self.lock()?;
        let row: Option<(Vec<u8>, Vec<u8>)> = conn
            .query_row(
                "SELECT revision,data FROM records WHERE model=?1 AND id=?2",
                params![model, &id.as_bytes()[..]],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(failed)?;
        row.map(|(revision, data)| self.decode(model, id, revision, data))
            .transpose()
    }

    fn list_page(
        &self,
        model: &str,
        after: Option<Uuid>,
        limit: u64,
    ) -> Result<RecordPage, StoreError> {
        if !(1..=MAX_PAGE_SIZE).contains(&limit) {
            return Err(StoreError::InvalidPageSize);
        }
        self.validate_model(model)?;
        let conn = self.lock()?;
        let mut stmt = conn
            .prepare(
                "SELECT id,revision,data FROM records
                 WHERE model=?1 AND (?2 IS NULL OR id>?2)
                 ORDER BY id LIMIT ?3",
            )
            .map_err(failed)?;
        let cursor = after.map(|uuid| uuid.as_bytes().to_vec());
        let mut query = stmt
            .query(params![model, cursor.as_deref(), (limit + 1) as i64])
            .map_err(failed)?;
        let mut records = Vec::new();
        while let Some(row) = query.next().map_err(failed)? {
            let id: Vec<u8> = row.get(0).map_err(failed)?;
            let revision: Vec<u8> = row.get(1).map_err(failed)?;
            let data: Vec<u8> = row.get(2).map_err(failed)?;
            records.push(self.decode(model, decode_id(&id)?, revision, data)?);
        }
        let next_cursor = if records.len() > limit as usize {
            records.pop();
            records.last().map(|record| record.id)
        } else {
            None
        };
        Ok(RecordPage {
            records,
            next_cursor,
        })
    }

    fn update(
        &self,
        model: &str,
        id: Uuid,
        expected_revision: u64,
        data: Value,
    ) -> Result<StoredRecord, StoreError> {
        let bytes = self.serialize(model, &data)?;
        let mut conn = self.lock()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failed)?;
        self.verify_write_registration(&tx)?;
        let revision = current_revision(&tx, model, id)?;
        if revision != expected_revision {
            return Err(StoreError::RevisionConflict);
        }
        let next = revision
            .checked_add(1)
            .ok_or_else(|| StoreError::Failed("revision overflow".into()))?;
        let changed = tx
            .execute(
                "UPDATE records SET revision=?1,data=?2
                 WHERE model=?3 AND id=?4 AND revision=?5",
                params![
                    &revision_bytes(next)[..],
                    bytes,
                    model,
                    &id.as_bytes()[..],
                    &revision_bytes(expected_revision)[..]
                ],
            )
            .map_err(failed)?;
        if changed != 1 {
            return Err(StoreError::RevisionConflict);
        }
        tx.commit().map_err(failed)?;
        Ok(StoredRecord {
            id,
            data,
            revision: next,
        })
    }

    fn delete(&self, model: &str, id: Uuid, expected_revision: u64) -> Result<(), StoreError> {
        self.validate_model(model)?;
        let mut conn = self.lock()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failed)?;
        self.verify_write_registration(&tx)?;
        if current_revision(&tx, model, id)? != expected_revision {
            return Err(StoreError::RevisionConflict);
        }
        let changed = tx
            .execute(
                "DELETE FROM records WHERE model=?1 AND id=?2 AND revision=?3",
                params![
                    model,
                    &id.as_bytes()[..],
                    &revision_bytes(expected_revision)[..]
                ],
            )
            .map_err(failed)?;
        if changed != 1 {
            return Err(StoreError::RevisionConflict);
        }
        tx.commit().map_err(failed)
    }
}
