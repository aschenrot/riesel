# Todo #119 — recoverable lifecycle and Query decision review

**Status:** research proposal, **not owner acceptance** and **not a production implementation**. Base: `f8aee971cbe433f0280ecb2e88f1efc87dd6c10b` on 2026-11-22. Do not merge an altered Todo model, change existing `DELETE` behavior, or upgrade a user's database based on this review alone.

## Original product outcome

Todo users can remove active tasks without permanent loss, restore **the same task identity** after restart or in another supported client, and deliberately purge a task from Trash. Title, completion, UUID and revision correctness must survive two-client concurrency, partial batch outcomes, unknown mutation outcomes and existing SQLite files. No automatic retention or Archive feature has been accepted. The authoritative record remains the task itself, not a browser cache.

The completed #107 delivery provides real linked, bounded `completed Eq bool` Query reads and Open-first/collapsed Completed. Trash must **not** make those views show deleted tasks; simply renaming the current permanent deletion button is insufficient.

## Source audit and constraints

- `examples/todo/src/task.rs`: `TaskData { title: String, completed: bool }`; `delete_task` calls revision-fenced `Records::delete`, permanently removing the record. `modify_task`/Storage v2 can revision-fence an in-place task field change while retaining UUID.
- `packages/storage/contract/src/storage.rs`: `create` allocates UUID; there is no caller-selected-UUID restore and no published atomic cross-model move or multi-command transaction.
- `packages/storage/contract/src/query.rs` and `record-query-usage.md`: `riesel.query.records@1` has **at most one** `RecordPredicate`, deterministic live ascending UUID order, a 32-result/256-inspected call envelope and one-use, bound/expiring cursors. SQLite evaluates the typed comparison on a bounded UUID-key scan rather than a persistent field index. Its reader is independent of Storage v2 but bound to the same linked owner/session for read-your-writes.
- `docs/product/todo/http-local-browser-contract.md`: the current `DELETE /api/v1/tasks/{id}` permanently deletes with a client-observed revision; `PATCH` accepts exactly one title or completed edit; missing/unknown mutation replies may be `outcome_unknown`. Old clients must not be silently given new destructive semantics.
- `sqlite-offline-migration-architecture.md` and `sqlite-offline-operator-runbook.md`: normal startup fails closed on incompatible model shape. Deliberate typed **offline** source→target migration, stopped legacy writers, verified retained backup, complete model-set validation, transactional transform, UUID preservation and a one-step revision increase are required. Existing operator facilities are implemented in bounded trusted-local Linux scope; Todo-specific historical/current types, plans, reviewable limits and operator instructions are not yet integrated.

## Alternatives compared against the same outcome

| Alternative | Potential benefit | Decisive cost/failure condition | Assessment |
| --- | --- | --- | --- |
| **A. One task record + independent lifecycle field** (`active`/`trash`) | Identity, completion and title remain on one authoritative record; transition/restore is one revision-fenced Storage update; no cross-model partial move | Requires explicit model migration, domain-validated lifecycle values, new Active/Trash Query semantics, intentional legacy API adaptation | **Recommended foundation**, conditional on typed migration and Query evidence |
| **B. Separate tombstones / Trash record model** | Could preserve current active model shape and isolate retention | Storage v2 has no atomic move across models. Crash/partial failure can leave both/no authoritative objects; regenerated IDs break identity; extra transactional owner/repair mechanism needed | Defer unless a demonstrated transactional approach outperforms A on the complete requirement |
| **C. Event log / command-history authority** | Audit, historical Undo and richer eventual recovery | Requires durable event ordering, idempotency, compaction, replay and a new consistency boundary beyond the requested Trash lifecycle | Too much unsupported scope for this milestone; potential independently accepted future capability |
| **D. Generic Storage soft-delete/restore operation** | Shared implementation for many models | Forces Todo retention semantics into Storage v2 despite no accepted multi-domain requirement; versioned API and provider work | Reconsider only with demonstrated cross-application need |
| **E. Delayed client-side deletion / recreate** | Small UI change | Browser crash loses Trash, client reconnection fails, new UUID/revision cannot preserve identity, races and unknown outcomes are unsafe | Reject |

### Query architecture must be decided separately

The first linked Query profile cannot natively express `location == active AND completed == false`. Alternatives:

