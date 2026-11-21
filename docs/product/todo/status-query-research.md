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


## Decision-ready interface comparison and preflight research (2026-11-22)

This section is an **architecture recommendation for maintainer review**, not
an accepted platform contract. The decision is against the **same Todo and
future-app outcome**: reliable filtered reads of authoritative, revisioned
records on linked and supported managed providers, with a credible path to
future lifecycle/Trash predicates and read-only clients.

| Alternative | Correct sparse filtering / growth | Provider ownership | Compatibility and evolution | Disposition |
| --- | --- | --- | --- | --- |
| Client grouping of 32-item UUID pages | Cannot ensure an Open task is visible; false global empty states possible | No new selection | No migration, but UX outcome fails | Reject as final grouped design |
| Todo-owned bounded continuation over v2 | Can be honest but may require 313 pages to find one 10k-record match; no snapshot | Uses existing record handle | Smallest initial change, continued read amplification | Fallback only for bounded partial views |
| Todo-owned direct SQLite filter/sidecar | Fast for local SQLite; bypasses portable model/provider boundary or needs synchronized index writes | Must prove same SQLite owner and atomicity | Coupled to Todo/SQLite, difficult managed or memory parity | Do not use as implicit shortcut |
| Unified RecordStore v3 with filtered reads | Native provider query; stable cursor possible | Query/write pairing intrinsic **when all operations use this one handle** | Requires v3 client/provider adoption and a plan to retain v2; read-only query clients also receive a write-capable interface | Valid competing architecture |
| Separate versioned Record Query capability co-exposed with v2 | Native provider query; stable cursor possible | Requires explicit same-authoritative-instance constraint for query+write applications | Retains v2 callers; query-only requirements are independently versioned, with extra pairing/hosting tests | **Preferred if pairing is enforceable** |

Do not misstate v3 as necessarily breaking all v2 clients: a provider could
expose both versions. Conversely, an additive query capability is not free:
one additional published interface, validation relation and provider tests
are long-lived maintenance obligations.

### Proposed first supported semantics (not frozen API names)

1. Filter a registered Rust-derived model by a bounded conjunction of typed,
   schema-validated **equality predicates** on supported scalar fields.
   Reject unknown model, field, unsupported field type, wrong value type,
   invalid predicate shape or excessive predicate count before querying.
   Neither a general SQL/JSONPath expression nor arbitrary client-provided
   SQL is included. Support `completed: bool` initially and an independent
   future Todo lifecycle field without changing the query principle.
2. Return 1–32 matching records per request, in ascending immutable UUID/ID
   order, with original record identity, revision and validated value.
   **A zero-match first page must not mean “no matching tasks” if a
   continuation still exists.** Prefer native bounded match pages rather
   than a chain of empty filtered views.
3. Treat the cursor as an opaque token associated with model, canonical
   predicate, ID ordering and an authoritative query session/instance
   generation. Reject wrong-filter, malformed and stale-generation tokens.
   It is a continuation position, **not** a transaction snapshot. Changing
   status behind the cursor may require full refresh/reconciliation.
4. Preserve Riesel's revision fencing and uncertain-outcome rules; neither
   a query response nor a failed HTTP reply permits blind write replay.
   Do not promise exact global totals or chronology. The UI can identify
   loaded matches and true current-page exhaustion, not a permanent
   all-time count.
5. Provide behavioral parity for Memory and linked SQLite; no second task
   database, independent sidecar, or unreviewed on-startup index migration.
   Start with provider-native SQLite filtering without a new physical index
   if **integrated** latency/resource tests pass. Any later provider-managed
   index requires migration, rollback and write-atomicity verification.
6. Initial hosting should match synchronous Storage v2 when co-hosted on one
   managed endpoint. Do not disguise a blocking SQLite call as native async.
   The first verified scope may explicitly limit SQLite to linked mode;
   managed SQLite parity must not be claimed without actual tests.

