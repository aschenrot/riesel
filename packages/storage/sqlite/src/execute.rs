//! Explicit, bounded typed SQLite model evolution. No automatic startup path.
//! One cooperative owner spans independent backup, transaction and verification.
use crate::{
    capacity::{check_sidecars, require_headroom},
    database::failed,
    deadline::Deadline,
    evolution::{EvolutionLimits, ModelEvolution},
    invariants::EvolutionRecords,
    maintenance::{
        SnapshotBudget, SqliteMaintenance, VerifiedSnapshot, audit, compare_records, denied,
    },
    owner::OwnerGuard,
    records::MAX_RECORD_BYTES,
};
use riesel_storage::StoreError;
use rusqlite::{
    Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params,
};
use serde_json::Value;
use std::path::Path;
use uuid::Uuid;

/// Explicit invocation intent. The future operator workflow must separately
/// establish human consent, preview identity, and legacy-process shutdown.
#[derive(Debug, Clone, Copy)]
pub enum EvolutionAuthorization {
    ExecuteWithVerifiedBackup,
}

#[derive(Debug)]
pub struct EvolutionReceipt {
    pub retained_backup: VerifiedSnapshot,
    pub records: u64,
    pub affected_records: u64,
    pub json_bytes: u64,
}

struct Update {
    model: String,
    id: Vec<u8>,
    previous_revision: Vec<u8>,
    next_revision: [u8; 8],
    data: Vec<u8>,
}

/// Evaluate all deliberate transforms before any schema/record UPDATE. The
/// transaction is still necessary to guard the atomic mutation and metadata.
fn prepare_updates(
    tx: &Transaction<'_>,
    plan: &ModelEvolution,
    budget: SnapshotBudget,
    deadline: Deadline,
    original_bytes: u64,
) -> Result<(Vec<Update>, u64), StoreError> {
    let mut statement = tx
        .prepare("SELECT model,id,revision,length(data),data FROM records ORDER BY model,id")
        .map_err(failed)?;
    let mut rows = statement.query([]).map_err(failed)?;
    let mut updates = Vec::new();
    let mut projected = original_bytes;
    while let Some(row) = rows.next().map_err(failed)? {
        deadline.check()?;
        let model: String = row.get(0).map_err(failed)?;
        let Some(transform) = plan.transformations.get(&model) else {
            continue;
        };
        let id: Vec<u8> = row.get(1).map_err(failed)?;
        Uuid::from_slice(&id).map_err(failed)?;
        let previous_revision: Vec<u8> = row.get(2).map_err(failed)?;
        let prior: [u8; 8] = previous_revision
            .as_slice()
            .try_into()
            .map_err(|_| denied("invalid migration record revision"))?;
        let next = u64::from_be_bytes(prior)
            .checked_add(1)
            .ok_or_else(|| denied("model evolution revision overflow"))?;
        let old_len: i64 = row.get(3).map_err(failed)?;
        let old_len = u64::try_from(old_len).map_err(failed)?;
        if old_len > MAX_RECORD_BYTES as u64 {
            return Err(denied("source record exceeds Storage payload bound"));
        }
        let old_data: Vec<u8> = row.get(4).map_err(failed)?;
        let old_value: Value = serde_json::from_slice(&old_data).map_err(failed)?;
        let new_value = transform(old_value)?;
        plan.target
            .get(&model)
            .ok_or_else(|| denied("target model missing from evolution plan"))?
            .validate_record(&new_value)
            .map_err(failed)?;
        let data = serde_json::to_vec(&new_value).map_err(failed)?;
        if data.len() > MAX_RECORD_BYTES {
            return Err(denied("transformed record exceeds Storage payload bound"));
        }
        projected = projected
            .checked_sub(old_len)
            .and_then(|v| v.checked_add(data.len() as u64))
            .ok_or_else(|| denied("evolution JSON byte budget overflow"))?;
        if projected > budget.max_json_bytes {
            return Err(denied("evolution JSON byte budget exceeded"));
        }
        if updates.len() as u64 >= budget.max_records {
            return Err(denied("evolution changed-record count exceeds budget"));
        }
        updates.push(Update {
            model,
            id,
            previous_revision,
            next_revision: next.to_be_bytes(),
            data,
        });
        deadline.check()?;
    }
    deadline.check()?;
    Ok((updates, projected))
}

