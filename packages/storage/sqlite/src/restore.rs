//! Deliberate offline restore using SQLite's transaction-protected backup API.
//! Only valid format-v1 destinations are supported in the initial slice.
//! The caller must explicitly acknowledge loss of writes made after the source
//! snapshot. Corrupt destination repair and power-loss durability are excluded.
use crate::{
    capacity::{check_sidecars, require_headroom},
    database::failed,
    deadline::Deadline,
    maintenance::{
        SnapshotBudget, SqliteMaintenance, VerifiedSnapshot, audit, compare_records, denied,
    },
    owner::OwnerGuard,
};
use riesel_schema::ModelDefinition;
use riesel_storage::StoreError;
use rusqlite::{Connection, OpenFlags};
use std::{collections::BTreeMap, fs, path::Path};

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
        let deadline = Deadline::start(budget)?;
        self.owner.verify()?;
        deadline.check()?;

        // The backup has its own cooperating lifetime guard; this protects
        // against a second supported provider modifying the verified snapshot.
        let mut source_owner = OwnerGuard::acquire(source_snapshot.as_ref())?;
        if source_owner.database_path() == self.owner.database_path() {
            return Err(denied(
                "restoration snapshot must not be the target database",
            ));
        }
        if fs::metadata(source_owner.database_path())
            .map_err(failed)?
            .len()
            > budget.max_snapshot_bytes
        {
            return Err(denied("restoration snapshot exceeds the byte ceiling"));
        }
        check_sidecars(source_owner.database_path(), budget)?;
        check_sidecars(self.owner.database_path(), budget)?;
        source_owner.opened()?;
        let mut source = Connection::open_with_flags(
            source_owner.database_path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(failed)?;
        deadline.install(&mut source)?;
        let verified = audit(&source, &snapshot_models, budget, deadline)?;

        // Check the currently opened target without permitting an implicit
        // schema upgrade or attempting to overwrite a corrupted destination.
        let mut target = Connection::open_with_flags(
            self.owner.database_path(),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(failed)?;
        deadline.install(&mut target)?;
        audit(&target, &self.models, budget, deadline)?;
        let journal: String = target
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .map_err(failed)?;
        if !journal.eq_ignore_ascii_case("wal") {
            return Err(denied("restoration requires a WAL-mode destination"));
        }

        // Reserve space on the destination filesystem for the SQLite restore
        // WAL as well as separately checking the retention filesystem.
        require_headroom(
            self.owner
                .database_path()
                .parent()
                .ok_or_else(|| denied("missing restoration target parent"))?,
            budget.max_snapshot_bytes,
        )?;

        // An explicit retained copy of the *current* state precedes mutation.
        // This uses VACUUM INTO and refuses an occupied destination.
        drop(target);
        let retained_pre_restore =
            self.backup_to_with_deadline(retain_current_at, budget, deadline)?;

        self.owner.verify()?;
        source_owner.verify()?;
        check_sidecars(source_owner.database_path(), budget)?;
        check_sidecars(self.owner.database_path(), budget)?;
        deadline.check()?;
        target = Connection::open_with_flags(
            self.owner.database_path(),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(failed)?;
        deadline.install(&mut target)?;
        audit(&target, &self.models, budget, deadline)?;
        target
            .pragma_update(None, "synchronous", "FULL")
            .map_err(failed)?;

        // SQLite's backup API uses a destination write transaction rather than
        // raw main-file replacement that could diverge from a live WAL.
        let backup = rusqlite::backup::Backup::new(&source, &mut target).map_err(failed)?;
        loop {
            deadline.check()?;
            match backup.step(64).map_err(failed)? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => {}
                _ => {
                    return Err(denied(
                        "restoration copy blocked; new authorization required",
                    ));
                }
            }
        }
        drop(backup);
        deadline.check()?;

        let after = audit(&target, &snapshot_models, budget, deadline)?;
        if after != verified {
            return Err(denied(
                "restored content does not match the verified snapshot",
            ));
        }
        compare_records(&source, &target, deadline)?;
        check_sidecars(self.owner.database_path(), budget)?;
        self.owner.verify()?;
        source_owner.verify()?;

        let restored_size = fs::metadata(self.owner.database_path())
            .map_err(failed)?
            .len();
        if restored_size > budget.max_snapshot_bytes {
            return Err(denied("restored database exceeds the byte ceiling"));
        }
        deadline.check()?;
        Ok(RestoreReceipt {
            retained_pre_restore,
            restored_records: after.records,
            restored_json_bytes: after.json_bytes,
        })
    }
}
