//! Explicit offline SQLite maintenance; no migration is run on provider open.
//! Snapshot creation is independently validated and does not overwrite files.
//! Restore/migration execution are separate Stage 6 delivery gates.
use crate::{
    database::{FORMAT_VERSION, failed, verify_models},
    owner::OwnerGuard,
    records::MAX_RECORD_BYTES,
};
use riesel_schema::ModelDefinition;
use riesel_storage::StoreError;
use rusqlite::{Connection, OpenFlags, params};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File},
    io::ErrorKind,
    path::{Path, PathBuf},
};
use uuid::Uuid;

fn denied(message: &str) -> StoreError {
    StoreError::Failed(format!("SQLite offline backup refused: {message}"))
}

/// Explicit resource ceilings, supplied by the caller rather than an
/// implicit application-wide or unlimited backup default.
#[derive(Debug, Clone, Copy)]
pub struct SnapshotBudget {
    pub max_records: u64,
    pub max_json_bytes: u64,
    pub max_snapshot_bytes: u64,
}

/// A verified, independently materialized SQLite backup artifact.
#[derive(Debug)]
pub struct VerifiedSnapshot {
    pub path: PathBuf,
    pub records: u64,
    pub json_bytes: u64,
    pub snapshot_bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct Audit {
    records: u64,
    json_bytes: u64,
}

fn audit(
    conn: &Connection,
    models: &BTreeMap<String, ModelDefinition>,
    budget: SnapshotBudget,
) -> Result<Audit, StoreError> {
    if budget.max_records == 0 || budget.max_json_bytes == 0 || budget.max_snapshot_bytes == 0 {
        return Err(denied("all snapshot budgets must be positive"));
    }
    let version: i64 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(failed)?;
    if version != FORMAT_VERSION {
        return Err(denied("unsupported SQLite physical format"));
    }
    let integrity: String = conn
        .pragma_query_value(None, "integrity_check", |row| row.get(0))
        .map_err(failed)?;
    if integrity != "ok" {
        return Err(denied("SQLite integrity check did not pass"));
    }

    let expected = models
        .iter()
        .map(|(name, model)| {
            serde_json::to_vec(model)
                .map(|bytes| (name.clone(), bytes))
                .map_err(failed)
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    verify_models(conn, &expected)?;

    let mut stmt = conn
        .prepare("SELECT model,id,revision,data FROM records ORDER BY model,id")
        .map_err(failed)?;
    let mut query = stmt.query([]).map_err(failed)?;
    let mut records = 0_u64;
    let mut json_bytes = 0_u64;
    while let Some(row) = query.next().map_err(failed)? {
        let model: String = row.get(0).map_err(failed)?;
        let id: Vec<u8> = row.get(1).map_err(failed)?;
        let revision: Vec<u8> = row.get(2).map_err(failed)?;
        let bytes: Vec<u8> = row.get(3).map_err(failed)?;
        if Uuid::from_slice(&id).is_err() || revision.len() != 8 {
            return Err(denied("corrupt UUID or revision"));
        }
        let definition = models
            .get(&model)
            .ok_or_else(|| denied("record belongs to an unknown model"))?;
        let data: Value = serde_json::from_slice(&bytes).map_err(failed)?;
        definition.validate_record(&data).map_err(failed)?;
        if serde_json::to_vec(&data).map_err(failed)?.len() > MAX_RECORD_BYTES {
            return Err(denied("record exceeds the Storage payload bound"));
        }
        records = records
            .checked_add(1)
            .ok_or_else(|| denied("record count overflow"))?;
        json_bytes = json_bytes
            .checked_add(u64::try_from(bytes.len()).map_err(failed)?)
            .ok_or_else(|| denied("record byte count overflow"))?;
        if records > budget.max_records || json_bytes > budget.max_json_bytes {
            return Err(denied("snapshot record or JSON byte budget exceeded"));
        }
    }
    Ok(Audit {
        records,
        json_bytes,
    })
}

/// Read both databases in stable order without materializing all rows.
fn compare_records(left: &Connection, right: &Connection) -> Result<(), StoreError> {
    const QUERY: &str = "SELECT model,id,revision,data FROM records ORDER BY model,id";
    let mut lstmt = left.prepare(QUERY).map_err(failed)?;
    let mut rstmt = right.prepare(QUERY).map_err(failed)?;
    let mut lrows = lstmt.query([]).map_err(failed)?;
    let mut rrows = rstmt.query([]).map_err(failed)?;
    loop {
        match (lrows.next().map_err(failed)?, rrows.next().map_err(failed)?) {
            (None, None) => return Ok(()),
            (Some(l), Some(r)) => {
                let old: (String, Vec<u8>, Vec<u8>, Vec<u8>) = (
                    l.get(0).map_err(failed)?,
                    l.get(1).map_err(failed)?,
                    l.get(2).map_err(failed)?,
                    l.get(3).map_err(failed)?,
                );
                let copied: (String, Vec<u8>, Vec<u8>, Vec<u8>) = (
                    r.get(0).map_err(failed)?,
                    r.get(1).map_err(failed)?,
                    r.get(2).map_err(failed)?,
                    r.get(3).map_err(failed)?,
                );
                if old != copied {
                    return Err(denied("backup contents differ from the source"));
                }
            }
            _ => return Err(denied("backup record count differs from source")),
        }
    }
}

/// A pending snapshot is never reported as a successful backup. On failure the
/// unique pending pathname is removed; destination is never overwritten.
struct PendingSnapshot(PathBuf);

impl Drop for PendingSnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn output_path(destination: &Path, source: &Path) -> Result<PathBuf, StoreError> {
    let name = destination
        .file_name()
        .ok_or_else(|| denied("backup needs a named destination file"))?;
    let parent = destination
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent).map_err(failed)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if fs::metadata(&parent).map_err(failed)?.permissions().mode() & 0o022 != 0 {
            return Err(denied("backup directory must not be group/other-writable"));
        }
    }
    let path = parent.join(name);
    if path == source {
        return Err(denied("backup destination is the live database"));
    }
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Ok(_) => return Err(denied("backup destination already exists")),
        Err(error) => return Err(failed(error)),
    }
    Ok(path)
}

