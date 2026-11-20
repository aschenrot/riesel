//! Deliberate offline restore using SQLite's transaction-protected backup API.
//! Only valid format-v1 destinations are supported in the initial slice.
//! The caller must explicitly acknowledge loss of writes made after the source
//! snapshot. Corrupt destination repair and power-loss durability are excluded.
use crate::{
    maintenance::{SnapshotBudget, SqliteMaintenance, VerifiedSnapshot, audit, compare_records, denied},
    owner::OwnerGuard,
    database::failed,
};
use riesel_schema::ModelDefinition;
use riesel_storage::StoreError;
use rusqlite::{Connection, OpenFlags};
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};

/// Explicit, non-default acknowledgement of irreversible writes newer than
/// the retained source snapshot. A later CLI must still obtain operator consent.
#[derive(Debug, Clone, Copy)]
pub enum RestoreAuthorization {
    DiscardPostSnapshotWrites,
}

/// Restoration result. The pre-restore copy is retained, never implicitly
/// deleted, so an operator can inspect or deliberately reverse the action.
#[derive(Debug)]
pub struct RestoreReceipt {
    pub retained_pre_restore: VerifiedSnapshot,
    pub restored_records: u64,
    pub restored_json_bytes: u64,
}

impl SqliteMaintenance {
    /// Restore an independently validated historical snapshot into an existing
    /// and healthy format-v1 SQLite file. Consumes the maintenance owner so its
    /// old cached model registration cannot be reused following restore.
    ///
    /// This operation first retains an independent verified pre-restore snapshot
    /// at a distinct, unoccupied destination. It never silently restores during
    /// normal application startup and cannot claim an older process was stopped.
    ///
    /// On errors *before* SQLite's restore call, the original target is unchanged.
    /// If an error occurs after that call, the target's commit state may be
    /// uncertain; the retained pre-restore backup must be inspected explicitly.
    pub fn restore_from_backup(
        self,
        source_snapshot: impl AsRef<Path>,
        snapshot_models: BTreeMap<String, ModelDefinition>,
        retain_current_at: impl AsRef<Path>,
        budget: SnapshotBudget,
        _authorization: RestoreAuthorization,
    ) -> Result<RestoreReceipt, StoreError> {
        self.owner.verify()?;

        // The backup has its own cooperating lifetime guard; this protects
        // against a second supported provider modifying the verified snapshot.
        let mut source_owner = OwnerGuard::acquire(source_snapshot.as_ref())?;
        if source_owner.database_path() == self.owner.database_path() {
            return Err(denied("restoration snapshot must not be the target database"));
        }
        if fs::metadata(source_owner.database_path()).map_err(failed)?.len()
            > budget.max_snapshot_bytes
        {
            return Err(denied("restoration snapshot exceeds the byte ceiling"));
        }
        source_owner.opened()?;
        let source = Connection::open_with_flags(
            source_owner.database_path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(failed)?;
        let verified = audit(&source, &snapshot_models, budget)?;

        // Check the currently opened target without permitting an implicit
        // schema upgrade or attempting to overwrite a corrupted destination.
        let mut target = Connection::open_with_flags(
            self.owner.database_path(),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(failed)?;
        audit(&target, &self.models, budget)?;
        let journal: String = target
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(failed)?;
        if !journal.eq_ignore_ascii_case("wal") {
            return Err(denied("restoration requires a WAL-mode destination"));
        }

        // An explicit retained copy of the *current* state precedes mutation.
        // This uses VACUUM INTO and refuses an occupied destination.
        drop(target);
        let retained_pre_restore = self.backup_to(retain_current_at, budget)?;

        self.owner.verify()?;
        source_owner.verify()?;
        target = Connection::open_with_flags(
            self.owner.database_path(),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(failed)?;
        audit(&target, &self.models, budget)?;
        target
            .pragma_update(None, "synchronous", "FULL")
            .map_err(failed)?;

        // SQLite's backup API uses a destination write transaction rather than
        // raw main-file replacement that could diverge from a live WAL.
        target
            .restore(
                rusqlite::MAIN_DB,
                source_owner.database_path(),
                rusqlite::backup::NO_PROGRESS,
            )
            .map_err(failed)?;

        let after = audit(&target, &snapshot_models, budget)?;
        if after != verified {
            return Err(denied("restored content does not match the verified snapshot"));
        }
        compare_records(&source, &target)?;
        self.owner.verify()?;
        source_owner.verify()?;

        let restored_size = fs::metadata(self.owner.database_path()).map_err(failed)?.len();
        if restored_size > budget.max_snapshot_bytes {
            return Err(denied("restored database exceeds the byte ceiling"));
        }
        Ok(RestoreReceipt {
            retained_pre_restore,
            restored_records: after.records,
            restored_json_bytes: after.json_bytes,
        })
    }
}
