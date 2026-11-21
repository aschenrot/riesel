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

## Reproduced outcomes and comparison — 2026-11-21

The [exact-head research CI #44916963789](https://github.com/aschenrot/riesel/actions/runs/44916963789) passed **5/5** tests on a pinned Rust 1.100.0 Linux runner. The preceding [browser CI #44916559745](https://github.com/aschenrot/riesel/actions/runs/44916559745) passed **41/41** real Rust/SQLite Chromium scenarios, including the 33-task screen capture.

| Scenario | Verified outcome |
| --- | --- |
| 33 tasks, first 32 Done | First Open-only page returns zero matches but is **not exhausted**; 33rd record is Open |
| 100, Open at UUID positions 70/98/99 | Two 32-record pages find zero Open; all four pages eventually find three |
| 10,000, only last UUID Open | Eight pages inspect 256 with no Open and remain partial; complete traversal requires **313 pages** |
| Mutation after a cursor has passed a task | Reopening that task behind the cursor is **not included** in continuation; revision checking independently prevents stale overwrites |

**Single-run observations, not a latency SLA:** at 10,000 records, a complete in-process scan took **38 ms** with MemoryStore and **657 ms** with linked SQLite. These timings omit HTTP round trips, task/UI rendering, and actual concurrency load. Creation/seeding times are separate.

The SQLite scratch-index test used the database *seeded through the real linked provider* but issued SQL directly after releasing the owner. For the final Open task, the median of 11 warmed SQL-only samples was **12,297 μs** without an index versus **42 μs** after a temporary expression index was created. `EXPLAIN QUERY PLAN` changed from `SEARCH records USING PRIMARY KEY (model=?)` to `SEARCH records USING COVERING INDEX todo_status_research_idx (model=? AND <expr>=?)`. These numbers exclude provider/transport integration and do not establish an operational index-migration pathway.

### Interpretation and provisional direction

- **Loaded-only grouping** is acceptable only if openly labeled as incomplete; it does not fulfill a globally actionable Open-first list at the chosen growth horizon.
- **Eager full scans on each view** are inappropriate as the default large-list solution: sparse matches force 313 pages at 10,000, with no consistent snapshot and a potentially lengthy browser request chain. Per-request bounded scans retain correctness of their `partial/exhausted` state but cannot guarantee promptly finding distant matches.
- **Status-scoped cursor reads at the Todo boundary** are the leading *product-facing* semantics. A query must return actual matches in stable ID order, a cursor scoped to the selected predicate, bounded work and honest exhaustion. Under concurrent mutations, avoid promising a snapshot or global exact count unless supported by a separately verified consistency mechanism. Refresh/restart should reconcile cursor drift.
- **Provider-integrated filtering with an optional index** is promising for read efficiency; its ownership must be researched across linked SQLite, memory and potential managed providers. Do not introduce direct SQLite SQL in Todo's browser handler or unversioned index migrations. Evaluate a reusable, typed/versioned query capability when cross-application reuse justifies its contract and provider parity.
- **Future Trash compatibility** means a query design should not freeze a bespoke boolean-only public Storage API. Treat completion, lifecycle location and query cursor provenance as distinct semantics. Ctrl/Cmd selection, batch changes and Undo must remain revision-fenced and predictable across section transitions.

**No architecture is accepted by these experiments.** They identify the UX guarantee and rule out misleading empty states; owner review is still required before changing a public query contract, physical index or persistent task model.

## Shared provider-instance selection research (2026-11-21)

The accepted component model and Counter/Observer implementation already
demonstrate that separate versioned contracts *can* share one state instance.
However, the provider resolver binds requirements independently. An identical
provider **definition** does not imply that two requirements target the same
**running instance**.

The research test in `examples/counter/app/src/tests.rs` constructs one
two-capability application and two preflight plans, each with identical pinned
provider metadata. In the first, writer/query use **different named managed
instances**; in the second, both use the **same named managed instance**. It
asserts that the existing planner accepts and distinguishes the two selections
rather than requiring co-location automatically. It does not launch an
executable, prove runtime shared state in the split case, or test hostile
operator-owned TCP services. Existing Counter tests separately exercise
write-through-observer on one local component and one managed instance.

This is an important constraint for an additive query capability: merely
exposing `RecordStoreContract` and `RecordQueryContract` from one component
type is **insufficient** if the application can select them independently.
Any accepted query+write co-location guarantee needs explicit preflight
validation of authoritative **instance identity** (including named managed
instance keys), consistent provider/contract pins and a refusal to continue
when they differ. Do not infer authenticated peer identity from a remote TCP
address alone. A single versioned Storage contract avoids this particular
cross-capability pairing constraint, but imposes its own compatibility and
migration costs.

**No pairing enforcement API is authorized by this test.** The owner must
accept whether the contract is a separately versioned query interface plus
verified same-instance selection, or an updated unified Storage contract.

## Co-located read/write contract spike

An independently versioned **test-only** `research.todo.records.status@1` query and unchanged `riesel.storage.records@2` mutation contract are bound to **one authoritative Arc-backed component state**. The actual linked MemoryStore and SqliteStore providers are tested separately. The handler creates Open/Done records, observes writes immediately through the query, reopens Done by revision-checked update, and rejects stale overwrites without hiding the changed records.

The query implementation deliberately scans existing UUID pages. It proves typed capability composition and linked-provider state sharing, **not** index efficiency, stable query continuation, a production API, managed SQLite support, or preflight enforcement of query/write co-selection. Those remain separate acceptance gates.

## Decision gate

Before implementing UX groups, choose one complete semantic model:
- Does `Open` mean all currently queryable matches, or only loaded UUID pages with explicit `loaded only` phrasing? A partial read cannot justify a global-empty state.
- What is the bounded continuation contract, including empty matching pages, cursor progress, exhaustion and page count? How should changes behind the cursor be reconciled (refresh/restart)?
- What does Select all mean when Open and Completed sections are separately paged or collapsed? How is keyboard focus preserved after Complete/Reopen/Undo?
- Do an application-specific status view and its future acceleration require a new transport-neutral capability, or can an app adapter guarantee semantics on existing Riesel abstractions?
- How will lifecycle state for future Trash (#119) extend filtering without double-maintained identity or an unsafe storage migration?

No generic `RecordStore` contract, SQLite physical index, model evolution or Open/Completed UI is authorized by this investigation. Status-query specification remains in [#107](https://github.com/aschenrot/riesel/issues/107), and accepted architecture belongs in the product/contract docs after owner review.
