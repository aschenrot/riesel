//! Explicit application-owned V2 SQLite snapshot and fresh-location restore.
//! This is a verified local **data** copy, not a signed artifact, secure
//! external author, active-session transfer or automatically applied upgrade.
//! Reuses Storage's existing no-clobber/safe-directory destination policy.

use super::{FORMAT, SqlitePlanRevisionRepository, storage};
use crate::maintenance::output_path;
use riesel_app::PlanRevisionError;
use rusqlite::{Connection, OpenFlags, params, types::Value};
use std::{
    ffi::OsString,
    fs::{self, File},
    path::{Path, PathBuf},
};
use uuid::Uuid;

/// Explicit workload ceiling; large application workspaces must authorize a
/// larger budget rather than relying on unlimited snapshot materialization.
#[derive(Debug, Clone, Copy)]
pub struct V2SnapshotBudget {
    pub max_snapshot_bytes: u64,
    pub max_rows: u64,
}

impl Default for V2SnapshotBudget {
    fn default() -> Self {
        Self {
            max_snapshot_bytes: 256 * 1024 * 1024,
            max_rows: 100_000,
        }
    }
}

/// Integrity-checked, SQLite-format-checked local V2 snapshot metadata.
/// The registration stamp is a **copyable correlation string**, not a
/// signature or independently verified OS/application identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedV2Snapshot {
    pub path: PathBuf,
    pub snapshot_bytes: u64,
    pub plan_count: u64,
    pub semantic_count: u64,
    pub layout_count: u64,
    pub registration_stamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotAudit {
    plan_count: u64,
    semantic_count: u64,
    layout_count: u64,
    registration_stamp: String,
}

/// An unfinished snapshot cannot accidentally become a successful output.
struct Pending(PathBuf);
impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// A hard-linked target is removed if final directory synchronization fails.
struct Publication(Option<PathBuf>);
impl Drop for Publication {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_file(path);
        }
    }
}

fn rows(connection: &Connection, query: &str) -> Result<u64, PlanRevisionError> {
    let count: i64 = connection.query_row(query, [], |row| row.get(0)).map_err(storage)?;
    u64::try_from(count).map_err(storage)
}

fn audit(
    connection: &Connection,
    expected_stamp: &str,
    budget: V2SnapshotBudget,
) -> Result<SnapshotAudit, PlanRevisionError> {
    if budget.max_snapshot_bytes == 0 || budget.max_rows == 0 {
        return Err(PlanRevisionError::Invalid(
            "snapshot budget must be positive".into(),
        ));
    }
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage)?;
    if version != FORMAT {
        return Err(PlanRevisionError::UnsupportedFormat(version));
    }
    let integrity: String = connection
        .pragma_query_value(None, "integrity_check", |row| row.get(0))
        .map_err(storage)?;
    if integrity != "ok" {
        return Err(PlanRevisionError::Storage("snapshot integrity check failed".into()));
    }
    if rows(connection, "SELECT count(*) FROM pragma_foreign_key_check")? != 0 {
        return Err(PlanRevisionError::Storage("snapshot foreign-key check failed".into()));
    }
    let tables = rows(
        connection,
        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
    )?;
    if tables != 5 {
        return Err(PlanRevisionError::Storage("unknown or incomplete V2 store schema".into()));
    }
    let mismatched: i64 = connection
        .query_row(
            "SELECT count(*) FROM plan_heads WHERE registration_stamp <> ?1",
            params![expected_stamp],
            |row| row.get(0),
        )
        .map_err(storage)?;
    if mismatched != 0 {
        return Err(PlanRevisionError::Conflict);
    }
    if rows(
        connection,
        "SELECT count(*) FROM plan_heads AS h
         LEFT JOIN semantic_revisions AS r ON r.plan_id=h.plan_id AND r.revision=h.semantic_revision
         WHERE r.plan_id IS NULL",
    )? != 0
    {
        return Err(PlanRevisionError::Storage(
            "snapshot head references a missing semantic revision".into(),
        ));
    }
    let plan_count = rows(connection, "SELECT count(*) FROM plan_heads")?;
    let semantic_count = rows(connection, "SELECT count(*) FROM semantic_revisions")?;
    let layout_count = rows(connection, "SELECT count(*) FROM layout_revisions")?;
    let layout_heads = rows(connection, "SELECT count(*) FROM layout_heads")?;
    let handoffs = rows(connection, "SELECT count(*) FROM source_handoffs")?;
    let total = plan_count
        .checked_add(semantic_count)
        .and_then(|value| value.checked_add(layout_count))
        .and_then(|value| value.checked_add(layout_heads))
        .and_then(|value| value.checked_add(handoffs))
        .ok_or_else(|| PlanRevisionError::Invalid("snapshot row count overflow".into()))?;
    if total > budget.max_rows {
        return Err(PlanRevisionError::Invalid("snapshot row budget exceeded".into()));
    }
    Ok(SnapshotAudit {
        plan_count,
        semantic_count,
        layout_count,
        registration_stamp: expected_stamp.to_owned(),
    })
}

