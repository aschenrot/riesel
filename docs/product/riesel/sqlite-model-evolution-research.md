# Stage 6 — SQLite model evolution: research and proposed decision

**Status: research for specification [Issue #41](https://github.com/aschenrot/riesel/issues/41); Gate B is not yet accepted.**
This document distinguishes *observed implementation facts*, *target requirements*, *experiment results* and *proposals*. A green fixture does not authorize a migration mechanism or prove power-loss safety.

## Decision to make

How should a linked, single-owner SQLite \`RecordStore\` upgrade its Rust-derived application model definitions and JSON records without silently rewriting customer data or invalidating the published version-2 Storage capability?

This is **not** the same as upgrading SQLite's physical database tables. The existing on-disk format version is \`PRAGMA user_version=1\`. It has a \`models(name,definition)\` table and \`records(model,id,revision,data)\` table; \`definition\` and \`data\` are serialized JSON, \`id\` is a 16-byte UUID and \`revision\` an eight-byte big-endian \`u64\`. Existing \`SqliteStore::open\` compares the **entire model set** against persisted definitions in a transaction. Mismatches reject startup, with no migration or MemoryStore fallback.

The public \`ModelDefinition::validate_record\` accepts a missing *optional* field but rejects a missing required field, unknown field or type mismatch. Thus some data can satisfy both old and new models even though **existing provider startup must still reject any model-definition change**. Treat metadata compatibility, data compatibility and business compatibility as three different questions.

Authoritative baseline: [SQLite provider](../../../packages/storage/sqlite/src/database.rs), [schema validation](../../../crates/riesel_schema/src/record_validation.rs), [Storage architecture](durable-record-storage-architecture-proposal.md). Research fixtures: [migration_research.rs](../../../packages/storage/sqlite/tests/migration_research.rs).

## Scope and hard constraints

1. Preserve one explicit local SQLite path/owner, fallible startup and side-effect-free ordinary registration/\`App::preflight\`. No new generic Storage protocol, managed provider, remote rollout, implicit temporary provider, or reinterpreted timeout/retry semantics.
2. A migration must be **explicitly authorized**, source-checked, and fail closed. Never convert an unknown schema, silently remove data, reset UUIDs/revisions or run because a consumer merely reopened its application.
3. Separate the **physical disk format version** from **application model evolution**. No physical format increment is inherently needed to transform JSON documents, though a future change to migration metadata itself might require one.
4. Use the authoritative Rust model definitions for the target; avoid a second handwritten schema. If prior Rust types are used as source schemas, their historical shape/identity must remain immutable and testable.
5. Any accepted deployment mechanism must define behavior for overflow at \`u64::MAX\`, validation failure, concurrent file access, interruption, disk-full/permissions, partial or inaccessible backups, downgrade, and stale clients.
6. A migration is not a guarantee that older binaries understand the newer database. Old model definitions must continue to fail closed unless a separate, explicit backward migration is approved and tested.

## Concrete model-change cases for Gate B

The following are **proposed boundaries**; no transition is approved merely because the record validator accepts the resulting JSON.

| Change | Existing v1 behavior | Proposed authorized behavior |
| --- | --- | --- |
| Add an \`Option<T>\` field | Existing records may already validate against the new shape, but changed persisted model definition still blocks startup | Explicitly named metadata-only transition **after full-record validation**; require a decision on revision fencing even if bytes are unchanged |
| Add a required field | Existing records fail target validation | Require an explicitly authored default/transform and revalidate every record; no inferred default |
| Rename a field | Old field becomes unknown and new field missing | Require a named transform retaining intended information; an inferred rename is not reliable |
| Change a field type | Some existing values fail target validation or have ambiguous semantics | Require deliberate checked conversion; fail all rows atomically if any value cannot convert |
| Remove a field or model | Old data/model metadata conflicts with the new shape/set | No silent deletion; explicit retention/export or destructive-transition authorization and verifiable backup |
| Change business meaning without changing shape | Metadata checks may see no difference | **Not detectable from structural schema alone**; require an explicit domain/compatibility decision if stored values need rewriting |
| Alter physical SQLite tables / format | \`PRAGMA user_version\` differs, or required tables no longer match | Separate, explicitly versioned physical-format migration; JSON model transitions do not automatically approve this |

For every authorized case, specify source model identity, target model identity, whether record bytes change, revision behavior, cross-model invariants, downgrade path and a failing fixture. A candidate that cannot explain a case must reject it, not silently approximate.

## Evidence and candidate comparison

The candidate dimensions are **authoring/authorization**, **execution/atomicity**, and **backup/recovery**. They need not be selected as one indivisible package. These are qualitative design comparisons, not benchmark results.

| Candidate | Authorization and authoring | Atomicity / recoverability | Model, identity and revision risk | Operational cost / fit | Current disposition |
| --- | --- | --- | --- | --- | --- |
| Exact match plus manual export/recreate/import | Explicit operator action; minimum new SDK API | Depends on import transaction and verified backup; easy to make an unsafe manual sequence | Import must preserve UUIDs, full \`u64\` revisions, model metadata and references; CRUD \`create\` cannot simply restore old IDs | Tooling and operator burden rise with data volume | Keep as safe **no-automatic-upgrade** fallback, not adequate as default managed schema evolution |
| Explicit ordered Rust transformations within one SQLite \`IMMEDIATE\` transaction | Explicit maintenance command and source/target schema verification; a Rust old-to-new function can express renames and type changes | SQL rows and metadata can commit/roll back together; large transactions hold a write lock, may exhaust disk/WAL | Validate old and new shapes; preserve UUID; explicitly decide revision fencing; overflow must abort | Strong fit to current linked single-file store; needs bounded work, offline upgrade and a verified recovery path | **Provisional first candidate**, subject to tests and Gate B |
| Automatically infer compatible changes (e.g. optional fields) | Low developer boilerplate if compatibility is genuinely proven | Can use the same transaction, but auto-detection is a separate policy risk | JSON structural compatibility does **not** prove semantic compatibility, defaults or rename intent; removing fields risks data loss | Tooling for safe classification may exceed a first bounded slice | Consider explicit **metadata-only** migration after validation; reject implicit automatic data changes |
| Copy/shadow transform, validate, swap | Explicit maintenance step and restore point | Source can remain untouched during construction, but switching files needs WAL handling, ownership fencing, and crash-safe swap/restore contract | Must copy metadata, IDs and revisions exactly; verify all transformed target records and references | Extra disk, cross-platform file replacement and checkpoint/directory durability risks | Candidate for large databases or later hardening; do not assume a bare file rename is crash-safe |
| SQLite backup API / \`VACUUM INTO\` as a preflight snapshot | Explicit operator backup with integrity verification | Creates a consistent snapshot; does not roll back committed source changes or safely replace a live database by itself | Independent of application schema transformation | Requires extra disk and a restore/verification playbook | Relevant complement to any mutating approach, **not** a migration engine |

No SQL migration crate can automatically convert arbitrary Riesel **JSON BLOB records** from one Rust model shape to another. A SQL schema tool may help with physical table changes, but is not by itself a substitute for record transformations.

**External sources:** [SQLite transactions](https://www.sqlite.org/lang_transaction.html), [WAL operation and concurrent writers](https://www.sqlite.org/wal.html), [WAL mode and synchronization](https://www.sqlite.org/pragma.html#pragma_synchronous), [online backup](https://www.sqlite.org/backup.html), [VACUUM INTO limitations](https://www.sqlite.org/lang_vacuum.html#vacuuminto), and [rusqlite 0.43.0 opt-in backup feature](https://docs.rs/crate/rusqlite/0.43.0/features).

## Fixture evidence — scope of the spike

The research-only tests use the **actual production \`SqliteStore::open\` to create format-v1 databases** (two named models, two tasks, one UUID-linked note and an updated task at revision 2). A local **test-only** SQL transformation explores an \`IMMEDIATE\` transaction; it is **not connected to the SDK**, not a supported migration tool and not proof that every future application migration is atomic.

| Fixture | Observation being tested | Does not establish |
| --- | --- | --- |
| Frozen format-v1 bytes from a static, independently authored SQL fixture | Historical schema, model-definition bytes, UUID/revision BLOB encoding and records reopen with the current provider, then pass a test-only upgrade | Compatibility with arbitrary historical database versions or a supported upgrade path |
| Format-v1 reopen and changed model | v1 metadata, UUID/revision BLOB shape, model count, restart-safe rejection without logical data mutation | Byte-for-byte unchanged database/WAL or a supported migration |
| Add an optional field | Existing records pass target *record* validation, but provider still rejects unequal model metadata | That any optional field is semantically safe to auto-upgrade |
| Explicit additive transformation | Update task JSON + task metadata together; note references and IDs unchanged; task revisions increment, stale write conflicts | Generic migration API, semantic cross-model consistency, unlimited transaction size |
| Injected per-record validation failure / early interruption | Roll back modified rows and model metadata together | Real disk-full, I/O error and every crash point |
| Full-width revision overflow | Fail entire transformation if incrementing a record at \`u64::MAX\` | A new revision/generation scheme |
| Forced child process exit before commit | Incomplete model/data writes are rolled back when SQLite reopens | Host/power loss or volatile filesystem corruption recovery |
| \`VACUUM INTO\` with live source open | Consistent snapshot reopens and preserves an acknowledged record | Crash-safe destination replacement, backup retention or directory sync guarantees |

**Verified research probe:** Rust 1.100.0 with the pinned `rusqlite = 0.43.0` succeeded on all **11/11** migration research tests at commit `33d9f5cf` ([run 44657435701](https://github.com/aschenrot/riesel/actions/runs/44657435701)). These are test-only SQLite semantics, not a production migration engine. Normal workspace validation and smoke remain an independent final-commit gate.

## Provisional specification to review at Gate B

This is a **proposal**, not an accepted requirement.

### Authorization and execution

- Preserve strict exact-model-match on normal \`SqliteStore::open\`; never silently migrate during startup or standard Riesel preflight.
- Consider a distinct **offline maintenance action** that accepts the explicit database path, trusted Rust migration plan and desired new Rust-derived model set. Require application shutdown, exclusive ownership policy, a read-only *preview* separate from \`App::preflight\`, and a separate opt-in mutating execution.
- Check the **entire stored model set** and expected source definitions before starting mutation. Apply only deliberately registered transitions; unsupported/unknown source states fail without metadata or record rewrite.
- For a bounded first delivery, test **one file-wide \`BEGIN IMMEDIATE\` transaction** for transformed record payloads, their revisions, and model metadata. Validate source and output records. Abort the complete transaction on any failure. Define size/duration/disk quotas and reject databases outside the guaranteed envelope instead of silently making many independently committed batches.
- A schema-only transition with unchanged payloads may be technically possible; authorization and future stale-client behavior still need explicit review. No inference solely from an added \`Option<T>\`.
- A change to the physical table layout is a **separate** versioned transition from JSON model evolution.

### Critical unverified risk: stale open providers

Current \`SqliteStore\` caches registered \`ModelDefinition\` values when opened; ordinary \`create\` and \`update\` validate against that cached map and do **not** re-read persisted model metadata on every command. SQLite's \`BEGIN IMMEDIATE\` provides **transactional writer serialization**, not lifetime-long provider ownership. By inspection of these code paths, a *second provider already opened with old Rust model definitions* could potentially write legacy JSON **after** another connection commits new model metadata. Such a write could make later reads under the target schema fail.

This is a **code-path risk inference, not a successfully executed stale-owner fault-injection test**. Before accepting a migration implementation, demonstrate the failure mode with a controlled two-owner fixture and select a real exclusive-offline ownership/fencing policy; alternatively design a runtime check that rejects stale providers. Do not claim an \`IMMEDIATE\` transaction alone prevents these writes. Until then, the safe migration contract cannot allow another active provider or untrusted direct database writer.

### Revisions, identity, compatibility

- **Proposed conservative fencing:** increase revision once for each record in every changed model during a model transition (even if a particular payload is unchanged), so clients holding pre-migration revisions cannot write unnoticed. Preserve UUID and model association. If any required increment overflows, reject and roll back the entire migration.
- Preserve all unchanged-model records and references, and validate any application-specific cross-model invariants through explicit migration checks. The generic Storage schema does **not** currently provide foreign-key enforcement for JSON references.
- Never automatically downgrade. A reverted binary cannot open the evolved model set; rollback means restoring a separately verified compatible backup, losing any post-snapshot writes unless a separate restoration policy exists.
- Model transition identity must be tied to **persisted expected source definitions** and derived target definitions. A plain numeric sequence number alone is not sufficient to establish the old data shape. Review whether exact serialized definitions are a stable cross-version representation or whether a separately versioned canonical encoding is needed.

### Existing SDK/derive constraints verified

- The \`RieselModel\` derive requires a **named Rust struct** with \`#[riesel(model = "name")]\` and derives a \`BTreeMap\` definition from its field types. It does not support generics, and rejects field-level Serde customization.
- \`Application::records_at<T>()\` registers a single authoritative definition and requirement alias; registering an old and a new shape under the same name in one application fails as a duplicate. \`Application::sqlite_storage()\` binds the **current** registered model map only.
- Accordingly a typed migration can retain a historical \`OldTask\` with the same persistent model identity **outside** the current app's normal \`records<T>\` registration. An offline maintenance action is more consistent with this SDK boundary than silently modifying \`sqlite_storage()\` startup. The author-facing API shape still needs proof with a real Rust derive fixture and migration ownership/alias checks.

Sources: [RieselModel derive](../../../crates/riesel_macros/src/derives/model.rs) and [Application builder](../../../crates/riesel/src/application/builder.rs).

### Minimal author-facing API alternatives (illustrative, not compiled)

A. Typed transitions: \`migration::<OldTask, Task>(|old| -> Result<Task> { ... })\`, where the old and new Rust structures each derive their model definition. This removes handwritten JSON schemas but requires keeping an immutable historical Rust type and an unambiguous persistent model name.

B. Versioned record transform: \`migration_for::<Task>(expected_source_definition, |old_json| -> Result<NewJson> { ... })\`. This can handle removed legacy Rust types but risks duplicating source metadata and weakens compile-time shape checks.

Prefer the smallest API **after** verifying model alias binding, multiple-model transactional consistency, old-schema identity, and Rust derive behavior. Do not modify the generic \`Application\` builder or \`RecordStore\` contract without a demonstrated reusable need.

### Failure / acceptance gates before an implementation Issue

1. Old **format-v1** fixtures must reopen unchanged with the old binary; they must not be mutated on inspection or on unsupported new-model startup.
2. Explicit migration must preserve UUIDs, unchanged-model data and record ordering, increment required \`u64\` revisions atomically, reject overflow, and fail on invalid/unsupported schema changes.
3. Verify \`BEGIN IMMEDIATE\` conflicts against another open writer; define operational owner exclusion rather than claiming a reliable file lock simply from a successful transaction.
4. Exercise per-record exceptions, transaction rollback, abrupt process exit, full/damaged database, permission/I/O errors and corruption; distinguish which faults are simulated from those proven on the pinned build.
5. Independently verify **backup completeness**, restore of an old binary/database, target model validation, and fail-closed state after failed or interrupted migration. Do not copy a live WAL \`.db\` alone.
6. Verify ordinary SDK preflight and application startup remain side-effect free/exact-match respectively. Linux tests, smoke, documentation and Windows **manual pre-release validation** are separate gates.
7. Define log/error redaction: no untrusted raw database path, record body or secret payload in browser/remote errors.
8. Document accepted migration envelope (record count, payload bound, timeouts, lock/WAL size) and defer large/online migrations when guarantees are insufficient.

## Unresolved decisions for an accountable reviewer

- Are automatic **metadata-only** upgrades explicitly authorized for validated optional-field additions, or should all changed model metadata require a named transition?
- Is revision-per-record fencing appropriate for every schema transition, including a source/target model with structurally identical data? Are \`u64::MAX\` records a deliberate hard stop?
- Must migrations run via a separate offline maintenance CLI, an explicit API call, or an opt-in startup mode? What is the trusted operator consent mechanism?
- Which backup/restore and OS/disk failure guarantees are in scope for the first implementation? Does the product require a recoverable backup **before** mutation, and who verifies and retains it?
- Which migrations need access to **multiple model types** at once, and how are cross-model references/renames specified without another schema authority?
- What happens when the DB is too large for an atomic transformation, disk space is insufficient, or another process holds a read/write handle?

**Decision gate:** review evidence and these policy questions in Issue #41. Record **accept, revise, defer or reject** with rationale; only after acceptance update the durable Storage architecture and plan separate implementation Issue(s). No proposed API or execution mode is part of the delivered platform today.
