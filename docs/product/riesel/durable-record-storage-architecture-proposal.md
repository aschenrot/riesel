# Stage 6 — Durable Record Storage architecture

**Status: proposed for Gate B acceptance.** Decision evidence: [Issue #24](https://github.com/aschenrot/riesel/issues/24), [research brief](durable-record-storage-research.md), [backend comparison](durable-record-storage-evaluation.md), and [isolated Rust probes](https://github.com/aschenrot/riesel/pull/29). Merging the reviewed decision PR accepts the **bounded first-delivery contract below**; it does not certify production durability or complete Stage 6.

## Decision and rationale

Deliver a reusable **linked, SQLite-backed** provider for `RecordStore` v2, using `rusqlite`. The first supported deployment is **one local host, one logical provider owner per database file**, on a documented local filesystem. A managed-process provider, PostgreSQL-based remote provider, sharing the file across hosts, and replication are separate future deliveries. Do not imply local/managed parity until the managed model-configuration and readiness semantics are accepted and verified.

SQLite is preferred for the first provider because it has documented transaction/synchronization behavior, established inspection/backup and schema-management facilities, and can implement ordered CRUD with revision-checked atomic changes. **redb remains a viable alternative**, especially for an accepted pure-Rust requirement; the candidate research did not identify a decisive correctness disadvantage in either engine. This choice is based on first-delivery operational fit, not unmeasured benchmarks or a preference for SQL itself.

## Accepted scope upon integration

| Area | First-delivery requirement |
| --- | --- |
| Capability | Preserve the published `RecordStore` v2 operations, `Records<T>` API and error variants; do not change unrelated capabilities |
| Placement | Linked provider only. Applications register it explicitly; it must never become an implicit fallback from an unavailable persistent provider |
| Database ownership | Explicit path, single logical provider owner, and local disk; no claim of multi-process direct-file sharing or network-filesystem safety |
| Preflight | Inspect factory metadata, contracts and placement **without opening, creating or modifying** any database files |
| Startup | Open/create the database, configure SQLite, inspect persistent metadata and fail through the existing local factory result **before** capability use |
| Mutations | Each acknowledged write is a committed SQLite transaction; update/delete use atomic compare-and-swap against the expected revision; stale and missing records stay distinguishable |
| Durability | Initial WAL with `synchronous=FULL`; acknowledge only after a successful commit. State explicitly that real host power-loss safety also depends on OS, filesystem and device behavior |
| Identity and pagination | Stable UUIDs and full `u64` revisions across restarts; ordered model+UUID bounded pages with the existing non-snapshot cursor behavior |
| Model enforcement | Provider validates authoritative application model definitions even for raw `RecordStore` callers; unknown models and invalid data fail |
| Compatibility | Persist format/version and model definition metadata. Initially **exact schema match** for reopening existing databases; incompatible schema fails during startup without rewriting records; migrations deferred |
| Failure behavior | Preserve `NotFound`, `RevisionConflict`, `InvalidPageSize`, `Failed`, and `Transport`; fail closed on database open, permission, lock, corruption or incompatible schema errors; do not silently repair corrupted data |
| Recovery | Existing runtime session semantics remain; no automatic retry/exactly-once command semantics, transparent rebinding or silent MemoryStore fallback |

The 32-record page limit is contractual. Mirror the current MemoryStore 64 KiB serialized record bound in this first provider to respect existing RPC frame budgets; changing it requires explicit review.

### Data representation

- Key rows by model name and canonical 16-byte UUID BLOB, using order-preserving SQLite BLOB sorting; verify UUID ordering and exclusive page cursors against the memory provider.
- Store revision as a fixed **8-byte big-endian BLOB**, not SQLite signed `INTEGER` (which cannot represent every `u64`). Reject `u64::MAX` updates with a documented overflow error; delete at that revision remains possible.
- Store JSON data in a bounded deterministic representation; validate its shape, declared model fields and maximum size before any mutation. Treat invalid decoded persistent records as errors rather than returning misleading application data.
- Use explicit SQLite transactions for create/update/delete and metadata updates. If a conditional write affects zero rows, distinguish missing records from stale revisions **inside the same transaction**. Generate UUIDs in the application/provider and retry an extremely unlikely insert collision without reusing an acknowledged ID.
- Version the provider's on-disk format separately from application model shapes. Compare stored model metadata against the supplied model set on startup; do not auto-migrate existing records in the first delivery.

## Reuse existing runtime mechanisms

### Fallible startup — no new generic runtime API needed

`LocalProviderFactory::create()` already returns `Result<LocalProviderInstance, ConnectorError>`. The existing generic `LocalComponentFactory` accepts a fallible creation closure, and runtime startup already propagates local factory errors and rolls back earlier initialized components. **Use these interfaces for the linked provider.**

The SQLite connection must be opened only within that closure, not in provider registration, `definition()`, `contracts()`, preflight or a `ComponentDefinition::new` infallible constructor. No generic fallible `ComponentDefinition` extension is required for this scope. Such an extension may be considered later if managed-component parity or another reusable use case justifies it; this decision does not approve it.

### Model metadata — no second source of truth

`Application::records<T>()` already registers Rust-derived definitions in `AppSchema.models` with `model_bindings` indicating the Storage requirement. Reuse these definitions to construct the linked provider; do **not** require application authors to maintain a parallel schema file or trust only SDK-side checks.

The implementation must make an unambiguous binding between a selected Storage alias, its registered models and its provider configuration. An explicit SDK helper is acceptable if its API remains minimal and does not silently collect models from unrelated aliases. Verify builder call ordering or defer configuration binding until the final application schema is known. The backend provider API itself may accept explicit model definitions for standalone tests.

A managed provider will additionally require an accepted authenticated/owned model-configuration channel and startup validation before it advertises readiness; **that work is not part of the initial linked delivery**.

### Path, errors and operational safety

Use an explicit per-provider path and startup policy; no hardcoded ports, implicit working-directory path or implicit Storage fallback. Verify file creation permissions and failure behavior. Document SQLite WAL side files, locking/timeout behavior and backup/restore (use SQLite's backup facilities or a documented safe shutdown procedure). Do not claim networked or multi-owner file access is supported.

`StoreError::Failed` is adequate to preserve v2 for first-delivery operational failures only if diagnostic messages are safe and actionable. Record error categories in tests, without changing the transport contract or leaking secrets/filesystem data inadvertently.

## Evidence and limitations

On 2026-11-19, [Rust 1.100.0 research CI](https://github.com/aschenrot/riesel/actions/runs/44569551231) passed **5/5** isolated tests for `rusqlite 0.43.0` and `redb 5.0.0`: ordered keys, full `u64` representations, revision conflicts, competing writers and committed values after forced process exit **with handles still live**. The initial process-exit test was corrected before this result (see [Issue #24](https://github.com/aschenrot/riesel/issues/24#issuecomment-7015633350)). These tests confirm narrow engine feasibility; they do **not** prove Riesel integration, cross-process ownership, fault injection, OS-crash behavior or power-loss durability.

The accepted implementation must add Riesel-specific tests for provider registration/preflight purity, missing/invalid paths, schema mismatch, raw model validation, parity against MemoryStore, concurrent revisions, page boundaries, clean restart and process-termination recovery. Appropriate smoke and standard `cargo validate --check` / `cargo smoke` are mandatory. Windows remains a manual pre-release check; Linux CI alone is not production certification.

## Implementation boundaries and dependencies

1. **Durable provider:** a separate `packages/storage/` implementation using the existing fallible `LocalComponentFactory`, explicit configuration, persistent metadata and atomic `RecordStore` operations.
2. **Developer-facing linked registration:** bind already-declared Rust models to the correct Storage alias without manual duplicate schema. Work may proceed in parallel with the provider if the provider constructor/contract is agreed.
3. **Parity and restart verification:** independent fixtures and contract-level tests, integrated with the provider before merging accepted behavior.
4. **Todo R1:** adopt the accepted provider and implement usable headless commands only after durable Storage and startup contracts pass.

Do not interpret completion of this first Storage slice as completion of Stage 6 production reliability. Managed-process durability, migrations, network trust, authorization, external connectivity and generic fallible component construction remain independently scoped.

**Gate B decision:** an accountable reviewer must explicitly accept, revise or defer this limited architecture. After acceptance is recorded in Issue #24 and the corresponding PR is merged, create independently verifiable implementation Issues with accepted dependencies and verification boundaries.
