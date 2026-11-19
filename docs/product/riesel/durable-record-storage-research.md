# Stage 6 — Durable Record Storage research brief

**Status: research brief; architecture not selected.** [Specification Issue #24](https://github.com/aschenrot/riesel/issues/24) owns the investigation and final decision. This document defines what to research, not which database or architecture to implement.

## Problem and verified baseline

Riesel needs a reusable persistent `RecordStore` provider to meet Todo R1's across-restart requirements. The first deployment model (embedded versus separately hosted) is not yet accepted.

- `RecordStore` v2 defines create/get/list-page/update/delete, record IDs, caller-observed revision checks, and UUID-ordered bounded pages **without** a multi-page snapshot.
- The typed `Records<T>` facade validates typed inputs and outputs. `MemoryStore` is ephemeral and validates raw records against model metadata only when configured with models. Its 64 KiB record limit is provider-specific; the contract's page limit is 32.
- Managed-provider loss invalidates the application session; a lost response can follow a committed write. Neither automatic command retry nor exactly-once execution is provided.
- Existing component construction is infallible (`Fn() -> S`). Durable backend initialization is fallible; startup errors and readiness must be designed without file I/O during side-effect-free preflight.

References: [Storage contract](../../../packages/storage/contract/src/storage.rs), [typed Records SDK](../../../crates/riesel/src/records.rs), [component factory](../../../crates/riesel_app/src/component.rs), [runtime lifecycle](runtime-lifecycle.md), [Todo product](../todo/todo.md).

## Requirements to confirm before selecting a backend

| Area | Known boundary | Decision or evidence needed |
| --- | --- | --- |
| Record contract | Preserve v2 CRUD, IDs, revisions, CAS and bounded UUID paging | Justify any versioned contract change rather than assuming one |
| Persistence | Todo tasks must survive application and provider restart | Define acknowledged-write guarantees for graceful exit, process crash, OS crash and power loss, with filesystem/sync assumptions |
| Concurrency | Stale revisions must fail, not overwrite newer state | Define atomicity, concurrent-writer/process ownership, locking and timeout behavior |
| Validation | Typed calls validate model shapes; raw memory calls may be permissive | Decide how a durable provider obtains authoritative model metadata and rejects incompatible or invalid data |
| Compatibility | No accepted durable on-disk format or migration policy exists | Define data-format/version checks, incompatible model startup behavior, migration limits and safe backup/recovery |
| Lifecycle | Local/managed providers share capability contracts; preflight is side-effect-free | Define fallible initialization, database-path configuration, readiness, error propagation and rollback |
| Failure semantics | `StoreError` separates NotFound, RevisionConflict, InvalidPageSize, Failed and Transport | Determine whether corruption, busy/lock, disk-full and schema mismatch need new typed errors; avoid silent retries |
| Deployment | Riesel supports local and managed placements; external hosts are not automatically supervised | Evaluate embedded single-host ownership against a separately managed server and document operational cost |

These are **questions to settle**, not a claim that every proposed guarantee is already accepted. A process-kill restart test alone cannot establish power-loss durability.

## Research method

### 1. Discover and screen candidates

- Capture the search date, documentation/release sources, candidate names and deployment assumptions. Cover materially distinct engine architectures and a server-backed comparator. Add credible options found during the search; do not treat the examples below as a ranked shortlist.
- Separate **database engines** from Rust access layers or bindings. For example, `rusqlite` is a SQLite binding and `heed` is a binding for LMDB.
- Define must-pass requirements before excluding candidates. Screen on transactions/revision safety, a documented durability model, viable Rust integration, applicable deployment/ownership constraints, upgrade/data-safety posture and licensing. Mark unresolved evidence **unknown**, not pass or fail.
- Record concise inclusion/exclusion reasons. Broad catalogue coverage does not require benchmarking or prototyping every engine.

| Illustrative candidate family | Examples to screen (unranked) |
| --- | --- |
| Embedded relational SQL | SQLite; Rust SQLite-compatible engines such as Turso |
| Transactional B-tree / ordered KV | redb; LMDB; libMDBX |
| Log-structured KV | Fjall; RocksDB |
| Other embedded document/storage engines | SurrealDB/SurrealKV, Persy, sled where applicable |
| Server-backed comparator | PostgreSQL |

These groupings are for discovery, not mutually exclusive technical classifications or an endorsement of a particular product.

### 2. Compare viable approaches consistently

| Evaluation dimension | Minimum comparable evidence |
| --- | --- |
| **Required: atomic revision semantics** | Transactions/locking and a credible CAS implementation; concurrent-writer behavior |
| **Required: durable acknowledgment and recovery** | Commit/sync guarantee by failure mode; corruption and recovery limits |
| **Required: runtime integration** | Rust API, fallible startup/readiness, local/managed deployment, cancellation and error mapping |
| **Required: compatibility/data safety** | Format/model version behavior, upgrade, backup and incompatible-schema handling |
| Operations and maintenance | Supported platforms/toolchains, packaging, licensing, release/maintenance evidence, tooling and storage ownership |
| Workload fit | Expected data volume, contention, paging, compaction/checkpoint or file-growth costs |

Separate requirements from preferences; a pure-Rust implementation, popularity, broad query features or benchmark results are **not** automatic inclusion or exclusion rules. Declare hard thresholds before screening. Compare candidates under equivalent durability settings; where reliable evidence is absent, record that limitation rather than inventing scores.

### 3. Resolve only decision-critical unknowns

- Use primary documentation and current maintenance/release evidence first. Identify statements as vendor/project claims or independently tested results.
- Prototype the smallest operation or failure scenario needed to resolve a material uncertainty; make test inputs, versions, sync settings and results reproducible.
- Perform deeper comparison on materially different **viable** approaches rather than every catalogued engine. Stop expanding the search when additional candidates are unlikely to change the decision, documenting coverage and the reason. If evidence is insufficient, **defer** the decision instead of pretending a winner exists.

### 4. Record an auditable decision

The output is a dated comparison table showing which candidates meet, fail or remain unknown on each necessary requirement, linked evidence, relevant trade-offs, and explicit reasons for exclusions. Recommend an approach **or defer** with the missing evidence stated. Record any necessary published-contract or lifecycle change separately from the engine selection.

Starting references: [SQLite durability](https://www.sqlite.org/pragma.html#pragma_synchronous), [SQLite WAL](https://www.sqlite.org/wal.html), [redb](https://docs.rs/redb/latest/redb/), [Fjall](https://docs.rs/fjall/latest/fjall/), [heed](https://docs.rs/heed/latest/heed/), [RocksDB](https://github.com/facebook/rocksdb/wiki/Transactions), [libMDBX](https://github.com/erthink/libmdbx), [Turso](https://github.com/tursodatabase/turso), [SurrealDB](https://surrealdb.com/docs), [PostgreSQL](https://www.postgresql.org/docs/).

## Two acceptance gates

### Gate A — research brief (this PR)

- [ ] Existing behavior is distinguished from requested outcomes and unresolved design questions.
- [ ] The candidate-discovery method, eligibility criteria, comparison dimensions, evidence standards and bounded stopping rule are reviewable.
- [ ] No backend, deployment topology, new core factory API or new durability guarantee is implicitly accepted.

Approving and merging this **research brief** allows the investigation to proceed; it does not complete Issue #24 or authorize implementation.

### Gate B — architecture decision (Issue #24, subsequent reviewed change)

- [ ] A dated comparison with documented exclusions and unresolved evidence is available.
- [ ] Durability, revisions, model validation, error semantics, upgrade, provider ownership and fallible initialization have explicit, testable decisions.
- [ ] Relevant alternative approaches and remaining risks are recorded; if a decision is deferred, the outstanding research is stated.
- [ ] The product/architecture documents reflect the accepted contract, with maintainer decision and evidence linked from #24.
- [ ] **Only after acceptance**, independent implementation Issues and dependencies are created.

## Implementation verification to plan after Gate B

The accepted architecture must define how to test contract parity, atomic concurrent updates, clean/abrupt restart, persisted model mismatch, invalid paths/locking, local and managed startup failure, and lack of automatic retry after ambiguous responses. Distinguish process-crash evidence from OS/power-loss guarantees. Use `cargo validate --check`, `cargo smoke`, relevant integration tests, and manual Windows pre-release validation as applicable.

**Out of scope for this investigation:** distributed replication, cloud orchestration, public unauthenticated remote access, multi-user authorization, offline sync, exactly-once command semantics and a general-purpose migration framework. A new general Riesel abstraction is not approved without a separate justified contract decision.