### Required query/write ownership relation

An application binding both a writer and a query for the **same data source**
must explicitly declare *same authoritative instance* as a constraint
independent from startup dependency ordering. It must be checked after
provider selection but **before** creating a local factory or starting a
managed child:

- **Linked:** same registered local component/provider identity is eligible.
- **Managed:** only an explicitly shared instance name and compatible
  pinned provider definition/launch configuration are eligible at preflight;
  subsequently verify actual startup/session and both interfaces on that
  child. Unnamed aliases or split names are not shared.
- **External TCP:** advertised provider metadata or the same loopback
  address cannot authenticate server or instance identity; fail closed for
  an authoritative pairing claim until a separately accepted #75 trust
  mechanism exists.

A query-only consumer need not declare a writer pairing. Do not impose
read/write privileges on a read-only client merely because the provider
implementation supports both. The pairing invariant must also cover
multiple model-to-storage aliases and recovery after whole-session restart,
not just one hardcoded Todo example.

A new research-only test in
`examples/todo/tests/query_capability_research.rs` exercises a **candidate**
plan validator for shared/split/unnamed managed requirements and shared
linked providers. This is not a public runtime enforcement mechanism, does
not launch an actual managed query provider, and does not authenticate a
remote process. Its exact-head CI status must be checked separately.

### Acceptance tests to require before production integration

- Reject a split/mismatched query+writer plan before either factory/child
  starts, including named-instance and provider-pin mismatches. Prove
  successful real managed co-host startup and read-your-writes, then reject
  stale session bindings after managed crash/restart.
- Exercise invalid schema/type/predicate, wrong-filter or malformed cursor,
  stale generation and page bounds on Memory and SQLite with equivalent
  results and error classes.
- Exercise 33, 100, 10,000 sparse records, changes behind the cursor,
  concurrent conflicting revisions, index-free latency, and repeated
  completion/reopening without a false global-empty state.
- Verify Todo HTTP first/middle/exhausted pages, separate Open/Completed
  cursors, accessible collapse/focus transitions, Undo and loaded-only
  multi-selection; refresh after cross-client changes. Preserve R0/R1 CLI.
- Test clean upgrade with existing SQLite files **without** physical
  mutation. If an index is later selected, test explicit migration,
  backup/rollback, interrupted writes and schema compatibility under
  established offline ownership rules.
- Prove that future Trash can be filtered independently from completion
  without turning permanent deletion into a browser-only delayed remove.

### Decision / delivery boundary

**Recommend:** additive typed Record Query capability, with declarative
same-instance pairing and fail-closed supported targets, subject to the
above guarantees. **Fallback:** unified opt-in v3 Storage contract if
declarative pairing cannot be made simpler and more reliable than versioned
migration/authorization costs.

The specification owner must explicitly accept or revise that trade-off;
the research PR must remain draft until then. Upon acceptance, record
invariants in the authoritative product/architecture documents and split
implementation along independently testable boundaries: provider/query
contract + pairing; Todo domain/HTTP adapter; Open/Completed UI. Do not
merge research-only types as production APIs.


## Cross-workload architecture and bias review — 2026-11-22

**Research extension, not a change to the accepted platform product scope or a
public query API.** The motivating user outcome remains correct Open-first Todo
with a scalable reusable path for schema-registered application records.
The wider comparison is a *stress test of the proposed abstraction* against
credible application and editor/game persistence needs, not an authorization
to develop a general game engine, full query language, or transaction service.

### Authority and source of future requirements

- `docs/project/delivery.md`: compare architectures against a plausible
  growth horizon, reject both short-term example bias and speculative
  abstraction bias, and make public-interface decisions deliberately.
- `docs/product/riesel/riesel.md` and `api-authoring-v3.md`: applications
  depend on versioned capabilities; components retain state and process
  ownership; game/ECS integrations are optional specialized applications.
