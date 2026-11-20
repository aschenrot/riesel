# SQLite offline migration — operator runbook

**Supported boundary:** bounded format-v1, trusted local Linux filesystem, one cooperating owner per SQLite database. This procedure is not online migration, corrupt-file repair, network orchestration, uncooperative-process isolation or a host power-loss guarantee.

## Build the application-specific maintenance entrypoint

Author historical and current `RieselModel` Rust types with the same persistent model name. Construct the complete `ModelEvolution` with deliberate typed transforms and, where necessary, registered cross-model invariants. Supply a finite `EvolutionLimits` reflecting the actual deployment to `run_operator_maintenance_cli`. Do not introduce an independent schema file. The reusable [example](../../../packages/storage/sqlite/examples/offline_evolution.rs) demonstrates the command wiring for a **sample** task model, not arbitrary SQLite databases.

```sh
cargo run -p riesel_storage_sqlite --example offline_evolution -- preview /trusted/app.db /trusted/source-backup.db
cargo run -p riesel_storage_sqlite --example offline_evolution -- execute /trusted/app.db /trusted/source-backup.db
cargo run -p riesel_storage_sqlite --example offline_evolution -- restore /trusted/app.db /trusted/source-backup.db /trusted/pre-restore.db
```

These paths are illustrative. The database must already exist with **exact historical metadata** for preview/execute. Restore requires an existing **healthy, evolved target database** and the verified historical archive. Use a project-specific executable with its *actual* historical/current models; do not apply the sample plan to unrelated files.

## Prerequisites

1. Stop **all** owning providers, old Riesel binaries and direct SQLite writers, and prevent external services from restarting them. Cooperative owner locks cannot stop uncooperative or pre-fencing processes.
2. Use a trusted local filesystem with stable regular database, owner-lock, WAL/SHM and recovery-directory identities and permissions. Never delete/replace an active or persistent `.riesel-owner.lock` file. Network filesystems and unverified Windows targets are not supported.
3. Choose accurate finite record count, JSON byte, database snapshot, combined WAL/SHM and elapsed-time ceilings. The example's numbers are merely sample limits; invalid limits abort.
4. Select an **unoccupied** independent recovery-backup path and, before restore, a second unoccupied pre-restore retention path. The tool refuses overwrites and source/backup aliasing.
5. Provision space on the database and recovery volumes. Preflight conservatively requires at least **twice the declared maximum snapshot size** available on each involved volume and displays source size, observed WAL/SHM, available capacity and the target JSON projection. It **does not reserve** space, predict all WAL/temp file growth, or prevent other processes consuming capacity.

## Preview and separately approved execute

Run `preview DATABASE BACKUP` first. It acquires cooperative offline ownership, audits every stored source record and exact historical model set, evaluates typed transforms and prints changed Rust-derived model shapes, affected record counts and physical capacity observations. It makes no source-record mutation or completed backup artifact. Application-level invariants that must inspect the *resulting* database are not certified by read-only preview.

Run `execute DATABASE BACKUP` **only after reviewing** the output and confirming that legacy process shutdown and restart blocking are active. The command re-previews and requires typing the exact `MIGRATE <canonical database path> BACKUP <canonical backup destination>` phrase displayed. Missing, incorrect or truncated consent fails without migration or backup publication. There is deliberately no `--yes` flag. An operator must control the invoking terminal; this is not authenticated identity.

Within the same in-process reviewed session, Riesel takes and verifies an independent retained `VACUUM INTO` recovery snapshot. It re-audits complete source metadata and checks exact previewed UUID/revision/JSON bytes **inside one SQLite IMMEDIATE transaction before any writes**. Byte-neutral direct modifications and inserted rows cause a failure, even if aggregate counts and model metadata still appear compatible. A rejected drift may leave a verified backup of the newer source; it must not be mistaken for the operator-approved preimage.

After a successful migration, verify current Rust models open with the new data and changed-model revisions advance once; old Rust models must fail to open. Retain the historical source archive under operator control. Never silently restart an old binary.

## Recovery and restore

Run `restore DATABASE HISTORICAL_BACKUP RETAIN_CURRENT` only after separately checking consequences. The command validates the archived source and the currently healthy target format, checks the capacity/retention destination and requires a **different** exact phrase: `RESTORE <canonical database path> FROM <canonical historical archive> RETAIN <canonical pre-restore destination> DISCARD POST-SNAPSHOT WRITES`. All writes after the historical snapshot will be lost in the restored database. The command takes and verifies an independent copy of the *newer* state at the retention path before the restore operation.

After a successful restore, verify historical models open, current models reject startup and the newer pre-restore copy is preserved. The restore API does not repair an unreadable/physically corrupt destination. A post-copy restore failure may leave the state uncertain.

**Error handling:** a migration error before SQLite COMMIT rolls back all row/schema writes; the completed preimage can remain retained. An error **after** COMMIT may occur even though the new schema has already been installed. Never automatically retry or restore solely from an error status. Stop writers, inspect which Rust model opens and verify retained artifacts before another separately approved operation.

## Tested scope and exclusions

Linux CI covers typed execution and restore, explicit consent/refusal, preview/source drift detection, corrupt data, backup validity and preflight budgets. This is not a guarantee against true filesystem-wide ENOSPC after preflight, blocked OS IO deadlines, device/host power loss, malicious same-user tampering, uncooperative legacy writers, remote/online migrations or Windows release parity. Windows remains a separate manual pre-release validation gate.

Accepted contract: [SQLite offline migration architecture](sqlite-offline-migration-architecture.md). Outstanding operator-workflow acceptance and fault cases: [#51](https://github.com/aschenrot/riesel/issues/51).
