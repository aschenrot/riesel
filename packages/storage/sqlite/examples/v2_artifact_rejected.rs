//! Separate compiled replacement artifact with an incompatible descriptor.
mod v2_artifact_shared;

use riesel_app::{PlanRevisionRepository, S2DocumentLimits};
use riesel_storage_sqlite::V2SnapshotBudget;
use std::{env, path::PathBuf};
use v2_artifact_shared::{NEW, OLD, handlers, old_heads, rust_plan, store};

fn main() {
    let mut args = env::args_os().skip(1);
    let path = PathBuf::from(args.next().expect("store path"));
    let snapshot = PathBuf::from(args.next().expect("backup path"));
    assert!(args.next().is_none());
    let old = store(&path, OLD);
    let new = store(&path, NEW);
    let before = old_heads(&old);
    // Compiled successor supplies descriptor v2, but stored document binds v1.
    let rejected = new.adopt_registered_build(
        &before,
        &snapshot,
        V2SnapshotBudget::default(),
        &handlers(2),
        &[rust_plan()],
        S2DocumentLimits::default(),
    );
    assert!(rejected.is_err());
    assert_eq!(old_heads(&old), before);
}
