//! Constrained read-only access to the proposed transaction target.
use crate::{database::failed, deadline::Deadline, maintenance::denied, records::MAX_RECORD_BYTES};
use riesel_schema::ModelDefinition;
use riesel_storage::StoreError;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

/// A validated record read from the proposed target schema.
#[derive(Debug, Clone, PartialEq)]
pub struct EvolutionRecord {
    pub id: Uuid,
    pub revision: u64,
    pub data: Value,
}

/// Read-only target-state view for application-level invariant callbacks.
/// No database connection, SQL executor or mutation method is exposed.
pub struct EvolutionRecords<'a> {
    pub(crate) connection: &'a Connection,
    pub(crate) models: &'a BTreeMap<String, ModelDefinition>,
    pub(crate) deadline: Deadline,
}

impl<'a> EvolutionRecords<'a> {
    pub(crate) fn new(
        connection: &'a Connection,
        models: &'a BTreeMap<String, ModelDefinition>,
        deadline: Deadline,
    ) -> Self {
        Self {
            connection,
            models,
            deadline,
        }
    }

    fn definition(&self, model: &str) -> Result<&ModelDefinition, StoreError> {
        self.models
            .get(model)
            .ok_or_else(|| denied("invariant requested an unregistered model"))
    }

    fn decode(
        &self,
        model: &str,
        id: &[u8],
        revision: &[u8],
        bytes: &[u8],
    ) -> Result<EvolutionRecord, StoreError> {
        self.deadline.check()?;
        let id = Uuid::from_slice(id).map_err(failed)?;
        let revision: [u8; 8] = revision
            .try_into()
            .map_err(|_| denied("invariant found a corrupt record revision"))?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(denied("invariant found an oversized record"));
        }
        let data: Value = serde_json::from_slice(bytes).map_err(failed)?;
        self.definition(model)?
            .validate_record(&data)
            .map_err(failed)?;
        self.deadline.check()?;
        Ok(EvolutionRecord {
            id,
            revision: u64::from_be_bytes(revision),
            data,
        })
    }

    /// Read a validated record by model identity and UUID.
    pub fn get(&self, model: &str, id: Uuid) -> Result<Option<EvolutionRecord>, StoreError> {
        self.deadline.check()?;
        self.definition(model)?;
        let row: Option<(Vec<u8>, Vec<u8>)> = self
            .connection
            .query_row(
                "SELECT revision,data FROM records WHERE model=?1 AND id=?2",
                params![model, id.as_bytes()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(failed)?;
        row.map(|(revision, data)| self.decode(model, id.as_bytes(), &revision, &data))
            .transpose()
    }

    /// Visit a single model's records in UUID order without collecting rows.
    pub fn for_each<F>(&self, model: &str, mut visit: F) -> Result<(), StoreError>
    where
        F: FnMut(EvolutionRecord) -> Result<(), StoreError>,
    {
        self.deadline.check()?;
        self.definition(model)?;
        let mut statement = self
            .connection
            .prepare("SELECT id,revision,length(data),data FROM records WHERE model=?1 ORDER BY id")
            .map_err(failed)?;
        let mut rows = statement.query(params![model]).map_err(failed)?;
        while let Some(row) = rows.next().map_err(failed)? {
            self.deadline.check()?;
            let len: i64 = row.get(2).map_err(failed)?;
            if u64::try_from(len).map_err(failed)? > MAX_RECORD_BYTES as u64 {
                return Err(denied("invariant found an oversized record"));
            }
            let id: Vec<u8> = row.get(0).map_err(failed)?;
            let revision: Vec<u8> = row.get(1).map_err(failed)?;
            let data: Vec<u8> = row.get(3).map_err(failed)?;
            let record = self.decode(model, &id, &revision, &data)?;
            visit(record)?;
            self.deadline.check()?;
        }
        self.deadline.check()
    }
}
