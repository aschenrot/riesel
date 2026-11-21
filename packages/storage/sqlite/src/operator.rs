//! Deliberate, typed and standalone offline SQLite maintenance CLI workflow.
//! An application's own Rust binary supplies historical/current model types,
//! the complete ModelEvolution, limits, and this strict command surface.
use crate::{
    EvolutionReceipt,
    capacity::{available_bytes, observed_sidecar_bytes, require_headroom},
    database::failed,
    evolution::{EvolutionLimits, EvolutionPreview, ModelEvolution},
    maintenance::{SqliteMaintenance, denied, output_path},
    restore::{RestoreAuthorization, RestoreReceipt},
    review::ReviewedEvolutionAuthorization,
};
use riesel_storage::StoreError;
use std::{
    ffi::OsString,
    fs,
    io::{BufRead, Write},
    path::Path,
};

#[derive(Debug)]
pub enum OperatorOutcome {
    Preview(EvolutionPreview),
    Executed(EvolutionReceipt),
    Restored(RestoreReceipt),
}

fn write_summary(
    output: &mut impl Write,
    operation: &str,
    database: &Path,
    backup_destination: &Path,
    plan: &ModelEvolution,
    limits: EvolutionLimits,
    preview: EvolutionPreview,
) -> Result<(), StoreError> {
    // The shared low-level backup target preflight prevents accidental
    // overwrite, unsafe parent paths and aliases of the active source.
    let backup = output_path(backup_destination, database)?;
    let db_parent = database
        .parent()
        .ok_or_else(|| denied("database has no trusted parent directory"))?;
    let backup_parent = backup
        .parent()
        .ok_or_else(|| denied("backup destination has no trusted parent directory"))?;
    require_headroom(db_parent, limits.snapshot.max_snapshot_bytes)?;
    require_headroom(backup_parent, limits.snapshot.max_snapshot_bytes)?;
    let reserved = limits
        .snapshot
        .max_snapshot_bytes
        .checked_mul(2)
        .ok_or_else(|| denied("declared backup reserve overflows"))?;
    let physical_bytes = fs::metadata(database).map_err(failed)?.len();
    let sidecar_bytes = observed_sidecar_bytes(database, limits.snapshot)?;
    let free_db = available_bytes(db_parent)?;
    let free_backup = available_bytes(backup_parent)?;
    writeln!(output, "{operation}: OFFLINE SQLite model evolution").map_err(failed)?;
    writeln!(output, "Database: {}", database.display()).map_err(failed)?;
    writeln!(output, "Independent recovery backup: {}", backup.display()).map_err(failed)?;
    writeln!(
        output,
        "Records: {}; changed models: {}; affected records: {}",
        preview.records, preview.changed_models, preview.affected_records
    )
    .map_err(failed)?;
    writeln!(
        output,
        "Projected JSON bytes: {}; current database file bytes: {}",
        preview.projected_json_bytes, physical_bytes
    )
    .map_err(failed)?;
    writeln!(
        output,
        "Observed SQLite WAL/SHM bytes: {} (budget {})",
        sidecar_bytes, limits.snapshot.max_sidecar_bytes
    )
    .map_err(failed)?;
    writeln!(
        output,
        "Max independent snapshot bytes: {}; minimum free bytes per involved volume: {}",
        limits.snapshot.max_snapshot_bytes, reserved
    )
    .map_err(failed)?;
    writeln!(
        output,
        "Available on database volume: {}; available on backup volume: {}",
        free_db, free_backup
    )
    .map_err(failed)?;
    for (name, old) in &plan.source {
        let updated = &plan.target[name];
        if old != updated {
            let source = serde_json::to_string(old).map_err(failed)?;
            let target = serde_json::to_string(updated).map_err(failed)?;
            writeln!(output, "Model {name}: {source} -> {target}").map_err(failed)?;
        }
    }
    writeln!(
        output,
        "Preflight passed, but filesystem capacity is not reserved; SQLite writes may still fail."
    )
    .map_err(failed)?;
    writeln!(
        output,
        "Operator must stop older/uncooperative SQLite clients and prevent restart externally."
    )
    .map_err(failed)?;
    writeln!(
        output,
        "Only a trusted local Linux filesystem is verified; no online/remote/Windows/power-loss guarantee."
    )
    .map_err(failed)?;
    writeln!(
        output,
        "Preview does not validate post-transform cross-model invariants."
    )
    .map_err(failed)?;
    output.flush().map_err(failed)
}

fn require_confirmation(
    input: &mut impl BufRead,
    output: &mut impl Write,
    phrase: &str,
) -> Result<(), StoreError> {
    writeln!(
        output,
        "To authorize this destructive operation, enter exactly:"
    )
    .map_err(failed)?;
    writeln!(output, "{phrase}").map_err(failed)?;
    output.flush().map_err(failed)?;
    // Never accumulate an unbounded line while holding the exclusive owner
    // and an exact copy of the previewed records in memory. A valid answer
    // needs at most the displayed phrase plus CRLF.
    let max_bytes = phrase
        .len()
        .checked_add(2)
        .ok_or_else(|| denied("operator approval length overflow"))?;
    let mut reply = Vec::new();
    if input
        .take(max_bytes as u64)
        .read_until(b'\n', &mut reply)
        .map_err(failed)?
        == 0
    {
        return Err(denied("operator approval missing; nothing was changed"));
    }
    if reply.last() != Some(&b'\n') {
        return Err(denied(
            "operator approval must be a complete, bounded line; nothing was changed",
        ));
    }
    let reply = std::str::from_utf8(&reply)
        .map_err(|_| denied("operator approval is not UTF-8; nothing was changed"))?;
    if reply.trim_end_matches(['\r', '\n']) != phrase {
        return Err(denied(
            "operator approval did not match; nothing was changed",
        ));
    }
    Ok(())
}

