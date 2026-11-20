//! Stage 6: explicit typed schema-evolution planning only.
//! Preview is read-only; applying migrations requires separately verified
//! recovery and authorization gates and is not implemented by this module.
use crate::{
    database::failed,
    maintenance::{SnapshotBudget, SqliteMaintenance, audit, denied},
    records::MAX_RECORD_BYTES,
};
use riesel_schema::{ModelDefinition, RieselModel};
use riesel_storage::StoreError;
use rusqlite::{Connection, OpenFlags};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use uuid::Uuid;

type Transformation = Box<dyn Fn(Value) -> Result<Value, StoreError> + Send + Sync>;

/// An explicit plan for non-destructive changes to existing model identities.
/// Both schema maps should come from Rust-derived models, not handwritten JSON.
pub struct ModelEvolution {
    source: BTreeMap<String, ModelDefinition>,
    target: BTreeMap<String, ModelDefinition>,
    transformations: BTreeMap<String, Transformation>,
}

#[derive(Debug, Clone, Copy)]
pub struct EvolutionLimits {
    pub snapshot: SnapshotBudget,
    pub max_elapsed: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvolutionPreview {
    pub records: u64,
    pub changed_models: usize,
    pub affected_records: u64,
    pub projected_json_bytes: u64,
}

impl ModelEvolution {
    /// A shape transition must retain all persistent model names. Addition,
    /// removal, or physical format alteration requires a separate decision.
    pub fn new(
        source: BTreeMap<String, ModelDefinition>,
        target: BTreeMap<String, ModelDefinition>,
    ) -> Result<Self, StoreError> {
        if source.is_empty() || !source.keys().eq(target.keys()) {
            return Err(denied("source and target must have the same nonempty model set"));
        }
        if source == target {
            return Err(denied("source and target have no model changes"));
        }
        Ok(Self {
            source,
            target,
            transformations: BTreeMap::new(),
        })
    }

    /// Register one deliberate Rust-authored transformation, validated against
    /// the exact historical and target Rust model definitions.
    pub fn transform<Old, New, F>(mut self, convert: F) -> Result<Self, StoreError>
    where
        Old: RieselModel + DeserializeOwned,
        New: RieselModel + Serialize,
        F: Fn(Old) -> Result<New, StoreError> + Send + Sync + 'static,
    {
        if Old::MODEL_NAME != New::MODEL_NAME {
            return Err(denied("renaming a persistent model is unsupported"));
        }
        let name = Old::MODEL_NAME;
        if self.source.get(name) != Some(&Old::model_definition())
            || self.target.get(name) != Some(&New::model_definition())
        {
            return Err(denied("historical/target Rust model definitions do not match plan"));
        }
        if self.source.get(name) == self.target.get(name) {
            return Err(denied("unchanged models must not register transformations"));
        }
        if self.transformations.contains_key(name) {
            return Err(denied("duplicate model transformation"));
        }

        self.transformations.insert(
            name.to_owned(),
            Box::new(move |value| {
                let old: Old = serde_json::from_value(value).map_err(failed)?;
                let new = convert(old)?;
                serde_json::to_value(new).map_err(failed)
            }),
        );
        Ok(self)
    }

    fn validate_for(
        &self,
        actual: &BTreeMap<String, ModelDefinition>,
    ) -> Result<(), StoreError> {
        if actual != &self.source {
            return Err(denied("plan source models differ from the owned database models"));
        }
        for (name, old) in &self.source {
            let new = self
                .target
                .get(name)
                .ok_or_else(|| denied("plan target model is missing"))?;
            if (old != new) != self.transformations.contains_key(name) {
                return Err(denied("every changed model needs exactly one typed transformation"));
            }
        }
        Ok(())
    }
}

impl SqliteMaintenance {
    /// Validate every planned transformation against a consistent, read-only
    /// SQLite snapshot. This does not reserve data, execute a migration, or
    /// authorize later execution; execute must re-check state and take backup.
    pub fn preview_evolution(
        &self,
        plan: &ModelEvolution,
        limits: EvolutionLimits,
    ) -> Result<EvolutionPreview, StoreError> {
        self.owner.verify()?;
        plan.validate_for(&self.models)?;
        if limits.max_elapsed.is_zero() {
            return Err(denied("evolution preview requires a positive elapsed-time limit"));
        }
        let started = Instant::now();
        let conn = Connection::open_with_flags(
            self.owner.database_path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(failed)?;
        conn.execute_batch("BEGIN").map_err(failed)?;
        let counts = audit(&conn, &self.models, limits.snapshot)?;
        let mut projected = counts.json_bytes;
        let mut affected = 0_u64;

        let mut stmt = conn
            .prepare("SELECT model,id,revision,length(data),data FROM records ORDER BY model,id")
            .map_err(failed)?;
        let mut rows = stmt.query([]).map_err(failed)?;
        while let Some(row) = rows.next().map_err(failed)? {
            if started.elapsed() > limits.max_elapsed {
                return Err(denied("evolution preview elapsed-time budget exceeded"));
            }
            let name: String = row.get(0).map_err(failed)?;
            let Some(transform) = plan.transformations.get(&name) else {
                continue;
            };
            let raw_id: Vec<u8> = row.get(1).map_err(failed)?;
            Uuid::from_slice(&raw_id).map_err(failed)?;
            let revision: Vec<u8> = row.get(2).map_err(failed)?;
            let revision: [u8; 8] = revision
                .try_into()
                .map_err(|_| denied("corrupt record revision"))?;
            if u64::from_be_bytes(revision) == u64::MAX {
                return Err(denied("model migration would overflow record revision"));
            }
            let size: i64 = row.get(3).map_err(failed)?;
            let size = u64::try_from(size).map_err(failed)?;
            if size > MAX_RECORD_BYTES as u64 {
                return Err(denied("source record exceeds Storage byte limit"));
            }
            let bytes: Vec<u8> = row.get(4).map_err(failed)?;
            let original: Value = serde_json::from_slice(&bytes).map_err(failed)?;
            let updated = transform(original)?;
            let schema = plan
                .target
                .get(&name)
                .ok_or_else(|| denied("target definition is missing"))?;
            schema.validate_record(&updated).map_err(failed)?;
            let changed_bytes = serde_json::to_vec(&updated).map_err(failed)?;
            if changed_bytes.len() > MAX_RECORD_BYTES {
                return Err(denied("target record exceeds Storage byte limit"));
            }
            projected = projected
                .checked_sub(size)
                .and_then(|bytes| bytes.checked_add(changed_bytes.len() as u64))
                .ok_or_else(|| denied("projected JSON byte budget overflow"))?;
            if projected > limits.snapshot.max_json_bytes {
                return Err(denied("projected JSON exceeds configured budget"));
            }
            affected = affected
                .checked_add(1)
                .ok_or_else(|| denied("affected record count overflow"))?;
            if started.elapsed() > limits.max_elapsed {
                return Err(denied("evolution preview elapsed-time budget exceeded"));
            }
        }
        self.owner.verify()?;
        Ok(EvolutionPreview {
            records: counts.records,
            changed_models: plan.transformations.len(),
            affected_records: affected,
            projected_json_bytes: projected,
        })
    }
}