- **Q1: bounded, typed conjunction profile** (candidate). Accept at most two or a small fixed finite number of field comparisons under a **new separately versioned supported record Query profile**, with exact predicate validation/canonicalization, signature-bound one-use cursors and Memory/SQLite semantic parity. Owner/provider evaluates both predicates within the same budget. Keep v1 intact. No universal SQL/expression engine, permanent physical index or automatic snapshot. This is the **preferred durable reusable direction** *if* the cost and compatibility tests justify the new profile.
- **Q2: one denormalized Todo status-bucket string** (`active_open`, `active_done`, `trash`) for the existing single-predicate Query. Avoids a new Query profile, but duplicating location and completion introduces a manually maintained invariant, contradictory states and further migration expense. Only acceptable with one-transaction derived-state enforcement and demonstrated operational advantages.
- **Q3: application-owned specialized projected view/query** that exposes the compound policy without changing Query v1. May be suitable where provider/domain authorization boundaries require it, but must not scan without budgets, make the HTTP layer the data authority or duplicate persistent indexes.
- **Q4: query `completed` and remove Trash rows in the browser.** Not acceptable as an authoritative, scalable filter; potentially requires arbitrarily many pages, and cannot distinguish definitely empty from only temporarily unmatched segments without fully exhausting the live scan.

For the first release, Q1 versus a bounded, application-owned Q3 should be compared with *real* Memory/SQLite 33/100/10,000 mixed-status/Trash fixtures, provider inspection budgets and a two-client mutation-behind-cursor race. A sparse scan may still be slow; a separately accepted physical index requires offline upgrade and rollback. No latency guarantee or global count can be inferred from existing #107's 514-ms isolated result.

## Proposed behavioral contracts requiring owner acceptance

1. **Model:** one closed set of `active` and `trash` locations, independent of `completed`. Every migrated historical task becomes `active`, preserving title and completion. Do not add Archive or retention timestamp without a concrete need. Decide whether a Rust typed enum is supported by registered model/query fields before selecting wire shape; the tested string shape is a *research candidate*, not the approved model API.
2. **Commands:** `move_to_trash(id, observed_revision)`, `restore(id, observed_revision)`, `purge(id, observed_revision)`; reject invalid source states. All require a fresh client-observed revision, return a confirmed resulting record for reversible changes, and never reissue an uncertain write. Preserve UUID and completed state when moving/restoring, increment revision on each successful change. A second client's intervening edit produces a conflict.
3. **Old delete interface:** do **not** silently relabel current `DELETE` as Trash or continue allowing old clients to purge active records unnoticed. Define a clearly versioned route/CLI transition and fail-closed compatibility behavior (e.g., permanent purge accepted only for a currently trashed row, after explicit confirmation; otherwise reject). Audit R0, R1, R2, API shape, legacy clients and maintenance scripts before freezing names/status codes.
4. **Reading:** Open/Completed show only **active** tasks; Trash is an explicitly opened independent section with its own finite cursor, exhausted/error/refresh generation and no fake counts. Query is applied inside the Todo-domain authority; HTTP supplies only restricted selectors. Mutation-behind-cursor and lost/expired one-use cursor require honest fresh reconciliation, not silent retries or claims of a snapshot.
5. **Undo and batches:** immediate Undo of confirmed Move to Trash uses the returned revision and restores in place; Undo cannot override later edits. Batch Move to Trash is per-row, loaded-only, ordered and reports successful/failed/unknown rows separately; no unapproved atomic-batch claim. Permanent purge has separate explicit user confirmation and no Undo.

## Evidence and implementation gates

A research-only test in this branch models an old `task` schema and a candidate evolved same-model `location` field against the **real** SQLite provider and offline maintenance engine. It tests migration, verified backup, UUID/revisions, stale-write rejection, live single-field Query visibility and restore. This proves a **mechanism** but not a completed Todo migration executable or UI. Because the test uses explicit schema definitions, production authoring must independently use Rust-derived historical/current models.

Before final architecture acceptance:

1. Compile and run the research test and current Linux CI at its exact revision; resolve failures rather than treating the source as proof.
2. Run realistic 33/100/10,000 native Memory/SQLite and real HTTP/Chromium **compound** Active/Open/Completed/Trash experiments; measure candidates, pages, latency and invalidation without hiding sparse costs.
3. Validate the actual Todo historical → evolved **Rust-derived** schema and a bounded operator preview/execute/restore against an old database fixture; include malformed data, capacity, interrupted execution, retained backup, restart, multiple clients and old binary compatibility.
4. Specify Q1 or Q3 API version, predicate/operator boundaries, physical indexes if proven necessary, cursor/budget signatures and read-only grants. Test unsupported forms and cross-owner/session misuse.
5. Decide legacy `DELETE`/CLI compatibility, move/restore/purge routes, ambiguous mutation handling and batch/Undo UX, then update the accepted product/HTTP/architecture contracts. Only then create separable implementation issues for model+operator/Query+API/browser UX.

**Decision requested:** accept/modify/reject A (single-record location + offline migration) as the data foundation; separately select Q1 (bounded typed conjunction) or Q3 (Todo/domain-owned limited compound projection) after targeted evidence. No proposal here changes `main` or constitutes product-owner acceptance.
