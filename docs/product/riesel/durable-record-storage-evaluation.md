# Stage 6 — Durable Record Storage: candidate evaluation

**Research date:** 2026-11-19. **Status:** evidence review and conditional recommendation; **not an accepted architecture decision**. Investigation: [Issue #24](https://github.com/aschenrot/riesel/issues/24). Method and acceptance boundaries: [research brief](durable-record-storage-research.md).

## Decision context and evidence limits

Riesel's current `RecordStore` v2 needs model-scoped CRUD, UUID-ordered pagination, atomic expected-revision update/delete and durability across restarts. Todo R1 needs an embedded or otherwise deployable backend without changing domain operations. Any engine must integrate with Riesel's local/managed lifecycle and side-effect-free preflight.

**Evidence collected:** upstream engine/binding documentation and release/compatibility information, dated above. **Not performed:** a Riesel integration prototype, comparable workload benchmarks, process-kill tests or power-failure tests. Upstream transaction/durability claims are not independent proof of Riesel's configuration or deployment. Unknowns remain unknown.

The initial deployment hypothesis is **one application/provider-owned local database on a supported single host**, rather than an independently operated server. It is a question for the architecture gate, not a constraint used to declare PostgreSQL intrinsically unsuitable.

### Screening criteria

1. **Mandatory for this provider:** transactions or equivalent atomic revision-aware mutation; durable commit options and documented crash behavior; compatible Rust integration; stable-enough on-disk data handling and error reporting; explicit local/managed ownership and startup semantics.
2. **Must be established in the Riesel integration:** model registration/validation for raw capability callers, migration or safe schema rejection, configuration and permissions, startup/readiness failure propagation, restart behavior and matching `RecordStore` errors.
3. **Preferences, not hard exclusions:** Rust-only engine, SQL versus KV, development tool simplicity, compact native build, compression and high benchmark throughput. No published throughput/size target currently justifies a performance-based exclusion.

All candidates require integration work. A candidate with documented transactions is **potentially viable**, not automatically verified against the Riesel contract.

## Candidate discovery and screening

| Candidate | Documented fit | Material cost or unknown | Screening outcome |
| --- | --- | --- | --- |
| **SQLite** via `rusqlite` | ACID transactions; conditional `UPDATE ... WHERE revision=?`; established tooling and schema versions; WAL/synchronous controls | Bundled C dependency, single active writer per file, WAL + sync policy, local-file locking and checkpoint/backup procedures | **Deep comparison** |
| **redb 5.0** | Pure-Rust, ACID ordered KV with MVCC and documented immediate-durability commit; single-/multi-process concurrency modes | Application-owned schema metadata and on-disk model rules; recent major release (2026-11-18) warrants targeted compatibility testing | **Deep comparison** |
| **Fjall 3.1** | Rust LSM, ordered range iteration, optional serializable transactions, explicit persist modes | Background compaction, journal/persist policy, workload tuning; no established need for LSM advantages in current record contract | **Viable; conditional follow-up** |
| **LMDB** via `heed` | Mature mmap transaction engine, ordered range iteration, concurrent reads/single-writer model | Native dependency, environment/map sizing, memory-map operational constraints, model metadata owned by Riesel | **Viable; conditional follow-up** |
| **libMDBX** | LMDB-family transactional mmap engine, multiple sync modes and Rust bindings | Extra binding/FFI and durability-mode verification; not yet an evidenced advantage over above for this scope | **Screened; retain as alternative** |
| **RocksDB** | Embedded LSM transactions and mature operational tuning | C++/native build, compaction, configuration and backup costs; no evidence current workload needs those trade-offs | **Screened; reconsider for LSM-specific needs** |
| **Turso** | Active Rust SQL engine targeting SQLite compatibility and file interoperability | Upstream compatibility matrix explicitly lists partially supported SQL/C API; validate required feature/failure semantics before depending on it | **Defer first-provider use pending tests** |
| **SurrealDB / SurrealKV** | Rust embedded durable engine and richer queries | Adds a query/document/server abstraction not needed by current five-operation capability | **Screened; reconsider for broader query requirements** |
| **Persy** | Rust transactional record/index engine with WAL crash-recovery claims | On-disk format operations, ecosystem support and lifecycle fit not yet evidenced to the same level | **Unknown; further research only if leading options fail** |
| **sled** | Embedded Rust KV and transactions | Upstream README still warns of beta status and changing disk format/migrations | **Defer for durability-first deployment** |
| **PostgreSQL** | Server-backed transactional SQL and operational ecosystem | Independently operated service and connection/identity/recovery responsibilities | **Separate server-provider option**, not an embedded competitor |

Evidence: SQLite [appropriate uses](https://www.sqlite.org/whentouse.html), [WAL](https://www.sqlite.org/wal.html), [synchronous modes](https://www.sqlite.org/pragma.html#pragma_synchronous), [Rust bindings](https://docs.rs/crate/rusqlite/latest); redb [API and concurrency](https://docs.rs/redb/latest/redb/), [durability](https://docs.rs/redb/latest/redb/enum.Durability.html), [current changelog](https://docs.rs/crate/redb/latest/source/CHANGELOG.md); Fjall [transactions and persistence](https://docs.rs/fjall/latest/fjall/); LMDB [heed](https://docs.rs/heed/latest/heed/) and [environment/mapping](https://docs.rs/heed/latest/heed/struct.Env.html); [libMDBX durability modes](https://libmdbx.dqdkfa.ru/docs/en/getting-started/durability-modes.html); [RocksDB Rust binding](https://docs.rs/crate/rocksdb/latest); [Turso compatibility](https://github.com/tursodatabase/turso/blob/main/COMPAT.md); [SurrealDB embedded](https://surrealdb.com/docs/reference/rust/embedding); [Persy](https://docs.rs/persy/latest/persy/); [sled upstream caveats](https://github.com/spacejam/sled/blob/main/README.md).

**No benchmarking-based eliminations.** The screening distinguishes an appropriate first delivery from a universally superior engine. Fjall, LMDB, libMDBX and RocksDB are not judged incorrect; the current workload does not demonstrate the need to absorb their particular integration costs. The two main comparators are SQLite and redb because they supply different, sufficiently documented ways of meeting the narrow current contract.

## Primary comparison: SQLite versus redb

| Decision dimension | SQLite via `rusqlite` | redb |
| --- | --- | --- |
| Atomic CAS | Single SQL statement conditioned on expected revision; use affected-row count and a transactional existence check to distinguish conflict from missing record | One ordered key/value table, read/check/write under an atomic write transaction |
| Durable acknowledgment | WAL `synchronous=FULL` syncs WAL for each commit; `NORMAL` can lose recently acknowledged writes after OS/power failure | `Durability::Immediate` documented as persistent once transaction commit returns; `None` is not |
| Read and paging | Model + UUID indexed table, bounded `ORDER BY id`; no cross-page snapshot promise | Composite model/UUID key and ordered range iteration; no cross-page snapshot promise |
| Schema/upgrade handling | Relational tables and explicit DB schema version metadata/migration mechanisms | Riesel must manage model definitions, data-format identity and upgrade policy within KV tables |
| Process ownership | Multiple connections/processes on a local disk with one active writer; WAL not a multi-host network-FS solution | 5.0 documents `ExclusiveWriter` default plus `SingleWriter`/`MultiWriter` modes with platform-specific file locks; mode is an explicit architecture choice |
| Packaging | SQLite C library via optional bundled build; `rusqlite` v0.43 lists Rust 1.89 MSRV | Pure Rust engine; v5.0 was released on 2026-11-18, so verify migration and Rust 1.100 compatibility on actual pinned dependency |
| Operational tools | SQL inspection, integrity/backup facilities; WAL checkpointing and file permissions require documented operation | Engine APIs for integrity and closing; custom inspection/export/schema tools would be Riesel's responsibility |
| Performance | Not measured for Riesel | Not measured for Riesel |

Primary evidence: [SQLite SQL UPDATE](https://www.sqlite.org/lang_update.html), [SQLite WAL](https://www.sqlite.org/wal.html), [SQLite PRAGMA synchronous](https://www.sqlite.org/pragma.html#pragma_synchronous), [redb concurrency modes](https://docs.rs/redb/latest/redb/enum.ConcurrencyMode.html), [redb Durability](https://docs.rs/redb/latest/redb/enum.Durability.html), [rusqlite release/MSRV](https://docs.rs/crate/rusqlite/latest/source/Cargo.toml.orig).

### Durability and safety boundaries

- An acknowledged write is not equivalent to a safely persisted write unless the selected engine's **actual configured commit/sync path** guarantees it. SQLite WAL `NORMAL` and `FULL` differ; redb exposes different durability modes. Document hardware/filesystem assumptions and do not infer a tested power-loss guarantee from process-kill tests.
- `RecordStore` v2 has **no idempotency key** and no exactly-once semantics. After a lost reply, the caller must not blindly replay `create` (which would allocate another UUID), nor assume an attempted update/delete failed.
- SQLite and KV candidates can enforce optimistic revision checks transactionally; none automatically defines the app's schema/versioning, backup, or lifecycle policy.
- Provider initialization must be fallible and observable **after** side-effect-free preflight, **before** managed readiness or local capability use. Existing `ComponentDefinition<S>` construction is infallible; a narrowly scoped runtime factory design needs explicit review.

## Provisional recommendation and open decisions

**Provisional first choice for an embedded, single-host `RecordStore`: SQLite via `rusqlite`.** Its established transaction/durability semantics and operational inspection/migration tooling are directly relevant to storing user data long-term. This preference is **conditional**, not an accepted engine choice or a finding that redb is less correct.

**Main comparator:** redb, particularly if native build portability or a strict Rust-only dependency policy becomes an accepted requirement. Its ordered KV model fits the existing capability closely; the recently released v5 process-sharing and format/version behavior should be verified under Riesel's toolchain.

**Unresolved architecture decisions before Gate B approval:**
1. Should the first provider guarantee **single-host local-disk storage only**? May more than one local/managed process open the same database?
2. What is the default acknowledged-write guarantee for abrupt process failure versus OS/power loss? Which durability modes are permissible for production?
3. How does the provider receive authoritative model metadata across linked and managed placements, and what happens when the stored schema differs?
4. Does Riesel extend its general `ComponentDefinition` with a fallible constructor, or use a separate constrained provider interface? Neither design is accepted here.
5. What are the explicit DB-path, file ownership/permissions, locking, corrupt-file, backup and error-classification policies?
6. What are the actual dependency version, licensing, pinned toolchain and Windows pre-release compatibility results?

### Decision-critical follow-up, not a full engine implementation

- Create **small isolated proofs** for SQLite and redb: atomic two-writer expected-revision update, bounded UUID paging, reopen after clean exit and forced process termination, invalid/open-locked-file errors, and schema-metadata mismatch behavior. These are research experiments, not new production providers.
- Document the tested versions, commit modes, filesystem, toolchain, commands, failures and measurable observations. Compare comparable acknowledgment guarantees; do not infer power-loss protection from tests unable to simulate it.
- Review the minimal fallible startup change separately against Riesel's local and managed lifecycle.
- If either candidate fails a hard requirement, re-open the relevant screened candidates rather than forcing a weak selection.

## Gate B: architecture decision pending

This evaluation **does not complete** [Issue #24](https://github.com/aschenrot/riesel/issues/24) and does not authorize implementation Issues. Gate B needs the explicit decisions above, evidence resolving any decision-critical unknowns, accepted testable guarantees, and maintainer acceptance or documented deferral in a separate reviewed architecture change.

No distributed replication, multi-user authorization, network filesystem sharing, implicit command retries, generic migration framework or cross-platform production claim is implied.
