# Stage 6 — Durable Record Storage architecture proposal

**Status: proposed; Gate B not accepted.** Decision owner and evidence: [Specification Issue #24](https://github.com/aschenrot/riesel/issues/24). Research: [brief](durable-record-storage-research.md), [candidate evaluation](durable-record-storage-evaluation.md). This document is a reviewable recommendation, not implementation authorization.

## Proposed first delivery

Introduce an explicit **SQLite-backed `RecordStore` provider** using a supported `rusqlite` version. It would be a reusable provider under `packages/storage/`, alongside—not replacing—MemoryStore, with `RecordStore` v2 and `Records<T>` preserved.

The initial supported topology would be a **single host, local database file, one logical owning provider per file**, used as a linked or runtime-managed component. Other Riesel application clients access records through its capability, not by independently modifying the file. A separate PostgreSQL-backed provider is a distinct later option; server deployment is not silently treated as embedded storage.

**Rationale:** conditional SQLite preference is supported by established transactional semantics, documented sync behavior, on-disk inspection and upgrade tooling. redb remains a credible pure-Rust fallback. The current `RecordStore` has no demonstrated need for a more complex LSM or distributed engine. This is not a performance conclusion. See the [dated comparison](durable-record-storage-evaluation.md).

## Proposed observable guarantees

| Behavior | Proposed contract |
| --- | --- |
| Lifecycle | File is not opened or created during schema inspection, provider registration or preflight. Opening and metadata validation occur during provider startup; failures reject startup without false readiness or panic. |
| Persistence | A successfully acknowledged mutation survives a clean provider/application restart and an abrupt provider **process** termination when the configured engine commit has completed. |
| Durability policy | SQLite WAL with `synchronous=FULL` as the initial durability-oriented default, on a documented supported local filesystem. Crash and power-loss guarantees remain limited by the OS, storage device and correct synchronization; test what can be tested. |
| Atomic writes | Create/replace/delete is transactional. Update/delete only succeeds if model, UUID and the observed revision still match. Two concurrent operations on the same revision cannot both succeed. |
| Identity and ordering | Generated UUIDs and revisions survive reopening. Model + UUID keys preserve bounded UUID-order cursor pagination; no multi-page snapshot is promised. |
| Validation | A durable provider enforces **authoritative registered model definitions**, including raw `RecordStore` callers, not just typed `Records<T>` callers. Unknown models and incompatible model metadata fail safely. |
| Schema evolution | Persist record format/version and model definitions. First release requires exact model compatibility by default; unsupported changes fail **before readiness** without rewriting data. Automatic migrations are deferred. |
| Error semantics | Preserve existing v2 `NotFound`, `RevisionConflict`, `InvalidPageSize`, `Failed` and `Transport` behavior. Document recoverable lock, read-only, disk-full and corruption failures; expand typed errors only through a separately accepted versioned contract. |
| Recovery | No implicit retry of ambiguous writes, no transparent rebinding across provider failure, and no fallback to MemoryStore. |

The above are **proposed** acceptance boundaries. They are not claims about an already built provider.

### Important representation details

- SQLite `INTEGER` is signed 64-bit, while `StoredRecord.revision` is `u64`. Preserve the full public revision domain (e.g. fixed eight-byte big-endian encoding) or explicitly justify any limitation in a versioned contract. The existing MemoryStore uses checked revision increment and returns an error on overflow.
- Store UUIDs in an order-preserving representation (e.g. sixteen-byte canonical UUID bytes); test that SQLite ordering matches the existing Rust implementation and page cursor semantics.
- Enforce the current 32-record page bound and evaluate the existing MemoryStore 64 KiB payload bound as an explicit provider parity rule; do not silently increase RPC frames.
- Zero affected rows on conditional update/delete must be classified as missing versus stale revision **without a race**. Do the distinction within the relevant transaction, not by an unprotected subsequent read.
- A committed SQLite transaction is not equivalent to a completed network/RPC call from the client's perspective; a lost response never authorizes automatic replay.

## Architecture interfaces that require explicit decisions

### A. Fallible provider initialization

Current `ComponentDefinition<S>` accepts only an infallible `Fn() -> S`, although `LocalProviderFactory::create` and the hosted server return `Result`. The existing startup code can propagate a failing factory and roll back earlier providers; the **component constructor** cannot express such a failure yet.

**Preferred direction to review:** add an opt-in fallible component factory, preserving `ComponentDefinition::new` compatibility and using the existing startup/rollback machinery. An alternative is a Storage-specific `LocalProviderFactory`, but that would duplicate generic local/managed construction semantics. Confirm this trade-off before changing the core runtime.

The managed service must initialize and validate its database **before** emitting `RIESEL_READY`. Fatal initialization errors should be visible in service startup diagnostics; readiness must not be emitted after a failed open. Preflight remains pure.

### B. Authoritative model metadata across placements

`AppSchema` already carries `models` and `model_bindings`; `Application::development()` passes cloned models to the MemoryStore development provider. In managed mode, the current provider readiness/selection channel contains **capability metadata, not model definitions**. A durable provider cannot assume the client SDK has validated every raw RPC request.

**Required design decision:** provide the model set for each selected Storage instance by a **deliberate, verified startup/configuration channel** in local and managed modes. Compare (i) application SDK generation of explicit provider configuration and (ii) an explicit model manifest bound to the managed provider instance. Do not add silent global discovery, a second provider resolver or an unauthenticated schema-mutation RPC merely for convenience.

Persist and compare model metadata against existing data. The planned error must identify the incompatible model without modifying the database. Resolve app/provider sharing and model-set changes before implementation.

### C. Path ownership and safe operations

Require an explicit database path/configuration bound to the chosen provider instance; avoid fixed ports, implicit application-wide defaults and unrelated current-working-directory paths. Define new-file creation versus open-existing behavior, filesystem permissions, lock contention, corrupted-file policy, and safe backup/restore procedures (including WAL-related files). Do not permit network-filesystem sharing as a supported first-release topology without independent evidence.

## Focused engine research evidence

On 2026-11-19, isolated Rust 1.100.0 probes compiled and tested **rusqlite 0.43.0** and **redb 5.0.0** without adding either to the Riesel workspace. [Research PR #29](https://github.com/aschenrot/riesel/pull/29), [5/5 engine tests](https://github.com/aschenrot/riesel/actions/runs/44568798067), and [standard Linux validation](https://github.com/aschenrot/riesel/actions/runs/44568798124) passed.

The tests exercised SQLite binary UUID ordering, cursor pagination, full `u64` BLOB revisions, stale CAS and two concurrent writers; redb ordered keys, `u64` revisions and two competing write transactions; and for **both engines** reopen after a commit followed by abrupt child process exit without closing the handle. This is narrow **engine feasibility evidence**, not integrated Riesel provider readiness, model registration, cross-process shared-file policy, corrupt-database recovery, or OS/power-loss verification. Neither candidate was disproven by these tests.

## Evidence still required before accepting Gate B

1. **Riesel integration:** verify chosen engine's model metadata, bounded pagination, errors and durability policy in the actual `RecordStore` adapter, including local and managed placements. The isolated tests do not establish compatibility with generated RPC or the Riesel lifecycle.
2. **Failure boundaries:** test relevant lock/busy, disk-full/invalid-path, corrupted-file and failed-startup outcomes, plus documented backup/upgrade policy. Preserve distinct process-crash and OS/power-loss claims.
3. **Runtime design:** review and verify a minimal fallible factory path through local startup, managed readiness failure and rollback, without side effects in preflight.
4. **Schema/channel design:** accept how model definitions reach the correct provider in both placements without duplicating developer-facing declarations.
5. **Platform policy:** record exact dependency versions, licensing and build requirements; Rust 1.100.0 Linux builds passed, but Windows remains a manual pre-release check. Confirm the final provider rather than extrapolating from standalone tests.
6. **Process ownership:** decide whether another process may directly open the file, and establish the selected engine's locking/recovery rules for that topology.

## Decision gate and implementation decomposition

**Gate B is not complete.** A maintainer must explicitly approve or revise the topology, backend, durability guarantee, model channel, error semantics and startup interface, with evidence linked in Issue #24. An unacceptable unknown permits a documented deferral or a return to alternative candidates.

Only then create independent implementation Issues, for example: (a) fallible factory support if approved, (b) durable provider/storage configuration and model policy, (c) contract/parity, concurrency and restart tests, and (d) Todo R1 integration after the provider is available. Agents may work in parallel against accepted interfaces; separate the tasks only where changes and tests are independently reviewable.

Excluded: distributed replication, automatic remote synchronization, public unauthenticated access, transparent exactly-once/retry, multi-user authorization and a generic migration framework.
