# Todo #107 — native Record Query integration audit and decision packet

**Date:** 2026-11-22. **Status:** research-only recommendation, **not** owner acceptance or Todo implementation.  
**Base:** Riesel `main` at `ada880cc2a5b03181993e223d0cf8bfce66a6709`.  
**Decision owner:** Todo product owner, through [specification #107](https://github.com/aschenrot/riesel/issues/107).  
**Related accepted platform decision:** [typed linked Record Query #138](../riesel/record-query-architecture.md), implemented [#160](https://github.com/aschenrot/riesel/issues/160) and [#161](https://github.com/aschenrot/riesel/issues/161).

## The user outcome is not a query method

For a durable local Todo app, **actionable open tasks must be discoverable without a hidden-page trap**, with completed tasks visually subordinate, responsive and accessible operations, and truthful pagination under live mutations. The reference loads 32 records, while relevant datasets contain 33, 100 or 10,000 entries. A client must not mistake an empty *partial* query page for a globally empty Open section. User-reported completion must remain revision-fenced; a lost write reply never licenses an automatic retry. The UI must remain usable with small lists and independent of Manager, native GUI and remotely authenticated publication.

### Verified current source-level baseline

- `TaskData` remains `{title, completed}`; `Task` carries generated UUID and a complete `u64` revision. `Todo::list_tasks_page(after, limit)` calls writable Storage v2 `Records::list_page` ([task.rs](../../../examples/todo/src/task.rs)).
- `GET /api/v1/tasks?after&limit` returns `{records,next_cursor}` by ascending UUID, **without a status filter or exhaustion field** ([routes.rs](../../../examples/todo/src/http/routes.rs)). Browser `All/Open/Done` applies local filtering to already loaded pages, and `Load more tasks` advances only the unfiltered cursor ([+page.svelte](../../../examples/todo/web/src/routes/+page.svelte)). Counts are explicitly *loaded only*, not total or currently matched globally.
- `todo_http` currently declares only `Records<TaskData>` and `AsyncTasks`, with a local linked SQLite owner. HTTP commands execute inside the existing bounded, session-owned blocking-task boundary, **not** directly on Tokio's current-thread reactor ([todo_http.rs](../../../examples/todo/src/bin/todo_http.rs), [http/mod.rs](../../../examples/todo/src/http/mod.rs)).
- The now-merged [read-only `riesel.query.records@1` profile](../riesel/record-query-usage.md) supports provider-native `completed Eq bool`, same-instance reader/writer pairing, 1–32 matched rows and 1–256 examined candidates per call, **empty partial pages** with `exhausted=false`, one-use five-minute cursors tied to the exact predicate/budgets, and **no global counts, field-sort index or snapshot**. It is trusted linked, not an independent client grant.
- [Prior #107 research](https://github.com/aschenrot/riesel/blob/research/todo-status-pagination-107/docs/product/todo/status-query-research.md) established the old first-32-hidden condition with native Storage and a real Chromium browser, sparse 100-/10,000-task cases, live-cursor omissions and a *scratch-only* optional SQLite index. Those results predate the merged native Query facet. Do not present them as tests of the newly shipped provider.

### Reproducible native-provider evidence in this research PR

`examples/todo/tests/native_status_query_research.rs` exercises the **production** `RecordQuery` and `RecordStore` traits on both **actual** registered-model MemoryStore and SqliteStore state, not a synthetic identity-Query or a consumer-side filter.

| Probe | Expected invariant | Why it matters |
| --- | --- | --- |
| 33, last UUID Open | Old first 32 UUID page has zero Open; native Query returns zero matches with `inspected=32`, `exhausted=false`, and a continuation; next page returns one Open | Empty partial **must not** be a global no-task placeholder |
| 100, Open at UUID ranks 70/98/99 | Four query calls, each examines at most 32; first two return empty partial pages; final 3 IDs are distinct and in order | Filter is provider-owned and its cursor makes monotonic progress |
| 33, mutation behind cursor | Reopen an already scanned Done task; continuation sees only the later Open task; fresh query sees both | Live Query is **not** a snapshot; explicit refresh reconciles |
| 10,000, only last UUID Open | With `scan_budget=256`, 40 calls and 10,000 examined records; 39 empty partial responses | 256-per-call bounded work **does not** mean bounded end-to-end work or first-match latency |

The 10,000-record probe is intentionally ignored in normal workspace tests and runs separately in the dedicated `Todo Native Query Research` workflow. Timings are printed as contextual observations only, with neither a browser nor HTTP request chain; **do not use runner timings as UX SLAs**. Test/code evidence is authoritative only after exact-head CI succeeds.

## Compare alternatives against the same product goal

| Candidate | Correctness at 33/100/10k | Resource/long-term implications | Recommendation |
| --- | --- | --- | --- |
| A — existing loaded-only UI grouping | Accurate **only as loaded**; cannot guarantee discovery of hidden Open records | No backend work, but violates the intended actionable global view | Reject as the primary accepted #107 outcome; retain as explicit fallback only |
| B — use linked native Record Query with status-scoped live continuation | Correct matching and honest partial/exhaustion; no exact counts or snapshot; distant Open requires repeated scan budgets | Reuses #161, avoids duplicate stored state, survives existing SQLite format, compatible with future separate Trash lifecycle | **Recommended initial integration**, subject to owner UX decision and 10k latency evidence |
| C — Todo-owned materialized status index/projection | Can make Open discovery efficient **only if** update/delete/reopen are atomic with projection and history remains coherent | Separate state and migration/recovery, complex ownership and revision fencing, index parity across Memory/SQLite; currently **not** an accepted atomic mutation primitive | Defer pending measured need; avoid an inconsistent second authority |
| D — provider-native maintained status access path/index | Potentially efficient; requires *verified* exact predicate, index plans, update maintenance, controlled migration and SQLite/Memory compatibility | SQLite index migration requires offline operator/owner exclusion contract; a generic physical-index API is not implied by logical Query | Investigate as an **independent performance profile**, not silent startup DDL |
| E — eager full enumeration and UI grouping | Eventually finds existing entries in quiescent store, but not a snapshot; can temporarily miss records modified behind cursor | 313 legacy 32-item pages for 10,000; unbounded startup latency and memory | Reject for regular refresh |

**Bias control:** B is not automatically optimal because #161 exists. B is attractive for semantic correctness and reuse, but worst-case sparse retrieval remains linearly proportional to total stored records; the research must reveal whether the product's desired 10k-case latency requires an explicit index. C/D deserve reevaluation when the workload requires it, while feature creep must not make a Todo-specific index into a universal ECS/game Query engine. The earlier scratch SQL indexed times are not equivalent to actual native provider/HTTP latency.

## Proposed first Todo API boundary (NOT an accepted contract)

Keep the existing `GET /api/v1/tasks?after&limit` unchanged for backwards compatibility with R1/R2 clients. Consider an **additive** status route `GET /api/v1/tasks/query?completed={true|false}&cursor=<opaque>&limit=32`, with `{records,next_cursor,exhausted,inspected}` under the existing snake_case API. Exact route/path spelling remains an implementation choice. Restrict filtering to one supported `completed Eq bool` predicate; do not let clients supply arbitrary model names, field names, SQL/JSON, page budgets or provider references.

- The **Todo domain**, not the Axum route, owns the typed query selector. Inject both `Records<TaskData>` and `ModelRead<TaskData,RecordQueryContract>` into a single `Todo` domain session, backed by one **combined linked SQLite owner**. Existing CLI and R0 constructors must remain compatible; a Query reader should be optional for non-query use.
- Bound all blocking work through the existing `AsyncTasks::spawn_blocking`, preserving current pre-dispatch unavailable versus post-dispatch `outcome_unknown` handling. A Query GET that times out is an unavailable read observation, not a confirmed empty list.
- **Do not loop until a match without a higher-level work limit.** A simple first slice is one `query_page` per HTTP request with a fixed server-owned `scan_budget <= 256`. If that yields unusable sparse-page UX, a *bounded* server adapter may skip up to **four empty partial Query pages** per HTTP request (maximum 1,024 examined records), stopping on the **first nonempty page** or `exhausted=true`. Stopping at the first nonempty page avoids silently skipping rows when underlying single-use continuation identity binds the result `limit` and scan budget. **Four/1,024 is a proposed cap, not benchmark-certified.**
- Never return `exhausted=true` when the underlying provider says partial. Never synthesize a total count from an inspected-candidate count or from `records.length`. Preserve unchanged UUID IDs and decimal-string revisions.
- Keep cursor identity status-specific. Invalid/expired/consumed cursor is an explicit **stale-query** response; the browser can offer a fresh GET from the beginning, **not** silently rerun mutations or reuse an old cursor. A lost HTTP GET response might have consumed a one-use provider cursor. Browser reconciling from the start must not silently discard an unsaved edit.

## Proposed interaction model for owner acceptance

**Recommended choice:** an **Open section first** with the **Completed section collapsed initially**. Each section uses an independent filtered Query cursor and a separate `loaded / partial / exhausted / error` state. Prefer this over simultaneous `All/Open/Done` tabs once the sections reliably express their semantics; do not claim a chronological `Recently completed` label without a timestamp or ordering contract.

1. Initial page prioritizes Open; Completed may remain unloaded while collapsed. A section may show `No matching tasks in the checked records; continue checking` on an **empty partial** page, but only `No open tasks found on the completed scan` when genuinely exhausted. Explain the live/non-snapshot limitation through an available refresh, not misleading precision.
2. Default status values use no fake global totals. Say `N shown`/ `N loaded`; for unopened Completed sections do not show an inferred total count. `Select all` means **only visible, loaded, explicitly selected tasks**, not all backend matches.
3. Completing/reopening moves the task to the other section **only after server confirmation**. Reconcile both relevant section cursors on subsequent refresh because status can change behind an already-scanned UUID. Protect keyboard focus, active inline drafts and scroll positions; keep existing revision conflict, Undo and partial-batch failure semantics. A lost mutation reply leaves state uncertain and requires explicit reconciliation.
4. A collapsed section can expire its provider cursor after five minutes; re-expand with a fresh Query instead of assuming old continuation survives. Browser reconnects should not cause the application to shut down.
5. Future Trash (#119), Archive and lifecycle transitions remain **separate semantics** from `completed`. Do not add a second query engine or infer deleted/archived tasks from completion; record the intended path to a versioned lifecycle predicate separately.

## Decision questions that remain product-owned

1. **UX:** Accept Open-first plus initially collapsed Completed, and replace current All/Open/Done tabs? Or retain tabs with independently backend-scoped paging?
2. **10k sparse latency:** Is an honestly partial, bounded scan with explicit `Continue checking` acceptable for the first increment, or is discoverability within a separately specified latency/interaction target mandatory before launch? If the latter, research indexed provider execution and offline migration before committing UI behavior.
3. **Initial API:** Accept additive status-scoped read route and separate opacity/budget semantics, with no total counts and no timestamp-based ordering? The exact path and naming can remain implementation details.

**After recorded acceptance in #107:** update the Todo product definition and, only if a durable new contract is needed, its local HTTP contract. Close specification #107 with the decision and create one coherent implementation issue for domain/HTTP/UI/E2E. Tests must cover 33, 100, 10,000 sparse runs, mutation behind cursor, expired/lost token, completed-collapse keyboard/focus/scroll and batch/Undo/conflict behavior, live persistence restart, and exact-head Linux + browser Chromium CI. Do not open a production implementation PR merely because this research PR's tests pass.
