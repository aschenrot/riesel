//! Cooperative SQLite lifetime ownership on trusted single-host local filesystems.
//! A persistent sidecar is used for `File::try_lock`. Do not remove it on
//! shutdown: a reused pathname is not a stable lock identity if it is replaced.
//! Older binaries and uncooperative SQLite connections do not honor this lock.
use crate::database::failed;
use riesel_storage::StoreError;
use std::{
    ffi::OsString,
    fs::{self, File, Metadata, OpenOptions, TryLockError},
    io::ErrorKind,
    path::{Path, PathBuf},
};

pub(crate) struct OwnerGuard {
    database_path: PathBuf,
    lock_path: PathBuf,
    lock_file: File,
    opened_database: Option<Metadata>,
}

fn invalid(message: &str) -> StoreError {
    StoreError::Failed(format!("SQLite ownership denied: {message}"))
}

fn check_file(path: &Path) -> Result<Option<Metadata>, StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(invalid("file must be regular, not a symlink or directory"));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::{MetadataExt, PermissionsExt};
                if metadata.nlink() != 1 {
                    return Err(invalid("hard-linked files cannot have unique ownership"));
                }
                if metadata.permissions().mode() & 0o022 != 0 {
                    return Err(invalid("group/other-writable files are unsupported"));
                }
            }
            Ok(Some(metadata))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(failed(error)),
    }
}

#[cfg(unix)]
fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_identity(_left: &Metadata, _right: &Metadata) -> bool {
    // Windows file-ID verification is a separate manual pre-release gate.
    // std::fs::File::try_lock still excludes cooperating holders on its object.
    true
}

impl OwnerGuard {
    /// Acquire the same canonical sidecar identity used by normal providers and
    /// eventual maintenance. This runs only during startup/maintenance.
    pub(crate) fn acquire(path: &Path) -> Result<Self, StoreError> {
        let name = path
            .file_name()
            .ok_or_else(|| invalid("a named local SQLite file is required"))?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = fs::canonicalize(parent).map_err(failed)?;
        let parent_metadata = fs::metadata(&parent).map_err(failed)?;
        if !parent_metadata.is_dir() {
            return Err(invalid("database parent is not a directory"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if parent_metadata.permissions().mode() & 0o022 != 0 {
                return Err(invalid(
                    "database directory must not be group/other-writable",
                ));
            }
        }

        let database_path = parent.join(name);
        check_file(&database_path)?;

        let mut lock_name: OsString = database_path.as_os_str().to_owned();
        lock_name.push(".riesel-owner.lock");
        let lock_path = PathBuf::from(lock_name);
        check_file(&lock_path)?;

        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock_file = options.open(&lock_path).map_err(failed)?;
        match lock_file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(invalid(
                    "database is already owned by a cooperating process",
                ));
            }
            Err(TryLockError::Error(error)) => return Err(failed(error)),
        }
        let owner = Self {
            database_path,
            lock_path,
            lock_file,
            opened_database: None,
        };
        owner.verify_lock()?;
        Ok(owner)
    }

    pub(crate) fn database_path(&self) -> &Path {
        &self.database_path
    }

    /// Capture the newly opened DB file identity before exposing the provider.
    pub(crate) fn opened(&mut self) -> Result<(), StoreError> {
        self.verify_lock()?;
        self.opened_database = Some(
            check_file(&self.database_path)?
                .ok_or_else(|| invalid("opened database path is missing"))?,
        );
        Ok(())
    }

    fn verify_lock(&self) -> Result<(), StoreError> {
        let current = check_file(&self.lock_path)?
            .ok_or_else(|| invalid("ownership lockfile was removed"))?;
        let owned = self.lock_file.metadata().map_err(failed)?;
        if !same_identity(&owned, &current) {
            return Err(invalid("ownership lockfile identity changed"));
        }
        Ok(())
    }

    /// Detect visible replacement of the lock or database identity before any
    /// provider read/write. Cannot prevent malicious same-user rename races.
    pub(crate) fn verify(&self) -> Result<(), StoreError> {
        self.verify_lock()?;
        if let Some(expected) = &self.opened_database {
            let current = check_file(&self.database_path)?
                .ok_or_else(|| invalid("database path was removed"))?;
            if !same_identity(expected, &current) {
                return Err(invalid("database file identity changed"));
            }
        }
        Ok(())
    }
}


// A child process can briefly inherit a locked file description across fork,
// even when a descriptor is close-on-exec. Merely closing this File releases a
// flock only after the *last* inherited/duplicated descriptor is closed.
// Explicitly unlock when the authoritative owner is dropped so an inherited
// observer cannot extend the cooperative ownership lease after shutdown.
// This does not change live-owner exclusivity or the tamper-resistance scope.
impl Drop for OwnerGuard {
    fn drop(&mut self) {
        // Drop has no error channel. Normal file close remains the fallback if
        // this best-effort unlock reports an unexpected OS error.
        let _ = self.lock_file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::OwnerGuard;
    use crate::SqliteStore;
    use riesel_schema::{ModelDefinition, TypeRef};
    use std::collections::BTreeMap;


    /// A child retaining a duplicated locked fd must not extend ownership
    /// past the original provider's lifetime. This is a deterministic
    /// reproduction of the otherwise timing-dependent fork/exec window.
    #[cfg(unix)]
    #[test]
    fn inherited_lock_descriptor_does_not_outlive_explicit_owner_shutdown() {
        use std::process::{Command, Stdio};

        let temporary = tempfile::tempdir().unwrap();
        let db = temporary.path().join("records.db");
        let guard = OwnerGuard::acquire(&db).unwrap();
        assert!(OwnerGuard::acquire(&db).is_err(), "live owner must exclude a second");

        // stderr is a clone of the same open file description. The shell waits
        // on a parent-held stdin pipe so the inherited descriptor remains open.
        let inherited = guard.lock_file.try_clone().unwrap();
        let mut child = Command::new("sh")
            .arg("-c")
            .arg("read _")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::from(inherited))
            .spawn()
            .unwrap();
        assert!(child.try_wait().unwrap().is_none(), "child must still hold the inherited fd");

        drop(guard);
        let replacement = OwnerGuard::acquire(&db)
            .expect("owner shutdown must release lock even while a child holds a cloned fd");
        replacement.verify().unwrap();
        drop(replacement);

        drop(child.stdin.take());
        assert!(child.wait().is_ok(), "child should terminate after stdin is closed");
    }

    #[test]
    fn maintenance_owner_cannot_enter_a_running_database() {
        let temporary = tempfile::tempdir().unwrap();
        let db = temporary.path().join("records.db");
        let models = BTreeMap::from([(
            "task".into(),
            ModelDefinition {
                fields: BTreeMap::from([("title".into(), TypeRef::String)]),
            },
        )]);
        let store = SqliteStore::open(&db, models).unwrap();
        assert!(OwnerGuard::acquire(&db).is_err());
        drop(store);
        let maintenance = OwnerGuard::acquire(&db).unwrap();
        maintenance.verify().unwrap();
        drop(maintenance);
    }
}