fn open_snapshot(path: &Path) -> Result<Connection, PlanRevisionError> {
    let metadata = fs::symlink_metadata(path).map_err(storage)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(PlanRevisionError::Invalid(
            "snapshot must be a regular, non-symlink file".into(),
        ));
    }
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(storage)
}

fn verify(
    path: &Path,
    expected_stamp: &str,
    budget: V2SnapshotBudget,
) -> Result<VerifiedV2Snapshot, PlanRevisionError> {
    let bytes = fs::metadata(path).map_err(storage)?.len();
    if bytes > budget.max_snapshot_bytes {
        return Err(PlanRevisionError::Invalid("snapshot byte budget exceeded".into()));
    }
    let connection = open_snapshot(path)?;
    let audited = audit(&connection, expected_stamp, budget)?;
    Ok(VerifiedV2Snapshot {
        path: path.to_path_buf(),
        snapshot_bytes: bytes,
        plan_count: audited.plan_count,
        semantic_count: audited.semantic_count,
        layout_count: audited.layout_count,
        registration_stamp: audited.registration_stamp,
    })
}

fn compare_table_rows(
    source: &Connection,
    copied: &Connection,
    table: &str,
    columns: &str,
) -> Result<(), PlanRevisionError> {
    // Both column lists and table names are fixed internal constants only.
    let query = format!("SELECT {columns} FROM {table} ORDER BY 1,2,3");
    let mut source_stmt = source.prepare(&query).map_err(storage)?;
    let mut copied_stmt = copied.prepare(&query).map_err(storage)?;
    let column_count = source_stmt.column_count();
    let mut original = source_stmt.query([]).map_err(storage)?;
    let mut snapshot = copied_stmt.query([]).map_err(storage)?;
    loop {
        match (original.next().map_err(storage)?, snapshot.next().map_err(storage)?) {
            (None, None) => return Ok(()),
            (Some(left), Some(right)) => {
                for column in 0..column_count {
                    let original_value: Value = left.get(column).map_err(storage)?;
                    let copied_value: Value = right.get(column).map_err(storage)?;
                    if original_value != copied_value {
                        return Err(PlanRevisionError::Conflict);
                    }
                }
            }
            _ => return Err(PlanRevisionError::Conflict),
        }
    }
}

fn compare_exact_content(
    source: &Connection,
    snapshot: &Connection,
) -> Result<(), PlanRevisionError> {
    for (table, columns) in [
        ("plan_heads", "plan_id, semantic_revision, source_kind, source_generation, registration_stamp"),
        ("semantic_revisions", "plan_id, revision, inspection, node_ids, source_document"),
        ("layout_heads", "plan_id, layout_id, revision"),
        ("layout_revisions", "plan_id, layout_id, revision, semantic_anchor, layout_json"),
        ("source_handoffs", "plan_id, generation, source_kind, semantic_anchor, source_document"),
    ] {
        compare_table_rows(source, snapshot, table, columns)?;
    }
    Ok(())
}

fn pending_path(destination: &Path) -> PathBuf {
    let mut filename: OsString = destination.as_os_str().to_owned();
    filename.push(format!(".riesel-v2-pending-{}", Uuid::new_v4()));
    PathBuf::from(filename)
}