- `durable-record-storage-architecture-proposal.md` and
  `sqlite-offline-migration-architecture.md`: Storage v2 has a bounded
  linked SQLite realization; physical format/index changes do not happen
  automatically at application startup.
- `execution-plan-contract.md`: one execution owner per nested domain;
  Riesel does not also schedule a delegated ECS's internal tick/systems.
- `dornglut/runen-ecs/ARCHITECTURE.md`: the standalone ECS owns entity,
  component and resource lifecycle, dense storage, borrow-constrained
  queries and system scheduling; not Riesel RecordStore. The owning
  engine's world-persistence adapter may export an explicit checkpoint.
- `dornglut/runenwerk` accepted ADRs 0017 and 0018: useful **external**
  architecture comparators for explicit semantic authority, identity
  mapping, provenance and physical realization. They are NOT Riesel
  product acceptance or a requirement to adopt a common meta-framework.

### Workload matrix: what a Record Query must and must not own

| Plausible workload | Query shape / property | Authority and abstraction consequence |
| --- | --- | --- |
| Todo, settings, user profiles | Model-field equality, bounded ID pages, CAS revisions | **In scope:** reusable Record Query over authoritative typed records |
| Asset catalog and editor discovery | Equality plus likely future range/order/text search, metadata-only projections | **In scope for evolution analysis; not initial delivery:** don't assume equality and UUID order satisfy all future queries; don't couple storage to full-text/index technology |
| Game progression and inventory metadata | Durable reads/updates plus possibly cross-record invariant | **Record CRUD is applicable; atomic multi-record transfers are NOT provided by v2 or solved by a read/query interface** |
| Scene and world persistence | Complete versioned checkpoint, stable identity mapping, large payload or chunk stream | **Different consistency/transfer problem:** choose a domain-owned snapshot/restore or object/blob facility if accepted; do not turn independent record pages into an alleged world snapshot |
| Runtime ECS systems | Dense component tuples, structural epochs, mutable borrows, tick-local access | **Excluded:** RunenECS authority; Record Query's cursor/RPC/JSON semantics are not appropriate inside a frame |
| Physics/spatial and network replication | Spatial predicates, simulation time, prediction, rollback | **Excluded:** specialized domain authority and independently justified contracts |
| Logs and telemetry | Append/stream/range/retention and observer loss | **Excluded from first Record Query:** event-stream semantics differ from model-record pagination |

The meaningful reuse question is *which applications query registered durable
records*, not *which products use the word query*. A provider may implement
several contracts, but no global source of truth, world clock, single
transaction model or shared entity ID follows from that composition.

### Fresh reproducible negative tests

`examples/todo/tests/record_workload_research.rs` runs the unchanged
`RecordStore` v2 against both registered Memory and real linked SQLite with
two **additional** Rust-derived research models. It checks:

1. A game-like two-record inventory transfer issues a committed debit followed
   by a stale-revision credit. A second client's no-op revision bump forces
   the conflict. The first debit remains committed; **two v2 CAS calls are
   not an atomic multi-record transaction**. This says nothing about
   whether a future transaction capability is necessary or where it belongs.
2. An editor asset-metadata model has 65 records; only the last UUID has the
   searched tag. Reading two 32-record pages still finds no match. This
   demonstrates Todo's sparse-search defect on an independent typed model,
   though it is not an indexed query performance measurement.
3. A valid registered model with an 80-KiB text payload is rejected by both
   record providers' 64-KiB stored-record limit, while small asset metadata
   remains valid. A bulk texture/blob facility would require **different
   capacity and transfer contracts**, not a larger query response by default.

These fixtures are tests of **current behavior** and falsification of
overbroad platform claims; they are not a replacement for transaction,
persistent-stream or game-engine integration specifications. CI outcome must
be reported separately from test authoring.

### Growth, capacity and performance interpretation

