# Stage 6 — Durable Record Storage: proposed specification

**Status: proposal, not accepted.** Decision and evidence: [Specification Issue #24](https://github.com/aschenrot/riesel/issues/24). This document is for review and does not authorize an implementation or commit Riesel to a database engine.

## Outcome and scope

Provide a durable `RecordStore` implementation usable by Riesel applications without changing their typed `Records<T>` domain code. The first consumer is Todo R1; the provider must remain reusable by other applications.

This is an **embedded, single-host Storage** proposal, not a distributed database or remote Storage service. The precise multi-process file ownership and durability defaults remain decisions to be accepted below.

## Current baseline

- `RecordStore` version 2 declares create, get, bounded list, replace-if-revision, and delete-if-revision; the public `Records<T>` SDK performs typed model validation and forwards these calls.
- Records use framework-created UUIDs and monotonically increasing revisions. Paging is in ascending UUID order; consecutive pages are not a transaction snapshot.
- `MemoryStore` is ephemeral. Its schema-enforcing mode is optional, while the standalone reference server is permissive. It applies a 64 KiB JSON record limit and the shared 32-record page limit.
- Riesel's local/managed component factory is currently **infallible** (`Fn() -> S`). Opening a database is fallible. Provider startup, readiness, rollback, and preflight therefore need explicit design before implementation.
- A lost transport reply may follow a committed command. Session restart does not provide transparent handle rebinding or exactly-once execution.

References: [Storage contract](../../../packages/storage/contract/src/storage.rs), [typed Records SDK](../../../crates/riesel/src/records.rs), [component factory](../../../crates/riesel_app/src/component.rs), [runtime lifecycle](runtime-lifecycle.md).

## Proposed contract requirements

The first durable provider should meet these externally observable conditions, with any changes to the published `RecordStore` v2 contract explicitly reviewed:

1. **Identity and ordering:** stable UUID/model identity and revisions across reopening the same database; existing bounded cursor semantics retained, with no implied multi-page snapshot.
2. **Atomic writes:** one committed create/update/delete cannot leave a partial record. Concurrent update/delete with the same expected revision must allow at most one successful mutation.
3. **Validation:** model-aware writes must enforce the registered Rust model shape even for callers bypassing `Records<T>`. Unknown models, invalid fields and oversized payloads must be handled consistently with the agreed contract.
4. **Failure semantics:** not-found and stale-revision outcomes remain distinguishable. Database corruption, lock contention, incompatible stored metadata, I/O failures and transport failures must have documented, testable mappings; any expanded public error enumeration requires a separate compatibility decision.
5. **Lifecycle:** provider construction/initialization errors propagate without panic or false readiness. Definition and deployment inspection remain free of file creation/opening. Application-managed shutdown and crash handling preserve the documented session semantics.
6. **Configuration:** database location, initialization behavior, file ownership and access permissions are explicit. Production use does not silently fall back to MemoryStore.
7. **Recovery:** restart reconnects to the same persisted state where supported. Command retries after ambiguous transport failures are **not** automatic.

## Durability decision needed

"Persistent" must not conceal different failure guarantees:

| Failure | Minimum question for acceptance |
| --- | --- |
| Graceful application/provider shutdown | Are all acknowledged writes retained after reopening? |
| Abrupt provider process termination | Are previously acknowledged transactions retained and atomic? |
| OS crash or sudden power loss | Which acknowledged transactions are guaranteed retained under the configured sync policy and supported filesystem assumptions? |
| Full disk, permissions, locking, corrupted file | Which errors prevent readiness or cause a failed operation, and is prior data preserved? |

The specification must state whether power-loss durability is required by default and which filesystem guarantees are assumed. A process-crash test is **not** evidence of power-loss durability.

## Backend alternatives

Evaluate with the same contract tests and operational assumptions, without treating Rust-first as a reason to prefer any engine automatically.

| Candidate | Relevant strengths | Costs / uncertainties to evaluate |
| --- | --- | --- |
| SQLite via `rusqlite` | Mature transactional SQL and recovery tools, indexes and schema metadata, conditional revision updates | Bundled/native library policy, busy timeouts, WAL/checkpoint lifecycle, durability configuration, filesystem constraints |
| `redb` | Pure-Rust embedded transactional ordered key-value engine with crash-safety guarantees | Explicit application-managed metadata/schema design, operational tooling, file sharing, compatibility and error mapping |
| LMDB via `heed` | Mature single-writer transactional key-value model with memory-mapped reads | Native dependency, environment sizing/mapping policy, process access and operational recovery |

Primary documentation: [SQLite WAL](https://www.sqlite.org/wal.html), [SQLite synchronization](https://www.sqlite.org/pragma.html#pragma_synchronous), [SQLite network filesystem guidance](https://www.sqlite.org/useovernet.html), [rusqlite](https://docs.rs/rusqlite/latest/rusqlite/), [redb](https://docs.rs/redb/latest/redb/), [heed](https://docs.rs/heed/latest/heed/).

SQLite WAL with `synchronous=NORMAL` and `FULL` has different power-failure persistence semantics; do not select a mode by performance alone. WAL database files are not suitable for multi-host shared-network-filesystem access.

## Architecture decisions to resolve before implementation

- **Provider ownership:** one process owns the database file, versus documented safe multiple-process access. How do linked and managed placements both satisfy the capability contract?
- **Fallible startup:** use a narrow reusable fallible component-construction mechanism, a specialized provider factory, or another approach. No file I/O during preflight; errors must trigger runtime rollback and hosted readiness failure.
- **Schema compatibility:** persist authoritative model metadata or fingerprints; decide safe behavior on exact matches, incompatible changes and migrations. The first release may reject incompatible models and require explicit migration later, but must never silently reinterpret records.
- **Limits and errors:** decide whether the MemoryStore 64 KiB payload limit becomes provider parity, and whether current `StoreError::Failed` is sufficient for database-specific failures.
- **Durability configuration:** explicit sync mode and documented acknowledgement boundary, including limitations of the operating system and storage device.
- **Operation and recovery:** location ownership, permissions, backup/copy procedures, WAL/temporary files if applicable, lock contention, corruption handling, and observable diagnostics.

Record the decision in #24 with rationale, rejected alternatives and known limitations. Update this document to **accepted** only after explicit owner/maintainer approval.

## Verification gate for accepted implementation

Required test plan (refine with the selected backend):

- Run existing Storage contract tests against MemoryStore and the durable provider, including CRUD, model boundaries, paging, revision conflicts and invalid input.
- Reopen using the same configured path after graceful shutdown and after abrupt provider termination; verify acknowledged records, UUIDs and revisions.
- Race competing updates and deletes against the same observed revision; verify at most one winner and no corrupted state.
- Reject a mismatched persisted model or schema version safely at startup; do not create a partially initialized provider.
- Exercise invalid paths, permission/lock failures and failed initialization through local/managed lifecycle; verify rollback, startup diagnostics, and side-effect-free preflight.
- Verify transport cancellation or lost responses never triggers an implicit command retry. Distinguish what tests can prove from power-loss claims.
- Execute `cargo validate --check`, `cargo smoke`, and focused durable Storage integration tests on the accepted revision. Keep manual Windows checks as the pre-release gate.

## Exclusions and follow-up

No distributed replication, cloud database management, external public-network access, multi-user authorization, offline device synchronization, automatic retry/exactly-once execution or generic migration framework is approved by this proposal.

After acceptance, split implementation by genuinely independent deliverables (provider; runtime configuration/fallible startup if required; parity and restart test harness; Todo R1 integration). Parallel agent work is appropriate only after shared contracts and dependencies are fixed.