fn publish(
    pending: &Path,
    destination: &Path,
) -> Result<(), PlanRevisionError> {
    File::open(pending).map_err(storage)?.sync_all().map_err(storage)?;
    // Hard-link publication fails atomically if the destination already
    // exists. This never replaces an existing app-owned revision database.
    fs::hard_link(pending, destination).map_err(storage)?;
    let mut publication = Publication(Some(destination.to_path_buf()));
    fs::remove_file(pending).map_err(storage)?;
    #[cfg(unix)]
    File::open(
        destination
            .parent()
            .ok_or_else(|| PlanRevisionError::Invalid("missing snapshot parent".into()))?,
    )
    .map_err(storage)?
    .sync_all()
    .map_err(storage)?;
    publication.0 = None;
    Ok(())
}

impl SqlitePlanRevisionRepository {
    /// Capture a consistent, verified copy in a distinct trusted directory.
    /// Existing output is never overwritten. No active runner plan changes.
    pub fn backup_to(
        &self,
        destination: impl AsRef<Path>,
        budget: V2SnapshotBudget,
    ) -> Result<VerifiedV2Snapshot, PlanRevisionError> {
        let destination =
            output_path(destination.as_ref(), &self.path).map_err(storage)?;
        let source = self.connection()?;
        let before_version: i64 = source
            .pragma_query_value(None, "data_version", |row| row.get(0))
            .map_err(storage)?;
        let before = audit(&source, &self.trusted_registration_stamp, budget)?;
        let pending = pending_path(&destination);
        let cleanup = Pending(pending.clone());
        let path_str = pending
            .to_str()
            .ok_or_else(|| PlanRevisionError::Invalid("snapshot destination is not UTF-8".into()))?;
        source.execute("VACUUM INTO ?1", params![path_str]).map_err(storage)?;
        let after_version: i64 = source
            .pragma_query_value(None, "data_version", |row| row.get(0))
            .map_err(storage)?;
        if before_version != after_version {
            return Err(PlanRevisionError::Conflict);
        }
        let verified = verify(&pending, &self.trusted_registration_stamp, budget)?;
        let copied = open_snapshot(&pending)?;
        let after = audit(&copied, &self.trusted_registration_stamp, budget)?;
        if before != after {
            return Err(PlanRevisionError::Conflict);
        }
        compare_exact_content(&source, &copied)?;
        drop(copied);
        drop(source);
        publish(&pending, &destination)?;
        drop(cleanup);
        Ok(VerifiedV2Snapshot { path: destination, ..verified })
    }

    /// Read-only snapshot inspection: checks SQLite integrity, V2 schema,
    /// source-registration correlation and bounded stored history.
    pub fn verify_backup(
        path: impl AsRef<Path>,
        expected_registration_stamp: &str,
        budget: V2SnapshotBudget,
    ) -> Result<VerifiedV2Snapshot, PlanRevisionError> {
        verify(path.as_ref(), expected_registration_stamp, budget)
    }

    /// Restore only into a *new* destination; never overwrite a running app
    /// database or grant authority to another process. Caller must ensure the
    /// original application is stopped before adopting this history as live.
    pub fn restore_backup_to_new_file(
        backup: impl AsRef<Path>,
        destination: impl AsRef<Path>,
        expected_registration_stamp: &str,
        budget: V2SnapshotBudget,
    ) -> Result<VerifiedV2Snapshot, PlanRevisionError> {
        let backup = backup.as_ref();
        let verified = verify(backup, expected_registration_stamp, budget)?;
        let destination = output_path(destination.as_ref(), backup).map_err(storage)?;
        let pending = pending_path(&destination);
        let cleanup = Pending(pending.clone());
        fs::copy(backup, &pending).map_err(storage)?;
        let copied = verify(&pending, expected_registration_stamp, budget)?;
        if copied.plan_count != verified.plan_count
            || copied.semantic_count != verified.semantic_count
            || copied.layout_count != verified.layout_count
            || copied.snapshot_bytes != verified.snapshot_bytes
        {
            return Err(PlanRevisionError::Conflict);
        }
        let original = open_snapshot(backup)?;
        let restored = open_snapshot(&pending)?;
        compare_exact_content(&original, &restored)?;
        drop(original);
        drop(restored);
        publish(&pending, &destination)?;
        drop(cleanup);
        Ok(VerifiedV2Snapshot { path: destination, ..copied })
    }
}