/// Compare the final state with the retained source. Unmodified models must
/// remain byte-for-byte identical; changed rows keep UUIDs and bump revisions.
fn verify_revisions_and_unchanged(
    source: &Connection,
    target: &Connection,
    plan: &ModelEvolution,
    deadline: Deadline,
) -> Result<(), StoreError> {
    const QUERY: &str = "SELECT model,id,revision,data FROM records ORDER BY model,id";
    let mut old_statement = source.prepare(QUERY).map_err(failed)?;
    let mut new_statement = target.prepare(QUERY).map_err(failed)?;
    let mut old = old_statement.query([]).map_err(failed)?;
    let mut new = new_statement.query([]).map_err(failed)?;
    loop {
        deadline.check()?;
        match (old.next().map_err(failed)?, new.next().map_err(failed)?) {
            (None, None) => return Ok(()),
            (Some(a), Some(b)) => {
                let old_model: String = a.get(0).map_err(failed)?;
                let model: String = b.get(0).map_err(failed)?;
                let old_id: Vec<u8> = a.get(1).map_err(failed)?;
                let id: Vec<u8> = b.get(1).map_err(failed)?;
                let old_revision: Vec<u8> = a.get(2).map_err(failed)?;
                let revision: Vec<u8> = b.get(2).map_err(failed)?;
                let old_data: Vec<u8> = a.get(3).map_err(failed)?;
                let data: Vec<u8> = b.get(3).map_err(failed)?;
                if old_model != model || old_id != id {
                    return Err(denied("evolution changed a record identity"));
                }
                if plan.transformations.contains_key(&model) {
                    let prior: [u8; 8] = old_revision
                        .as_slice()
                        .try_into()
                        .map_err(|_| denied("source revision is corrupt"))?;
                    let expected = u64::from_be_bytes(prior)
                        .checked_add(1)
                        .ok_or_else(|| denied("evolution revision overflow"))?;
                    if revision != expected.to_be_bytes() {
                        return Err(denied("evolution did not advance revision exactly once"));
                    }
                } else if revision != old_revision || data != old_data {
                    return Err(denied("evolution modified an unchanged model"));
                }
            }
            _ => return Err(denied("evolution altered the record set")),
        }
    }
}

