# Todo status-query investigation — #107

**Research plan — not an accepted public API.** Source: main `59f18eb3ab9f02510cf65c002953d4bde393d11e`, 2026-11-21.

## Outcome and growth horizon

Users should see Open tasks first and a collapsible Completed section with honest empty states, keyboard/touch parity, stable selection and Undo, and acceptable behavior at 33, 100, and 10,000 tasks. Current Todo R2 pages by **UUID** using Storage v2 `list_page` up to 32 records, with no status query or snapshot guarantee. A visually empty first-page Open list must not be presented as globally empty.

## Common comparison criteria

All candidates must preserve application-owned task semantics, stable IDs and revision conflicts; page bounds; accurate `partial/exhausted` signals; correct behavior during status changes; multi-client/restart behavior; provider portability and RPC frame limits; predictable latency and resource budgets; and an evolution path to 10,000+ records. Compare **long-term cumulative costs**, not just first implementation effort.

| Candidate | Strongest property | Key trade-off / research |
| --- | --- | --- |
| Group only loaded tasks | No backend changes | Cannot claim global Open/Completed completeness; first page may hide only Open task |
| Fetch all UUID pages before rendering | Can fully enumerate quiescent dataset | 313 sequential reads for 10,000, unbounded startup and no snapshot |
| Todo-owned bounded scan continuation | Works with existing provider contract, explicit resource bound | Sparse match may require many round trips, empty partial page is not global empty, no snapshot |
| Todo-owned indexed projection/query | Potential efficient status-specific pages | Reindex atomicity with task mutation, provider parity, lifecycle/migration and revision semantics |
| Versioned reusable typed query capability | Reuse across applications/provider types | Contract/protocol/index semantics and migration cost must be justified by broader needs |
| Provider-specific SQLite JSON index | Potentially excellent local read complexity | Physical schema/index maintenance is outside the accepted typed model-only migration contract and introduces provider lock-in |

## Executable evidence

`examples/todo/tests/status_pagination_research.rs` uses the authoritative `Todo` domain on both the in-memory and **real linked SQLite** providers. It populates valid registered task data, sorts real generated UUIDs and positions Open tasks deliberately.

- 33 records: first **32 Done**, only 33rd Open; first page has no Open but is not exhausted.
- 100 records: Open in UUID positions 70, 98, 99; two page reads scan 64 records and yield no Open even though three exist.
- 10,000 records: only last UUID Open; eight pages scan 256 records without a match, full scan requires **313** pages.
- Concurrent mutation between pages: after the first cursor advances, another client changes a task behind the cursor to Open. Continuation cannot see it; revision-based updates remain fenced.

`.github/workflows/todo-status-research.yml` runs those tests with `--nocapture` and prints **observed** per-provider setup and scan timing in CI. Timing is machine- and load-dependent; do not turn one CI number into an SLA. This is deterministic *logic* evidence for non-snapshot behavior, not a latency-certified index alternative.

An additional browser integration fixture `examples/todo/web/e2e/status-query-research.spec.ts` seeds 33 records through the accepted local HTTP API with the first 32 UUIDs completed. It confirms the current Open tab displays its **loaded-page-only** placeholder while the sole Open task remains behind `Load more tasks`, then appears after cursor continuation. A screenshot records the partial empty state. This fixture is evidence of current behavior, not an acceptance test for a redesigned list.

## Read-only SQL predicate / temporary-index comparison

A fifth, **scratch-database-only** Rust test uses the same SQLite file seeded by the actual linked Todo provider at 10,000 records, then releases the provider's cooperative owner. It measures a `json_extract(CAST(data AS TEXT),'$.completed')` status predicate with UUID ordering, before and after creating a temporary expression index in that **isolated research database**. It reports SQLite `EXPLAIN QUERY PLAN` and the median of 11 in-process SQL-only timings, and asserts result identity. This compares query strategies but does **not** implement an accessible Riesel capability, include app transport/session costs, or establish an approved physical migration procedure for user databases.

## Decision gate

Before implementing UX groups, choose one complete semantic model:
- Does `Open` mean all currently queryable matches, or only loaded UUID pages with explicit `loaded only` phrasing? A partial read cannot justify a global-empty state.
- What is the bounded continuation contract, including empty matching pages, cursor progress, exhaustion and page count? How should changes behind the cursor be reconciled (refresh/restart)?
- What does Select all mean when Open and Completed sections are separately paged or collapsed? How is keyboard focus preserved after Complete/Reopen/Undo?
- Do an application-specific status view and its future acceleration require a new transport-neutral capability, or can an app adapter guarantee semantics on existing Riesel abstractions?
- How will lifecycle state for future Trash (#119) extend filtering without double-maintained identity or an unsafe storage migration?

No generic `RecordStore` contract, SQLite physical index, model evolution or Open/Completed UI is authorized by this investigation. Status-query specification remains in [#107](https://github.com/aschenrot/riesel/issues/107), and accepted architecture belongs in the product/contract docs after owner review.
