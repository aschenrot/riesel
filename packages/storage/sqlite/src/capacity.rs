//! Conservative filesystem-space preflight for explicit SQLite maintenance.
//! This is not a reservation: another writer, filesystem metadata, SQLite
//! temporary files, and sudden I/O failure can exhaust space after the check.
use crate::{
    database::failed,
    maintenance::{SnapshotBudget, denied},
};
use riesel_storage::StoreError;
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// Refuse unexpectedly large or nonregular SQLite WAL/shared-memory sidecars
/// before undertaking an offline backup or restore. This bounds observed
/// physical input size, not future WAL growth or arbitrary hostile writers.
pub(crate) fn check_sidecars(
    database_path: &Path,
    budget: SnapshotBudget,
) -> Result<(), StoreError> {
    let mut bytes = 0_u64;
    for suffix in ["-wal", "-shm"] {
        let mut name = database_path.as_os_str().to_owned();
        name.push(suffix);
        let path = PathBuf::from(name);
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(failed(error)),
        };
        if !metadata.file_type().is_file() {
            return Err(denied("SQLite sidecar must be a regular file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() != 1 {
                return Err(denied("hard-linked SQLite sidecars are unsupported"));
            }
        }
        bytes = bytes
            .checked_add(metadata.len())
            .ok_or_else(|| denied("SQLite sidecar byte count overflow"))?;
        if bytes > budget.max_sidecar_bytes {
            return Err(denied("SQLite WAL/SHM sidecar byte budget exceeded"));
        }
    }
    Ok(())
}

/// Reserve two declared maximum database images as a conservative floor for
/// snapshot materialization or restoration WAL growth. The bound intentionally
/// favors rejection; it is not an upper bound on every possible SQLite I/O.
pub(crate) fn require_headroom(
    directory: &Path,
    max_snapshot_bytes: u64,
) -> Result<(), StoreError> {
    let available = available_bytes(directory)?;
    check_capacity(available, max_snapshot_bytes)
}

fn check_capacity(available: u64, max_snapshot_bytes: u64) -> Result<(), StoreError> {
    let required = max_snapshot_bytes
        .checked_mul(2)
        .filter(|required| *required > 0)
        .ok_or_else(|| denied("invalid or overflowing filesystem reserve"))?;
    if available < required {
        return Err(denied(
            "insufficient available filesystem space for the declared maintenance reserve",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn available_bytes(directory: &Path) -> Result<u64, StoreError> {
    use std::{ffi::CString, mem::MaybeUninit, os::unix::ffi::OsStrExt};

    let path = CString::new(directory.as_os_str().as_bytes())
        .map_err(|_| denied("filesystem path contains an embedded NUL"))?;
    let mut result = MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: path is a valid NUL-terminated C string and result points to
    // sufficient writable memory for the statvfs structure.
    if unsafe { libc::statvfs(path.as_ptr(), result.as_mut_ptr()) } != 0 {
        return Err(crate::database::failed(std::io::Error::last_os_error()));
    }
    // SAFETY: statvfs returned success and initialized the entire structure.
    let stat = unsafe { result.assume_init() };
    let available = u128::from(stat.f_bavail)
        .checked_mul(u128::from(stat.f_frsize))
        .ok_or_else(|| denied("filesystem capacity overflow"))?;
    u64::try_from(available).map_err(|_| denied("filesystem capacity exceeds supported range"))
}

#[cfg(windows)]
fn available_bytes(directory: &Path) -> Result<u64, StoreError> {
    use std::{iter, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let path: Vec<u16> = directory
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect();
    let mut available = 0_u64;
    // SAFETY: path is NUL-terminated and available is a valid writable u64.
    let success = unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if success == 0 {
        return Err(crate::database::failed(std::io::Error::last_os_error()));
    }
    Ok(available)
}

#[cfg(not(any(unix, windows)))]
fn available_bytes(_directory: &Path) -> Result<u64, StoreError> {
    Err(denied(
        "filesystem capacity checks are unsupported on this platform",
    ))
}

#[cfg(test)]
mod tests {
    use super::{check_capacity, require_headroom};

    #[test]
    fn rejects_insufficient_or_unrepresentable_reserves() {
        assert!(check_capacity(1_999, 1_000).is_err());
        assert!(check_capacity(2_000, 1_000).is_ok());
        assert!(check_capacity(u64::MAX, u64::MAX).is_err());
        assert!(check_capacity(u64::MAX, 0).is_err());
    }

    #[test]
    fn reads_actual_capacity_for_an_existing_local_directory() {
        let root = tempfile::tempdir().unwrap();
        assert!(require_headroom(root.path(), 1).is_ok());
    }
}