/// Run an explicit maintenance command using the application's own Rust
/// model-evolution plan. This never attaches to normal provider startup.
///
/// Commands:
/// - `preview DATABASE BACKUP_DESTINATION` — read-only source review
/// - `execute DATABASE BACKUP_DESTINATION` — preview, preflight, explicit
///   confirmation, then retained backup and drift-checked evolution
/// - `restore DATABASE SOURCE_BACKUP RETAIN_CURRENT_AT` — validate an
///   existing target, explicitly confirm discarding newer writes, retain
///   current state, then restore the archived source state
///
/// `input` and `output` are injected for isolated tests and deliberate
/// terminal integration. There is no `--yes`, default execution or retry.
pub fn run_operator_maintenance_cli(
    args: &[OsString],
    plan: &ModelEvolution,
    limits: EvolutionLimits,
    input: &mut impl BufRead,
    output: &mut impl Write,
) -> Result<OperatorOutcome, StoreError> {
    let operation = args.first().and_then(|value| value.to_str()).unwrap_or("");
    match (operation, args.len()) {
        ("preview", 3) | ("execute", 3) => {
            let database = Path::new(&args[1]);
            let backup = Path::new(&args[2]);
            let maintenance =
                SqliteMaintenance::open_existing(database, plan.source.clone(), limits.snapshot)?;
            if operation == "preview" {
                let preview = maintenance.preview_evolution(plan, limits)?;
                write_summary(
                    output,
                    "PREVIEW",
                    maintenance.owner.database_path(),
                    backup,
                    plan,
                    limits,
                    preview,
                )?;
                return Ok(OperatorOutcome::Preview(preview));
            }
            let reviewed = maintenance.review_evolution(plan, limits)?;
            write_summary(
                output,
                "EXECUTE",
                reviewed.database_path(),
                backup,
                plan,
                limits,
                reviewed.preview(),
            )?;
            let backup_path = output_path(backup, reviewed.database_path())?;
            let phrase = format!(
                "MIGRATE {} BACKUP {}",
                reviewed.database_path().display(),
                backup_path.display()
            );
            require_confirmation(input, output, &phrase)?;
            let receipt = reviewed.execute(
                backup,
                ReviewedEvolutionAuthorization::ApproveInspectedPreview,
            )?;
            writeln!(
                output,
                "Migration committed: {} records; retained source: {}",
                receipt.records,
                receipt.retained_backup.path.display()
            )
            .map_err(failed)?;
            Ok(OperatorOutcome::Executed(receipt))
        }
        ("restore", 4) => {
            let database = Path::new(&args[1]);
            let archive = Path::new(&args[2]);
            let retain = Path::new(&args[3]);
            // Validate the source backup with the historical Rust-derived
            // schema, before prompting the operator to discard later writes.
            let verified_source =
                SqliteMaintenance::open_existing(archive, plan.source.clone(), limits.snapshot)?;
            let archive_path = verified_source.owner.database_path().to_owned();
            drop(verified_source);
            let maintenance =
                SqliteMaintenance::open_existing(database, plan.target.clone(), limits.snapshot)?;
            let database = maintenance.owner.database_path().to_owned();
            let retention = output_path(retain, &database)?;
            let database_parent = database
                .parent()
                .ok_or_else(|| denied("database has no trusted parent directory"))?;
            let retention_parent = retention
                .parent()
                .ok_or_else(|| denied("retention has no trusted parent directory"))?;
            require_headroom(database_parent, limits.snapshot.max_snapshot_bytes)?;
            require_headroom(retention_parent, limits.snapshot.max_snapshot_bytes)?;
            writeln!(
                output,
                "RESTORE (will discard all writes after archived snapshot)"
            )
            .map_err(failed)?;
            writeln!(output, "Database: {}", database.display()).map_err(failed)?;
            writeln!(
                output,
                "Verified source snapshot: {}",
                archive_path.display()
            )
            .map_err(failed)?;
            writeln!(
                output,
                "Retain current database at: {}",
                retention.display()
            )
            .map_err(failed)?;
            writeln!(
                output,
                "Restoration requires existing healthy format-v1 current storage, not corrupt-file repair."
            )
            .map_err(failed)?;
            writeln!(
                output,
                "Stop all legacy/uncooperative writers and prevent restart; data loss is irreversible without the retained pre-restore copy."
            )
            .map_err(failed)?;
            let phrase = format!(
                "RESTORE {} FROM {} RETAIN {} DISCARD POST-SNAPSHOT WRITES",
                database.display(),
                archive_path.display(),
                retention.display()
            );
            require_confirmation(input, output, &phrase)?;
            let receipt = maintenance.restore_from_backup(
                &archive_path,
                plan.source.clone(),
                retain,
                limits.snapshot,
                RestoreAuthorization::DiscardPostSnapshotWrites,
            )?;
            writeln!(
                output,
                "Restored {} records; retained newer state: {}",
                receipt.restored_records,
                receipt.retained_pre_restore.path.display()
            )
            .map_err(failed)?;
            Ok(OperatorOutcome::Restored(receipt))
        }
        _ => Err(denied(
            "usage: preview DATABASE BACKUP | execute DATABASE BACKUP | restore DATABASE SOURCE_BACKUP RETAIN_CURRENT",
        )),
    }
}