Current verified sparse Open at 10,000 records needs 313 full Storage v2
pages, and an editor metadata query has the same logical incompleteness
before page exhaustion. For pure UUID pagination, the full-walk page count is
`ceil(N / 32)`: **3,125** pages at 100,000 records and **31,250** at
1,000,000 records, *analytical only, not performance measurements*.
Filter selectivity is a first-class benchmark dimension; checking only common
matches biases toward full-scan solutions.

The current transport frame is **4 MiB** in
`crates/riesel_protocol/src/tcp/wire.rs`, each stored record is capped at
**64 KiB** by Memory/SQLite, and Storage v2 page size is **32**. A bounded
number of records therefore helps transport sizing but does **not** guarantee
bounded query CPU, database lock duration, memory allocation or response time.
Production work must measure predicate evaluation, provider locks, RPC,
HTTP/browser delivery and interference with concurrent mutations.
A one-time scratch SQLite expression-index result is **not** a published
complexity/SLA guarantee.

For a generic indexed query, explicitly decide how providers expose or
refuse unindexed operations that violate an accepted scan/time budget. Do
not promise a universally efficient range query over arbitrary JSON fields.
Physical index evolution must obey independent ownership/backup/migration
rules and keep revisions, identities and view correctness authoritative.

### Schema/value comparator correctness risk

Riesel's `ModelDefinition` supports `Bool`, `String`, `Uuid`,
`U64`, `I64`, `F64`, `Json`, `Optional` and `List` fields. Storage
v2 validates and preserves `u64` values in JSON and stores independent
revision counters as a full `u64`. This does **not** establish that every
SQLite JSON expression is a correct implementation of typed equality.

A new **scratch-SQLite-only**, CI-exercised research fixture in
`record_workload_research.rs` compares `u64::MAX` and
`u64::MAX - 1` under `json_extract`: values outside signed 64-bit range
may be returned as floating point and lose exact integer distinctions.
Likewise `json_extract` returns SQL NULL for both an absent optional
field and a field containing JSON null, whereas `json_type` can
distinguish missing from explicit null. These probes do not demonstrate
that the existing Riesel Storage provider returns wrong records: it does
**not** expose such filtered predicates today.

**Decision consequence:** A generic predicate must define type-specific
canonical representation, equality, null/missing semantics, and provider
parity before accepting numeric or optional equality. For the first
iteration, supporting a smaller set of exactly implementable scalar
predicates is preferable to silently returning wrong matches. Do not
treat SQL's implicit numeric conversions as the public Record Query
contract. Test complete `u64` values, both signs/ranges of `i64`,
UUID normalization and precise optional semantics as separate cases.
The exact first operator/field-type matrix remains unaccepted.

### Public contract evolution risks to resolve *before* freezing shape

1. **Predicate algebra:** The first viable native provider operation may be a
   validated conjunction of equality predicates. Before publication,
   demonstrate that a future comparison/range operator, null/optional
   semantics, sorting and metadata-only projection can be added through
   deliberate versioning without violating previous cursors or requiring a
   data rewrite. They are design scenarios, **not required v1 functions**.
   Reject unsupported field/operator types explicitly; never accept arbitrary
   SQL/JSONPath or silently evaluate the wrong predicate.
2. **Consistency tiers:** Live, non-snapshot continuation is acceptable for
   Todo; it is **not** a coherent checkpoint, fixed-revision dataset or
   multi-record transaction. Query/read-view and snapshot/transaction
   capabilities must not be conflated. Concurrent filter mutation can make
   records appear/disappear between pages; UI refresh/reconciliation is a
   product responsibility.
3. **Cursor identity and privacy:** Bind an opaque cursor to model, canonical
   filter, sort order, instance/session generation and chosen contract
   semantics. Treat it as untrusted input, validate expiration and supported
   continuation, and do not assume untrusted clients have permission merely
   because metadata matches. The final token encoding, possible tamper
   resistance and restart behavior need an explicit threat model.
