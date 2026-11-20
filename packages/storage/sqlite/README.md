# SQLite Record Storage provider

An explicit, reusable **linked-only** `RecordStore` v2 implementation. The constructor `sqlite_storage_provider(path, models)` is side-effect-free; the database is opened by the existing fallible local factory during runtime startup.

- SQLite WAL with `synchronous=FULL`, local-disk path and one logical owning provider per file.
- UUID-ordered records, 8-byte `u64` revisions, transactional optimistic update/delete, bounded pages, and 64 KiB JSON payloads.
- Requires authoritative model definitions and rejects incompatible persisted model metadata on reopen rather than silently migrating data.
- Database open, permission, lock, schema and corruption errors fail without falling back to MemoryStore.

**Not provided:** runtime-managed Storage hosting, schema migrations, network-filesystem sharing, transparent retries or OS/power-loss certification. Engine-level forced-process-exit research lives separately in [PR #29](https://github.com/aschenrot/riesel/pull/29). See the [accepted architecture](../../../docs/product/riesel/durable-record-storage-architecture-proposal.md).

**Cached-schema write safety:** Existing linked providers verify the entire persisted model registration and physical format within each SQLite create/update/delete transaction. A provider whose cached model set is no longer the stored model set rejects writes without mutating records. This does not prevent legacy/uncooperative writers, does not implement a migration engine or enforce exclusive database ownership, and does not establish safe online schema evolution. See [issue #44](https://github.com/aschenrot/riesel/issues/44) and [Stage 6 specification #41](https://github.com/aschenrot/riesel/issues/41).
