# Stage 6 — Durable Record Storage architecture proposal

**Status: proposed; Gate B not accepted.** Decision owner and evidence: [Specification Issue #24](https://github.com/aschenrot/riesel/issues/24). Research: [brief](durable-record-storage-research.md), [candidate evaluation](durable-record-storage-evaluation.md). This document is a reviewable recommendation, not implementation authorization.

## Proposed first delivery

Introduce an explicit **SQLite-backed `RecordStore` provider** using a supported `rusqlite` version. It would be a reusable provider under `packages/storage/`, alongside—not replacing—MemoryStore, with `RecordStore` v2 and `Records<T>` preserved.

The proposed topology is **one host, a local database file, and one logical owning provider per file**. Other application clients access records through the capability rather than modifying the file themselves. **Which placements belong to the first delivery is unresolved:** linked-only can satisfy Todo R1, while managed parity additionally requires a reliable model-configuration channel. A separate PostgreSQL-backed provider is a later deployment option, not automatically equivalent to embedded storage.

**Rationale:** conditional SQLite preference is supported by established transactional semantics, documented sync behavior, on-disk inspection and upgrade tooling. redb remains a credible pure-Rust fallback. The current `RecordStore` has no demonstrated need for a more complex LSM or distributed engine. This is not a performance conclusion. See the [dated comparison](durable-record-storage-evaluation.md).

## Delivery scope to accept

| Option | Benefit | Added obligation |
| --- | --- | --- |
| **Linked provider first** | Delivers Todo R1 persistence and the reusable `RecordStore` capability with a smaller integration surface | Explicitly document managed Storage as unsupported until independently accepted and tested |
| **Linked + managed parity immediately** | Exercises the same durable provider in both existing runtime placements | Must resolve managed model metadata/configuration, readiness diagnostics, file ownership and crash/recovery validation before the first release |

**Provisional scope preference:** linked provider first, unless managed parity is an accepted Stage 6 exit criterion. This is a sequencing proposal, not permission to claim complete production Storage or to make an ephemeral managed provider persistent by assumption. Both options require provider-side model validation and fallible startup; the second requires an additional managed configuration/identity decision.

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

For the linked provider, initialization must fail through the local factory result; preflight remains pure. **If managed placement is included**, the service must initialize and validate its database before emitting `RIESEL_READY`; fatal startup causes must remain diagnosable. Do not claim that the existing readiness frame already transmits model definitions.

### B. Authoritative model metadata across placements

`AppSchema` already carries `models` and `model_bindings`; `Application::development()` passes cloned models to the MemoryStore development provider. In managed mode, the current provider readiness/selection channel contains **capability metadata, not model definitions**. A durable provider cannot assume the client SDK has validated every raw RPC request.

**Required for the first delivery:** provide model definitions to the selected linked provider from authoritative application declarations without duplicating hand-maintained schemas; the existing `Application::development()` pattern shows one possible path. **Required before managed support is claimed:** design and test a deliberate startup/configuration channel for that instance, comparing application-generated configuration with an explicit model manifest. Do not add silent global discovery, a second resolver or an unauthenticated schema-mutation RPC merely for convenience.

Persist and compare model metadata against existing data. The planned error must identify the incompatible model without modifying the database. Resolve app/provider sharing and model-set changes before implementation.

### C. Path ownership and safe operations

Require an explicit database path/configuration bound to the chosen provider instance; avoid fixed ports, implicit application-wide defaults and unrelated current-working-directory paths. Define new-file creation versus open-existing behavior, filesystem permissions, lock contention, corrupted-file policy, and safe backup/restore procedures (including WAL-related files). Do not permit network-filesystem sharing as a supported first-release topology without independent evidence.

## Focused engine research evidence

On 2026-11-19, isolated Rust 1.100.0 probes compiled and tested **rusqlite 0.43.0** and **redb 5.0.0** without adding either to the Riesel workspace. [Research PR #29](https://github.com/aschenrot/riesel/pull/29) and the [corrected research CI run](https://github.com/aschenrot/riesel/actions/runs/44569551231) passed **5/5** engine tests.

The tests exercised SQLite binary UUID ordering, cursor pagination, full `u64` BLOB revisions, stale CAS and two concurrent writers; redb ordered keys, `u64` revisions and two competing write transactions; and **both engines** reopening an acknowledged record after child-process exit while the storage handle remains open. An initial probe incorrectly dropped the handle before exit; the corrected code exits **inside** the helper before Rust destructors run. The [Issue #24 evidence comment](https://github.com/aschenrot/riesel/issues/24#issuecomment-7015633350) records the correction.

This is narrow **engine feasibility evidence**, not integrated Riesel provider readiness, model registration, cross-process shared-file policy, corrupt-database recovery, or OS/power-loss verification. Neither candidate was disproven by these tests.

## Evidence still required before accepting Gate B

1. **Riesel integration:** verify chosen engine's model metadata, bounded pagination, errors and durability policy in the actual `RecordStore` adapter, including local and managed placements. The isolated tests do not establish compatibility with generated RPC or the Riesel lifecycle.
2. **Failure boundaries:** test relevant lock/busy, disk-full/invalid-path, corrupted-file and failed-startup outcomes, plus documented backup/upgrade policy. Preserve distinct process-crash and OS/power-loss claims.
3. **Runtime design:** review and verify a minimal fallible factory path through local startup, managed readiness failure and rollback, without side effects in preflight.
4. **Schema/channel design:** accept model propagation and validation for the first supported placement; resolve the managed manifest/registration channel before managed parity is delivered.
5. **Platform policy:** record exact dependency versions, licensing and build requirements; Rust 1.100.0 Linux engine builds passed, but Windows remains a manual pre-release check. Confirm the final provider rather than extrapolating from standalone tests.
6. **Process ownership:** decide whether another process may directly open the file, and establish the selected engine's locking/recovery rules for that topology.

## Decision gate and implementation decomposition

**Gate B is not complete.** A maintainer must explicitly approve or revise the first-delivery placement scope, file ownership, backend, durability guarantee, model channel, error semantics and startup interface, with evidence linked in Issue #24. An unacceptable unknown permits a documented deferral or a return to alternative candidates.

Only then create independent implementation Issues, for example: (a) fallible factory support if approved, (b) durable provider/storage configuration and model policy, (c) contract/parity, concurrency and restart tests, and (d) Todo R1 integration after the provider is available. Agents may work in parallel against accepted interfaces; separate the tasks only where changes and tests are independently reviewable.

Excluded: distributed replication, automatic remote synchronization, public unauthenticated access, transparent exactly-once/retry, multi-user authorization and a generic migration framework.