4. **Cross-capability binding:** Separate query+writer requirements need a
   generic, fail-closed same-authoritative-instance relation; tests for
   named managed and actual start/restart identity are not satisfied by
   comparing provider definitions or loopback addresses. A query-only client
   must not require mutation privilege. This is the clearest additional
   long-lived complexity relative to one versioned Storage v3 handle.
5. **Result sizing and execution:** Decide if a query returns full records
   or a forward-compatible projection. Respect the existing JSON RPC frame
   budget and bounded worker ownership; a synchronous SQLite query cannot
   become native async only by changing a trait signature.
6. **Data/model evolution:** Registered Rust field schemas are the source of
   truth. Persisted model shape and physical index format are separate
   migration concerns. An additional query interface must not silently
   migrate user files, construct second authoritative stores or change
   Storage v2 identity/revision semantics.

### Updated comparison: separate Query vs unified Storage v3

Both candidates can keep `RecordStore` v2 available by co-exposure; neither
automatically forces every existing client to migrate.

| Dimension | Separate, versioned Record Query | Unified v3 record interface |
| --- | --- | --- |
| Existing v2 clients | Unchanged | Unchanged only if v2 is co-exposed; new client binding needs a migration plan |
| Read-only consumers | Query-only contract without write methods | Exposes mutation methods unless a separately scoped read facet is provided |
| Read/write authority | Explicit same-instance binding plus runtime proof | One handle intrinsically selects one instance for its own operations |
| Future query-language growth | Independently version Query | Version Storage surface or add compatible method/argument semantics |
| Physical index/migration | Backend work with unchanged data authority | Same backend work; not inherently easier |
| Operational complexity | Extra interface, routing, selection and pairing tests | Fewer interfaces but broader capability/compatibility surface |
| ECS/asset/scene applicability | Neither is an ECS query or blob/checkpoint API | Neither becomes one merely through a v3 version |

**Provisional finding:** Additive Record Query remains the leading candidate
because persistent metadata/editor/profile queries are credible multi-app
uses and its read-only boundary avoids coupling every viewer to mutations.
However, a unified v3 is still a competitive alternative when pairing and
authorization costs are measured. Neither game-world snapshots nor
transactional inventory transfers justify expanding the **query** interface.
Do not treat the current evidence as a quantitative platform-wide
scalability benchmark.

### Revised decision and verification gates

**Architecture/specification acceptance, before publishing an API:** compare
a concrete, non-binding query descriptor/cursor and versioning strategy for
equality, optional/null values, unsupported operator types and later
range/sort/projection additions. Explain how Storage v2 and old query cursors
are preserved or explicitly invalidated. Evaluate query/write identity
preflight and its implementation cost against the single-handle v3 approach;
no real managed process or production query service must be implemented just
to *choose* an architecture. The additional models and negative tests are
representative stress cases, not a requirement to ship game infrastructure.
The owner/maintainer records the accept/revise/defer decision and the first
supported placement scope.

**Implementation acceptance, after that decision:** require provider-native
matching with linked Memory and SQLite parity, invalid predicates/cursors,
multiple model shapes/selectivity, measured resource budgets, concurrent
updates and rejected split-instance plans before shipping the capability.
Require real managed startup, co-selection, restart/fencing and failure tests
**before claiming managed placement support**; it may be a separate,
explicitly deferred delivery increment. Production performance, transport,
security and index migration claims each require their own evidence. The
Todo domain/HTTP/UI work still needs honest pagination and regression tests.

Record out-of-scope workloads and an evolution policy in the eventual
contract. Do **not** make game-domain frameworks, world snapshots, bulk
asset storage or future transaction services prerequisites of Todo #107.

Keep #107 open and PR #130 **draft research only** until the architecture
decision is recorded under `docs/project/delivery.md`. No generic Query
API, production SQL index, game infrastructure, physical format change or
Open/Completed UI is accepted by this extension.
