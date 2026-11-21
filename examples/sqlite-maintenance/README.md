# Typed SQLite maintenance example

This independent Cargo workspace package demonstrates an **application-owned** offline SQLite migration command. The historical and current models use the SDK's `#[derive(RieselModel)]` macro, so the Rust fields are the only schema source; no handwritten `TypeRef` map, JSON schema or migration manifest is required.

The example supports one **sample** persistent model named `task`: `{title: String}` to `{title: String, priority: i64}`. It is **not** a migrator for Todo or an arbitrary existing database. The database must already exist with the exact historical schema for preview and execute.

From the Riesel workspace root:

```sh
cargo run -p riesel_sqlite_maintenance_example -- preview /trusted/app.db /trusted/backup.db
cargo run -p riesel_sqlite_maintenance_example -- execute /trusted/app.db /trusted/backup.db
cargo run -p riesel_sqlite_maintenance_example -- restore /trusted/app.db /trusted/backup.db /trusted/pre-restore.db
```

These file paths are examples and require an existing appropriately typed database, trusted local Linux filesystem, exclusive service shutdown barrier and selected budget. The `execute` and `restore` commands prompt for different exact approvals; there is no automatic migration during ordinary application launch. See the [operator runbook](../../docs/product/riesel/sqlite-offline-operator-runbook.md) and [SQLite provider guide](../../packages/storage/sqlite/README.md) for the supported limits, recovery procedure and exclusions.
