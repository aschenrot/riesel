# Trusted-local V2 execution-plan revision persistence — first implementation

**Status:** first independently verified, local application-owned **persistence foundation** for accepted [#111 C/S2/V2/A2/B2/C3](execution-plan-contract.md), tracked by [#193](https://github.com/aschenrot/riesel/issues/193). Not the complete #193 feature, external editor exporter, verified remote publisher identity, activation journal or power-loss guarantee.

## Scope and ownership

The SDK defines `PlanRevisionRepository`, `PlanSemanticCandidate`, `PlanLayoutDraft`, `PlanAuthoringHead` and `CheckedSourceHandoff` in `riesel_app`, also available through the `riesel` facade. The first **optional implementation**, `SqlitePlanRevisionRepository` in `riesel_storage_sqlite`, is opened explicitly by a trusted Rust application against its own dedicated file. It does **not** modify a `RecordStore` database and does **not** add an SQLite dependency to the runtime graph runner or require Riesel Manager/GUI. Future backends can implement the same conditional repository protocol.

The caller supplies a local `trusted_registration_stamp` describing the installed app/operation-catalog build. The store fences mismatched **trusted local registrations** on mutation. **This stamp is not an authenticator:** any independent client with OS write access to the database can copy it or directly modify SQLite. The database must be protected by the application/OS owner. Never expose these repository methods as unauthenticated RPC, use a copied stamp as a per-client grant, or claim hostile same-UID protection. [#75](operational-contract.md), [#60](https://github.com/aschenrot/riesel/issues/60) and [#195](https://github.com/aschenrot/riesel/issues/195) govern the deliberately published application-authorized interface and activation semantics.

## Semantic and layout revisions

- **Semantic candidate:** `PlanSemanticCandidate::from_rust` accepts an already validated Rust plan but stores only its Serialize-only `ExecutionPlanDescription` **inspection projection**. `from_s2` requires the actual `OperationRegistry` with trusted exact-version A2 editor descriptors, strictly checks and binds the S2 document, and retains an approved document source. A stored inspection projection is **not** independently executable code.
- **Durable initialization and append:** `initialize` records an immutable first semantic revision, authoring source and generation. `append(expected_head, candidate)` checks exact semantic predecessor, source kind/generation and installed registration stamp in an SQLite `BEGIN IMMEDIATE` transaction before inserting a new immutable row. It does not change the active application runner.
- **Independent layouts:** `save_layout(expected_head, previous_layout_version, draft)` performs an independent compare-and-set on a named presentation stream. Layout coordinates are `i32` and may refer only to operation IDs present in the exact **historical semantic anchor**. A layout-only save never edits semantic dependencies, triggers, resources, domain, binding or semantic revision. Rebase requires the caller to specify the new anchor and pass checked node references; an unknown/removed node rejects rather than being silently dropped. Historical layouts are readable.
- **Consistent current authoring reads:** `current_authoring(plan_id)` returns the **authoritative head and current S2 document together inside one SQLite read transaction**, not two independently raceable queries. After Rust→document handoff with no semantic change, it reads the initial owner-handoff document; once a document-owned semantic revision is appended it reads **that revision's** document instead. Under Rust ownership it exposes no editable document. `semantic_document(plan_id, revision)` reads exact historical document-authored source; the older `source_document(plan_id, generation)` represents **the document at that handoff generation**, not necessarily the current revision. All returned bytes remain untrusted source and require exact A2 binding/authorization before executable publication.
- **Source handoff:** trusted Rust application code constructs a `CheckedSourceHandoff` from the *actual* old validated plan plus exact S2 registered-descriptor binding, or from a document plus an exactly matching intended Rust plan. Unsupported/missing/unsafe registered operations or unequal plan projections reject before transfer. A conditional `handoff` increments the source generation atomically without rewriting old semantic history and retains the document source for the new generation. All stale semantic and layout writers subsequently fail. The linked application must still authenticate any external transfer request and confirm it has authority to relinquish or adopt the source.
- **Failure semantics:** local writes either commit a complete SQLite transaction or roll back. Independent readers can reopen persisted accepted revisions; a failed or stale CAS does not mutate accepted revisions. Schema `user_version` is explicitly checked; foreign/unknown schema versions reject without an automatic migration. Local SQLite WAL and `synchronous=FULL` are checked and busy timeout is bounded (5 seconds).

## Example (trusted Rust process)

```rust
use riesel::{
    AccessMode, ExecutionPlanDraft, OperationKey, OperationNode,
    PlanAuthoringSource, PlanRevisionRepository, PlanSemanticCandidate,
};
use riesel_storage_sqlite::SqlitePlanRevisionRepository;

let key = OperationKey::new("app.load", 1)?;
let plan = ExecutionPlanDraft::new("app.pipeline", 1)
    .operation(OperationNode::new("load", key).access("state", AccessMode::Write))
    .validate()?;
let candidate = PlanSemanticCandidate::from_rust(&plan)?;
let store = SqlitePlanRevisionRepository::open(
    "/app-owned-data/plan-revisions.sqlite", "trusted-app-build-1"
)?;
let head = store.initialize(&candidate, PlanAuthoringSource::Rust, "trusted-app-build-1")?;
assert_eq!(head.semantic_revision, 1);
```

Use this example as a conceptual trusted-local call site; the storage location must be real, writable and app-owned, and the sample's hard-coded build correlation is not a security credential.

## Alternatives and why this first slice

The existing `RecordStore::update` supports CAS on **one record only**, not a coherent transaction spanning immutable semantic history, independent layouts, source generations and future activation receipts. A giant single-record aggregate would create coupling and write contention across unrelated layout streams. A filesystem lock/rename or append-only log could also satisfy these goals, but needs independently verified multi-process CAS, crash sync/fencing and replay/compaction. A mandatory Manager store violates headless Rust-first ownership.

[Research PR #204](https://github.com/aschenrot/riesel/pull/204) tested 6/6 targeted actual OS-process SQLite transaction scenarios, including opposing semantic/layout writers, pre-/post-commit process kill and source fencing. This made a **portable, app-owned revision repository protocol with an optional local SQLite reference adapter** an evidence-supported first implementation. It is not evidence for a universal mandatory SQLite backend.

## Checked source-handoff coverage diagnostics

`PlanSourceCoverageReport::compare_rust_to_s2(original, document, registry, limits)` reports **each** identifiable incompatibility before a deliberate Rust→document or document→Rust authoring transfer. Findings distinguish plan ID/revision mismatch, missing or additional nodes, registered handler/version or editor-policy-version mismatch, changed control prerequisites/triggers/resources/domain admission, and failures from the real A2/production plan binder. A complete report requires exact serialized semantic projection identity with the original validated plan. This is a conservative **fail-closed** contract: it may reject differently ordered but apparently equivalent authoring sources instead of silently reinterpreting them.

The checked `CheckedSourceHandoff` constructors enforce a complete coverage report before returning a transferable proof. This does not reverse-compile Rust closure code, prove callback resource aliasing or authorize a caller; the application/domain owner still controls which source receives authority, and the durable store still atomically fences the former writer. The report can later drive actionable UI diagnostics without a GUI dependency or an independent execution validator.

## Reproducible real-process commit-window crash verification

The default V2 storage build has **no fault hooks**. An explicitly opt-in `riesel_storage_sqlite/v2-fault-injection` test feature exposes four internal `V2FaultStage` points in the **same production** `SqlitePlanRevisionRepository::append` and `save_layout` SQL transaction paths:

- Semantic revision immediately **after** the new row and head update, **before** commit; and immediately **after** successful commit, **before** returning the result.
- Layout revision immediately **after** the new layout row and layout-head update, **before** commit; and immediately **after** successful commit, **before** returning the result.

The [Linux Validate workflow](../../../.github/workflows/validate.yml) runs the dedicated integration test with:

```sh
cargo test --locked -p riesel_storage_sqlite --features v2-fault-injection --test v2_process_crash
```

The parent spawns a genuine child OS process, blocks it at the selected **production adapter** hook, kills it without releasing the hook, and reopens the dedicated SQLite file. A pre-commit kill must leave no partial accepted head or revision, and a new conditional write must succeed. A post-commit kill must leave the committed head/revision, and stale retry must conflict. Layout tests additionally prove that semantic revision did not change and old layout history remains. The special feature must **never** be enabled in deployed builds or exposed to remote callers; code-only defaults remain hook-free.

The experiment demonstrates local process-death transaction atomicity/recovery at the chosen Linux SQLite WAL/FULL profile. It **does not** demonstrate power-loss durability, power-failure fsync correctness on all filesystems, storage artifact provenance, external app-client authentication, permitted runtime activation, backup/restore or schema migration. Those are independent obligations under #193 and #195.

## Application-owned verified V2 backup and new-file restore

The optional local SQLite backend also exposes `SqlitePlanRevisionRepository::backup_to(destination, V2SnapshotBudget)`, `verify_backup(path, expected_registration_stamp, budget)`, and `restore_backup_to_new_file(backup, destination, expected_registration_stamp, budget)`. These methods use a separate **data backup** lifecycle, not plan publication or runtime activation.

**Snapshot creation:** reuse the existing Storage maintenance destination policy: require an app-controlled directory, reject existing output or unsafe group/world-writable parent directories on Unix, create a unique pending file, and use SQLite's snapshot-consistent `VACUUM INTO` rather than copying the live WAL database. Audit the source and backup for expected V2 `user_version`, integrity, foreign keys, current semantic-head references, app-build correlation and explicit row/byte budgets. Compare the five content tables exactly and reject a racing source mutation detected during verification. Sync the pending file and publish with an atomic **no-overwrite hard link**, then sync the directory on Unix.

**Recovery:** `verify_backup` is a read-only check. `restore_backup_to_new_file` requires an existing verified backup and a fresh, absent destination, copies into a unique pending file, verifies the copy and matching data, then publishes without overwriting anything. It does **not** replace a live application's database, acquire the application runtime session or grant the new file concurrent execution authority. The application owner must quiesce/stop the old service and explicitly select the recovered file before using it as authoritative; otherwise there would be two independent writers even though each database enforces its own CAS. Never replace an active SQLite DB or its WAL sidecars through a plain filesystem copy.

**Installed build and format evolution:** the backed-up record carries the original app registration correlation; restoring or opening under a different claimed build does not make that build an authenticated publisher. A different trusted-local registration stamp is denied on mutation, and backup verification against a different stamp rejects. Unknown on-disk format versions **fail closed**; no destructive implicit migration or automatic registry upgrade is performed. Moving a plan to new exact-version Rust handlers still requires independently checked A2/B2 bindings and an app-authorized V2 source transfer. A future explicit store-version migration and real installed-executable principal verification need separate proofs.

The results are independently verifiable with `cargo test --locked -p riesel_storage_sqlite --test v2_backup`. These tests verify real persisted immutable plan/layout/source history across verified backup, reopen and new-file restore; later live-source mutations do not alter a snapshot; existing destinations cannot be overwritten; corrupted/unknown-format/foreign-stamp backups and undersized budgets reject without publishing output.

**Scope limits:** SQLite's local `VACUUM INTO` with explicit post-sync and integrity checks is evidence for the tested local filesystem, not a power-failure benchmark or an authenticated archival signature. The budget bounds resulting bytes/rows; it does **not** interrupt a blocking SQLite operation mid-call or prove capacity on every filesystem. No externally hostile same-UID client, cross-machine consensus, immutable cryptographic provenance, automatic activation, or Windows release validation is claimed.

## Verification and explicit remaining work

Focused production adapter tests target independent OS processes racing the actual `SqlitePlanRevisionRepository` semantic and layout operations, immutable plan history, independent layout streams, invalid anchor rebase, checked code↔document source handoffs, stale-writer fences across reopen, installed-build stamp mismatch and unknown on-disk version rejection. Preserve exact-head `cargo validate --check` and `cargo smoke` evidence for the implementation PR, not just the research fixtures.

**Not yet proved or delivered:** killing a process *inside each actual production adapter transaction* before/after commit, OS crash/power-loss guarantees, backups and schema migrations, quantified high-frequency layout contention, issuer-verified artifact/client identity, permissioned authoring/transfer across independent principals, ownership after application replacement, real runtime activation, receipts/unknown-effect reconciliation, typed S3 dataflow and a graphical editor. These remain [#193](https://github.com/aschenrot/riesel/issues/193), [#195](https://github.com/aschenrot/riesel/issues/195), [#196](https://github.com/aschenrot/riesel/issues/196) and [#187](https://github.com/aschenrot/riesel/issues/187) acceptance work. Do not close #193 based only on this first local implementation increment.
