# Stage 6 — Durable Record Storage: proposed specification

**Status: proposal, not accepted.** Decision and evidence: [Specification Issue #24](https://github.com/aschenrot/riesel/issues/24). This document is for review and does not authorize an implementation or commit Riesel to a database engine.

## Outcome and scope

Provide a durable `RecordStore` implementation usable by Riesel applications without changing their typed `Records<T>` domain code. The first consumer is Todo R1; the provider must remain reusable by other applications.

An **embedded, single-host provider is an initial scope hypothesis**, not an accepted technical constraint. The investigation must first justify that deployment boundary against a server-backed alternative. Multi-process ownership and durability defaults remain unresolved decisions.

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

## Research plan and decision evidence

**Status: required investigation, not completed research.** The names below are an initial discovery inventory, not a shortlist, ranking or backend recommendation. A candidate must not be selected or dismissed just because it is familiar, Rust-native, SQL-based or already mentioned here.

### 1. Discover and screen alternatives

- Define the deployment question first: embedded provider versus separately operated database service; distinguish engine choice from Rust access libraries (such as `rusqlite`, `SQLx`, or `heed`).
- Identify materially different engine families using primary technical documentation and repository release/maintenance records. Record the search scope and date, evidence links and reasons for inclusion/exclusion; add credible candidates found during research.
- Screen against **necessary outcomes** rather than vendor popularity: atomic revision-checked mutation, documented durability under chosen failure modes, supported Rust integration, compatible deployment/ownership and a maintenance/licensing posture appropriate to Riesel. A condition not yet evidenced is **unknown**, not automatically a pass.
- If embedded-only is adopted, record why a server-backed alternative does not fit the initial delivery. Do not evaluate server operation as though it had zero deployment cost.

Initial inventory to screen (illustrative, subject to expansion):

| Engine family | Examples to investigate | Distinct question |
| --- | --- | --- |
| Embedded relational SQL | SQLite; Rust-native SQLite-compatible engines such as Turso | Transaction semantics, migration tooling, operational dependence, compatibility/maturity |
| Pure-Rust transactional KV | redb | Ordered CRUD, MVCC, process ownership and durability |
| Log-structured KV | Fjall; RocksDB | Write amplification, compaction, atomic transactions and deployment cost |
| Memory-mapped transactional KV | LMDB; libMDBX | Single-writer/process model, file mapping, error/recovery semantics |
| Other embedded approaches | SurrealKV/SurrealDB, Persy, sled (where relevant) | Whether their different API or maturity provides a concrete benefit |
| Server-backed comparison | PostgreSQL (as a deployment-class comparator) | Whether shared server access or operations justify a non-embedded first provider |

A longlist item is **not** a promise to prototype it. Research should first filter clearly unsuitable approaches with traceable reasons, then deep-dive only into viable, materially different contenders.

### 2. Compare using declared criteria

Apply the same questions and explicit pass/fail/unknown evidence standards to each remaining candidate:

| Dimension | Evidence needed |
| --- | --- |
| **Required: atomicity and revision safety** | Documented transactions/locking plus a minimal competing-update proof where documentation is insufficient |
| **Required: durability and recovery** | Exact commit-acknowledgement/sync behavior, process-crash versus power-loss boundaries, recovery limitations and primary documentation |
| **Required: integration feasibility** | Rust API, fallible startup/readiness, owned database path/process, local/managed lifecycle and predictable error translation |
| **Required: compatibility/data safety** | Persisted metadata approach, schema-change detection, migration/backup options and documented corruption behavior |
| Operations and maintenance | Version/release evidence, licensing, platform/toolchain support, diagnostic tooling and upgrade compatibility |
| Resource/performance fit | Representative workload expectations, read/write concurrency, file growth, compaction/checkpoint needs and bounded paging |

Declare any hard exclusion thresholds **before** using them to discard candidates. Separate must-pass conditions from preferences; Rust-only implementation, benchmark speed, feature breadth and popularity are not implicit hard requirements. Do not manufacture numerical scores or weights. If benchmarking is needed, compare equal transaction sizes, acknowledgement durability settings, contention and dataset conditions.

### 3. Evaluate evidence and decide

- Record a comparison matrix with candidate, status (**meets / does not meet / unknown**), evidence links and date, assumptions, risks, and explicit rejection reasons. Distinguish engine claims from independently verified behavior.
- Use a focused prototype or reproducible test when a decision-critical behavior cannot be resolved from reliable documentation. A prototype tests the uncertainty; it is not automatically production implementation.
- Produce a reasoned recommendation **or defer the choice** when important evidence remains missing. Record why credible alternatives were not selected, remaining trade-offs, and any accepted change to the required contract or deployment boundary.
- The specification Issue (#24) remains open until a maintainer explicitly records the decision and its supporting evidence. This document may become an accepted architecture contract only after that gate; passing CI or adding this plan is not approval.

### Research acceptance criteria

- [ ] Deployment scope, system-level requirements and candidate eligibility criteria are recorded before ranking or elimination.
- [ ] The candidate search covers relevant engine families; material exclusions and omissions are justified with dated evidence.
- [ ] Viable alternatives are compared against the same criteria, with unknowns and differing durability guarantees explicit.
- [ ] Decision-critical claims are supported by primary sources and, where needed, reproducible focused tests.
- [ ] The resulting recommendation (or deferral), rationale and outstanding risks are documented and reviewed.
- [ ] No implementation Issue is authorized solely by completion of this research plan.

Primary documentation starting points: [SQLite WAL](https://www.sqlite.org/wal.html), [SQLite synchronization](https://www.sqlite.org/pragma.html#pragma_synchronous), [SQLite network filesystem guidance](https://www.sqlite.org/useovernet.html), [redb](https://docs.rs/redb/latest/redb/), [Fjall](https://docs.rs/fjall/latest/fjall/), [heed](https://docs.rs/heed/latest/heed/), [RocksDB transactions](https://github.com/facebook/rocksdb/wiki/Transactions), [libMDBX](https://github.com/erthink/libmdbx), [Turso](https://github.com/tursodatabase/turso), [SurrealDB](https://surrealdb.com/docs), and [PostgreSQL](https://www.postgresql.org/docs/).

## Architecture decisions to resolve before implementation

- **Provider ownership:** one process owns the database file, versus documented safe multiple-process access. How do linked and managed placements both satisfy the capability contract?
- **Fallible startup:** use a narrow reusable fallible component-construction mechanism, a specialized provider factory, or another approach. No file I/O during preflight; errors must trigger runtime rollback and hosted readiness failure.
- **Schema compatibility:** persist authoritative model metadata or fingerprints; decide safe behavior on exact matches, incompatible changes and migrations. The first release may reject incompatible models and require explicit migration later, but must never silently reinterpret records.
- **Limits and errors:** decide whether the MemoryStore 64 KiB payload limit becomes provider parity, and whether current `StoreError::Failed` is sufficient for database-specific failures.
- **Durability configuration:** explicit sync mode and documented acknowledgement boundary, including limitations of the operating system and storage device.
- **Operation and recovery:** location ownership, permissions, backup/copy procedures, WAL/temporary files if applicable, lock contention, corruption handling, and observable diagnostics.

Record the decision in #24 with rationale, evidence, rejected alternatives and known limitations. Update this document to **accepted** only after explicit owner/maintainer approval.

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
