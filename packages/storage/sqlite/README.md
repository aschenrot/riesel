# SQLite Record Storage provider

An explicit, reusable **linked-only** `RecordStore` v2 implementation. The constructor `sqlite_storage_provider(path, models)` is side-effect-free; the database is opened by the existing fallible local factory during runtime startup.

- SQLite WAL with `synchronous=FULL`, explicit local-disk path and one cooperating lifetime owning provider per file (Linux-verified exclusive sidecar lock).
- UUID-ordered records, 8-byte `u64` revisions, transactional optimistic update/delete, bounded pages, and 64 KiB JSON payloads.
- Requires authoritative model definitions and rejects incompatible persisted model metadata on reopen rather than silently migrating data.
- Database open, permission, lock, schema and corruption errors fail without falling back to MemoryStore.

**Not provided:** runtime-managed Storage hosting, schema migrations, network-filesystem sharing, transparent retries or OS/power-loss certification. Engine-level forced-process-exit research lives separately in [PR #29](https://github.com/aschenrot/riesel/pull/29). See the [accepted architecture](../../../docs/product/riesel/durable-record-storage-architecture-proposal.md).

**Cached-schema write safety:** Existing linked providers verify the entire persisted model registration and physical format within each SQLite create/update/delete transaction. A provider whose cached model set is no longer the stored model set rejects writes without mutating records. This does not prevent legacy/uncooperative writers, does not implement a migration engine or enforce exclusive database ownership, and does not establish safe online schema evolution. See [issue #44](https://github.com/aschenrot/riesel/issues/44) and [Stage 6 specification #41](https://github.com/aschenrot/riesel/issues/41).

### Cooperative owner boundary

The linked provider now acquires an exclusive standard-library file lock on a **persistent** `<database>.riesel-owner.lock` sidecar before SQLite opens. It holds the file handle through provider shutdown and refuses a second cooperating owner. Relative/parent-symlink paths resolve to one canonical database path. On Linux, unsupported direct file symlinks, hardlinks, writable-by-group/others database directories/files/lockfiles and detected lock/database identity changes fail closed. The lockfile remains after shutdown and must **not** be deleted or replaced during operation.

This is an operator-controlled, trusted-local-filesystem mechanism. It does **not** prevent pre-upgrade Riesel binaries, arbitrary SQLite clients, adversarial same-user directory mutation, or races against malicious pathname replacement. The already-open provider detects a visible replacement at its next operation but this is not a cross-process tamper-resistant lease. Stop old binaries and block their restart before any maintenance; do not infer this automatically. Windows file identity/path-policy validation remains a manual pre-release gate. See [ownership implementation #48](https://github.com/aschenrot/riesel/issues/48) and the [accepted offline migration architecture](../../../docs/product/riesel/sqlite-offline-migration-architecture.md). No actual migration/backup feature is enabled by this guard.
