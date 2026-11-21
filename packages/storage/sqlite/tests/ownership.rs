use riesel_schema::{ModelDefinition, TypeRef};
use riesel_storage::{RecordStore, StoreError};
use riesel_storage_sqlite::SqliteStore;
use serde_json::json;
use std::{collections::BTreeMap, path::Path, process::Command};

fn models() -> BTreeMap<String, ModelDefinition> {
    BTreeMap::from([(
        "task".into(),
        ModelDefinition {
            fields: BTreeMap::from([("title".into(), TypeRef::String)]),
        },
    )])
}

fn run_probed_owner(path: &Path, available: bool) {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "cooperating_sqlite_owner_is_exclusive_across_processes",
    ]);
    cmd.env("RIESEL_SQLITE_OWNER_DB", path);
    if available {
        cmd.env("RIESEL_SQLITE_OWNER_EXPECT_FREE", "1");
    }
    let result = cmd.output().unwrap();
    assert!(
        result.status.success(),
        "other process expected available={available}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn cooperating_sqlite_owner_is_exclusive_across_processes() {
    if let Some(db) = std::env::var_os("RIESEL_SQLITE_OWNER_DB") {
        let available = std::env::var_os("RIESEL_SQLITE_OWNER_EXPECT_FREE").is_some();
        let result = SqliteStore::open(std::path::PathBuf::from(db), models());
        assert_eq!(result.is_ok(), available);
        return;
    }
    let temporary = tempfile::tempdir().unwrap();
    let db = temporary.path().join("records.db");
    let store = SqliteStore::open(&db, models()).unwrap();
    assert!(matches!(
        SqliteStore::open(&db, models()),
        Err(StoreError::Failed(_))
    ));
    run_probed_owner(&db, false);
    drop(store);
    run_probed_owner(&db, true);
    SqliteStore::open(&db, models()).unwrap();
}

#[cfg(unix)]
#[test]
fn parent_symlink_aliases_share_the_same_owner_identity() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let db = temporary.path().join("records.db");
    let alias = temporary.path().join("directory-alias");
    symlink(temporary.path(), &alias).unwrap();
    let store = SqliteStore::open(&db, models()).unwrap();
    assert!(matches!(
        SqliteStore::open(alias.join("records.db"), models()),
        Err(StoreError::Failed(_))
    ));
    drop(store);
    // Preserve the reason if the released owner remains unexpectedly locked.
    if let Err(error) = SqliteStore::open(alias.join("records.db"), models()) {
        panic!("reopening the released parent-directory alias failed: {error:?}");
    }
}

#[cfg(unix)]
#[test]
fn rejects_file_symlinks_and_hardlink_aliases() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let db = temporary.path().join("records.db");
    drop(SqliteStore::open(&db, models()).unwrap());

    let symlink_path = temporary.path().join("symlink.db");
    symlink(&db, &symlink_path).unwrap();
    assert!(matches!(
        SqliteStore::open(&symlink_path, models()),
        Err(StoreError::Failed(_))
    ));

    let hardlink_path = temporary.path().join("hardlink.db");
    std::fs::hard_link(&db, &hardlink_path).unwrap();
    assert!(matches!(
        SqliteStore::open(&db, models()),
        Err(StoreError::Failed(_))
    ));
    std::fs::remove_file(&hardlink_path).unwrap();
    assert!(SqliteStore::open(&db, models()).is_ok());
}

#[cfg(unix)]
#[test]
fn lockfile_replacement_is_detected_and_old_provider_fails_closed() {
    let temporary = tempfile::tempdir().unwrap();
    let db = temporary.path().join("records.db");
    let store = SqliteStore::open(&db, models()).unwrap();
    let first = store.create("task", json!({"title":"preserved"})).unwrap();
    let lock_path = temporary.path().join("records.db.riesel-owner.lock");
    let removed_path = temporary.path().join("displaced.owner");
    std::fs::rename(&lock_path, &removed_path).unwrap();
    std::fs::File::create(&lock_path).unwrap();

    assert!(matches!(
        store.get("task", first.id),
        Err(StoreError::Failed(_))
    ));
    assert!(matches!(
        store.create("task", json!({"title":"blocked"})),
        Err(StoreError::Failed(_))
    ));
    // A replaceable pathname cannot enforce exclusivity against the attacker
    // that performs replacement. This test only verifies detection by the
    // already-open provider, not prevention of the replacement itself.
}

#[cfg(unix)]
#[test]
fn refuses_insecure_parent_or_lock_permissions_before_opening_database() {
    use std::os::unix::fs::PermissionsExt;

    let temporary = tempfile::tempdir().unwrap();
    let parent = temporary.path().join("unsafe-parent");
    std::fs::create_dir(&parent).unwrap();
    let db = parent.join("records.db");
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(matches!(
        SqliteStore::open(&db, models()),
        Err(StoreError::Failed(_))
    ));
    assert!(!db.exists());
    assert!(!parent.join("records.db.riesel-owner.lock").exists());
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();

    drop(SqliteStore::open(&db, models()).unwrap());
    let lock_path = parent.join("records.db.riesel-owner.lock");
    std::fs::set_permissions(&lock_path, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert!(matches!(
        SqliteStore::open(&db, models()),
        Err(StoreError::Failed(_))
    ));
    std::fs::set_permissions(&lock_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(SqliteStore::open(&db, models()).is_ok());
}

/// Symlinked lock paths cannot be trusted as unique ownership identities.
#[cfg(unix)]
#[test]
fn refuses_symlinked_lockfile_before_creating_a_database() {
    use std::os::unix::fs::symlink;

    let temporary = tempfile::tempdir().unwrap();
    let db = temporary.path().join("records.db");
    let target = temporary.path().join("unrelated.file");
    std::fs::write(&target, b"untouched").unwrap();
    let lock = temporary.path().join("records.db.riesel-owner.lock");
    symlink(&target, &lock).unwrap();

    assert!(matches!(
        SqliteStore::open(&db, models()),
        Err(StoreError::Failed(_))
    ));
    assert!(!db.exists());
    assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
}

/// The already-running provider refuses to write through a database path
/// switched to a different inode. This is detection, not a filesystem lease
/// against a hostile operator.
#[cfg(unix)]
#[test]
fn detects_database_path_replacement_before_record_mutation() {
    let temporary = tempfile::tempdir().unwrap();
    let db = temporary.path().join("records.db");
    let store = SqliteStore::open(&db, models()).unwrap();
    store.create("task", json!({"title":"keep"})).unwrap();
    std::fs::rename(&db, temporary.path().join("moved-original.db")).unwrap();
    std::fs::write(&db, b"replacement").unwrap();

    assert!(matches!(
        store.create("task", json!({"title":"blocked"})),
        Err(StoreError::Failed(_))
    ));
    assert!(matches!(
        store.list_page("task", None, 32),
        Err(StoreError::Failed(_))
    ));
}