/// An explicitly acquired *offline* owner. No migrations can be initiated
/// through this first backup-only interface. Acquiring may create a persistent
/// owner lockfile, but never creates/initializes an absent source database.
pub struct SqliteMaintenance {
    owner: OwnerGuard,
    models: BTreeMap<String, ModelDefinition>,
}

impl SqliteMaintenance {
    /// Take exclusive maintenance ownership and validate existing v1 records.
    /// A live linked provider blocks this action.
    pub fn open_existing(
        path: impl AsRef<Path>,
        models: BTreeMap<String, ModelDefinition>,
        budget: SnapshotBudget,
    ) -> Result<Self, StoreError> {
        let mut owner = OwnerGuard::acquire(path.as_ref())?;
        if fs::metadata(owner.database_path()).map_err(failed)?.len() > budget.max_snapshot_bytes {
            return Err(denied("source database exceeds the declared snapshot byte ceiling"));
        }
        let conn =
            Connection::open_with_flags(owner.database_path(), OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(failed)?;
        audit(&conn, &models, budget)?;
        drop(conn);
        owner.opened()?;
        Ok(Self { owner, models })
    }

    /// Create and verify an independent SQLite snapshot while holding the
    /// same exclusive lifetime lock as the regular provider. The destination
    /// must be absent and remain under the trusted operator's control.
    pub fn backup_to(
        &self,
        destination: impl AsRef<Path>,
        budget: SnapshotBudget,
    ) -> Result<VerifiedSnapshot, StoreError> {
        self.owner.verify()?;
        let output = output_path(destination.as_ref(), self.owner.database_path())?;
        let source = Connection::open_with_flags(
            self.owner.database_path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(failed)?;
        let before = audit(&source, &self.models, budget)?;
        let mut pending_name: OsString = output.as_os_str().to_owned();
        pending_name.push(format!(".riesel-pending-{}", Uuid::new_v4()));
        let pending = PathBuf::from(pending_name);
        if pending.exists() {
            return Err(denied("generated pending snapshot path already exists"));
        }
        let cleanup = PendingSnapshot(pending.clone());
        let pending_sql_path = pending
            .to_str()
            .ok_or_else(|| denied("backup output path is not valid UTF-8"))?;
        source
            .execute("VACUUM INTO ?1", params![pending_sql_path])
            .map_err(failed)?;
        let copied = Connection::open_with_flags(&pending, OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(failed)?;
        let after = audit(&copied, &self.models, budget)?;
        if before != after {
            return Err(denied("snapshot audit differs from source"));
        }
        compare_records(&source, &copied)?;
        let file_bytes = fs::metadata(&pending).map_err(failed)?.len();
        if file_bytes > budget.max_snapshot_bytes {
            return Err(denied("snapshot file byte budget exceeded"));
        }
        drop(copied);
        drop(source);
        self.owner.verify()?;
        File::open(&pending)
            .map_err(failed)?
            .sync_all()
            .map_err(failed)?;
        // Unlike rename, hard_link fails atomically if destination exists.
        // Both names are in the same directory/filesystem.
        fs::hard_link(&pending, &output).map_err(failed)?;
        fs::remove_file(&pending).map_err(failed)?;
        #[cfg(unix)]
        {
            let parent = output
                .parent()
                .ok_or_else(|| denied("missing backup parent"))?;
            File::open(parent)
                .map_err(failed)?
                .sync_all()
                .map_err(failed)?;
        }
        drop(cleanup);
        Ok(VerifiedSnapshot {
            path: output,
            records: after.records,
            json_bytes: after.json_bytes,
            snapshot_bytes: file_bytes,
        })
    }
}