/// Detect database-side triggers or unexpected SQL effects that replace a
/// typed transform's exact bytes with another schema-valid value.
fn verify_updated_bytes(
    tx: &Transaction<'_>,
    updates: &[Update],
    deadline: Deadline,
) -> Result<(), StoreError> {
    let mut statement = tx
        .prepare("SELECT revision,data FROM records WHERE model=?1 AND id=?2")
        .map_err(failed)?;
    for change in updates {
        deadline.check()?;
        let actual: Option<(Vec<u8>, Vec<u8>)> = statement
            .query_row(params![&change.model, &change.id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .optional()
            .map_err(failed)?;
        let (revision, data) = actual.ok_or_else(|| denied("evolution lost an updated record"))?;
        if revision.as_slice() != change.next_revision || data != change.data {
            return Err(denied(
                "evolution result differs from typed transform output",
            ));
        }
    }
    Ok(())
}

fn check_logical_size(conn: &Connection, budget: SnapshotBudget) -> Result<(), StoreError> {
    let pages: i64 = conn
        .pragma_query_value(None, "page_count", |row| row.get(0))
        .map_err(failed)?;
    let page_bytes: i64 = conn
        .pragma_query_value(None, "page_size", |row| row.get(0))
        .map_err(failed)?;
    let bytes = u64::try_from(pages)
        .ok()
        .and_then(|pages| {
            u64::try_from(page_bytes)
                .ok()
                .and_then(|size| pages.checked_mul(size))
        })
        .ok_or_else(|| denied("SQLite logical size overflow"))?;
    if bytes > budget.max_snapshot_bytes {
        return Err(denied("evolution logical database size exceeds budget"));
    }
    Ok(())
}

impl SqliteMaintenance {
    /// Consume the owned maintenance session. Retain a verified independent
    /// pre-image before opening the IMMEDIATE transaction. Any pre-commit error
    /// drops the transaction and rolls back writes; the backup stays retained.
    ///
    /// This is an internal maintenance capability, not an operator-approved
    /// preview/execute workflow. Legacy providers must be stopped externally.
    pub fn execute_evolution(
        self,
        plan: &ModelEvolution,
        backup_destination: impl AsRef<Path>,
        limits: EvolutionLimits,
        _authorization: EvolutionAuthorization,
    ) -> Result<EvolutionReceipt, StoreError> {
        plan.validate_for(&self.models)?;
        let deadline = Deadline::start_with_limit(limits.snapshot, limits.max_elapsed)?;
        self.owner.verify()?;
        check_sidecars(self.owner.database_path(), limits.snapshot)?;
        require_headroom(
            self.owner
                .database_path()
                .parent()
                .ok_or_else(|| denied("missing migration database parent"))?,
            limits.snapshot.max_snapshot_bytes,
        )?;
        deadline.check()?;

        // Backup is authoritative recovery data, never a temporary scratch
        // file. It is independently verified before any mutation is attempted.
        let retained_backup =
            self.backup_to_with_deadline(backup_destination, limits.snapshot, deadline)?;
        // Keep the verified archive unavailable to other cooperating providers
        // while it is the authority for source drift and revision comparison.
        let mut snapshot_owner = OwnerGuard::acquire(&retained_backup.path)?;
        snapshot_owner.opened()?;
        deadline.check()?;
        let mut source =
            Connection::open_with_flags(&retained_backup.path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(failed)?;
        deadline.install(&mut source)?;
        let archived = audit(&source, &plan.source, limits.snapshot, deadline)?;
        if archived.records != retained_backup.records
            || archived.json_bytes != retained_backup.json_bytes
        {
            return Err(denied("retained pre-migration archive failed revalidation"));
        }

        self.owner.verify()?;
        let mut target = Connection::open_with_flags(
            self.owner.database_path(),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(failed)?;
        deadline.install(&mut target)?;
        let mode: String = target
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(failed)?;
        if !mode.eq_ignore_ascii_case("wal") {
            return Err(denied("migration requires a WAL-mode SQLite database"));
        }
        target
            .pragma_update(None, "synchronous", "FULL")
            .map_err(failed)?;
        let tx = target
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failed)?;

        // Audit and exact compare *inside* the write transaction so external
        // direct writers cannot drift between validation and mutation.
        let old = audit(&tx, &plan.source, limits.snapshot, deadline)?;
        if old != archived {
            return Err(denied("database changed since retained migration snapshot"));
        }
        compare_records(&source, &tx, deadline)?;
        let (updates, projected) =
            prepare_updates(&tx, plan, limits.snapshot, deadline, old.json_bytes)?;
        let affected = updates.len() as u64;
        for change in &updates {
            deadline.check()?;
            let changed = tx
                .execute(
                    "UPDATE records SET revision=?1,data=?2
                 WHERE model=?3 AND id=?4 AND revision=?5",
                    params![
                        &change.next_revision[..],
                        &change.data,
                        &change.model,
                        &change.id,
                        &change.previous_revision,
                    ],
                )
                .map_err(failed)?;
            if changed != 1 {
                return Err(denied("stale record changed during offline migration"));
            }
        }
        for name in plan.transformations.keys() {
            deadline.check()?;
            let target_bytes = serde_json::to_vec(&plan.target[name]).map_err(failed)?;
            let source_bytes = serde_json::to_vec(&plan.source[name]).map_err(failed)?;
            let changed = tx
                .execute(
                    "UPDATE models SET definition=?1 WHERE name=?2 AND definition=?3",
                    params![target_bytes, name, source_bytes],
                )
                .map_err(failed)?;
            if changed != 1 {
                return Err(denied("source metadata changed during offline migration"));
            }
        }
        let after = audit(&tx, &plan.target, limits.snapshot, deadline)?;
        if after.records != old.records || after.json_bytes != projected {
            return Err(denied("evolved record audit differs from projected result"));
        }
        verify_revisions_and_unchanged(&source, &tx, plan, deadline)?;
        verify_updated_bytes(&tx, &updates, deadline)?;
        // Deliberate invariants inspect the complete proposed target through
        // validated reads without receiving a SQL execution or mutation API.
        let proposed = EvolutionRecords::new(&tx, &plan.target, deadline);
        for validate in &plan.invariants {
            deadline.check()?;
            validate(&proposed)?;
            deadline.check()?;
        }
        check_logical_size(&tx, limits.snapshot)?;
        check_sidecars(self.owner.database_path(), limits.snapshot)?;
        self.owner.verify()?;
        snapshot_owner.verify()?;
        deadline.check()?;

        // SQLite commits the record and metadata changes together. Errors
        // before this statement discard the IMMEDIATE transaction.
        tx.commit().map_err(failed)?;
        let confirmed = audit(&target, &plan.target, limits.snapshot, deadline)?;
        if confirmed != after {
            return Err(denied(
                "committed evolution failed post-commit verification",
            ));
        }
        self.owner.verify()?;
        snapshot_owner.verify()?;
        deadline.check()?;
        Ok(EvolutionReceipt {
            retained_backup,
            records: confirmed.records,
            affected_records: affected,
            json_bytes: confirmed.json_bytes,
        })
    }
}
