//! Research-only probes for a cooperating *lifetime* owner lock.
//! This file does not change SQLite Store startup or introduce a lock protocol.
//! File locks are OS/platform dependent and may be advisory.
use std::{fs::OpenOptions, path::Path, process::Command};

/// The standard-library file lock can exclude a cooperating second process
/// while the owner is active and is released on normal owner termination.
/// This does NOT prevent legacy binaries or raw SQLite clients from ignoring it.
#[test]
fn cooperating_cross_process_file_lock_blocks_until_owner_releases() {
    const PATH: &str = "RIESEL_OWNERSHIP_RESEARCH_LOCK_PATH";
    const EXPECT_FREE: &str = "RIESEL_OWNERSHIP_RESEARCH_EXPECT_FREE";

    fn open_lock(path: &Path) -> std::fs::File {
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .unwrap()
    }

    if let Some(lock_path) = std::env::var_os(PATH) {
        let file = open_lock(Path::new(&lock_path));
        let could_lock = file.try_lock().is_ok();
        let expect_free = std::env::var_os(EXPECT_FREE).is_some();
        assert_eq!(
            could_lock, expect_free,
            "cooperating process saw unexpected lifetime lock state"
        );
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.owner.lock");
    let primary = open_lock(&path);
    primary.try_lock().unwrap();

    let observe = |expect_free: bool| {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args([
            "--exact",
            "cooperating_cross_process_file_lock_blocks_until_owner_releases",
        ])
        .env(PATH, &path);
        if expect_free {
            cmd.env(EXPECT_FREE, "1");
        }
        let result = cmd.output().unwrap();
        assert!(
            result.status.success(),
            "lock observation failed (expect_free={expect_free}): {}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    observe(false);
    drop(primary);
    observe(true);
}

/// On Unix a renamed/unlinked lockfile retains its old inode and lock.
/// A new file at the same path is a *different lock identity*. This is why
/// lock-file deletion or replacement must be excluded by policy and verified.
#[cfg(unix)]
#[test]
fn replacement_of_cooperating_lockfile_creates_a_second_lock_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db.owner.lock");
    let displaced = dir.path().join("displaced.owner.lock");
    let original = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    original.try_lock().unwrap();
    std::fs::rename(&path, &displaced).unwrap();
    let replacement = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    replacement.try_lock().unwrap();
    assert_ne!(
        std::fs::canonicalize(&path).unwrap(),
        std::fs::canonicalize(&displaced).unwrap()
    );
    // Both independent file objects remain locked simultaneously. Therefore
    // canonicalizing a parent directory is insufficient against replacement.
}
