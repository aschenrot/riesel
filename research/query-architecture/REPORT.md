# General Query architecture — evidence and cross-domain assessment (#138)

**Research date:** 2026-11-22 (Europe/Berlin). **State:** **interim comparative research**, not an owner-accepted architecture, completed platform-wide screening or implementation.
**Code/contract baseline:** `aschenrot/riesel` `main` `93b5188d122f5fa35a3bf047d6d068995c1d74b5`; [spec #138](https://github.com/aschenrot/riesel/issues/138); [plan](PLAN.md); [discovery log](DISCOVERY.md). Other research cited below is **not merged into this baseline**.
**Audience/outcome:** Rust application authors requiring honest selective reads over capability-owned data, starting with Todo. Whether other users need one shared cross-domain Query interface is **not yet an accepted requirement**.

## Method and evidence classification

Candidates were collected in the separate [discovery log](DISCOVERY.md),
which records public upstream documentation for **16 discovered mechanisms
and comparators**; the discovery categories are not a mandatory shortlist.
Read `README.md`, `CONTRIBUTING.md`, `docs/project/delivery.md`,
relevant accepted Riesel product and authoring/Storage/lifecycle
contracts, the #107 research branch, and independent RunenECS architecture.
Cross-repository Runenwerk ADRs 0017/0018 were consulted as
**external comparison**, not Riesel acceptance.

The terms below distinguish:

- **Verified Riesel baseline:** current `main` source and accepted
  documentation; explicit limits are not performance guarantees.
- **Existing Riesel research evidence:** #107 / PR #130 exact-head
  tests, not a production filtered-query implementation.
- **New independent experiment:** the isolated `sqlite_live_keyset.py`
  scratch SQLite probe described below.
- **Upstream API description:** official project documentation describing
  another project's facilities, not validated Riesel applicability.
- **Architectural inference:** a plausible design consequence, still
  subject to maintainer decision and verification.

No Riesel public Query API, provider-native filter, GraphQL server,
DataFusion dependency, index migration or generalized ECS adapter was
implemented in this PR.

## Exact baseline and functional gaps

1. `RecordStore` has stable published identity
   `riesel.storage.records@2` and five operations: create/get/list_page/
   update/delete. `list_page(model, after: Option<Uuid>, limit: u64)`
   uses UUID order, limit 1–32 and **non-snapshot** continuation.
   There is **no server-side status predicate**.
2. `Records<T>` validates `RieselModel` values and supports
   `list_page`; its `list()` convenience enumerates potentially
   all records and warns against large collections.
3. Registered model fields include `String`, `Bool`, `Uuid`,
   `U64`, `I64`, `F64`, `Json`, `Optional`, and `List`.
   The validation code specifically accepts a **missing optional
   property and explicit JSON null**. It does not specify query-operator
   equivalence between those two forms.
4. SQLite uses a **single owned connection** and checks owner fencing,
   exact registered model definitions, UUID-as-BLOB ordering,
   u64 revisions as big-endian bytes and current offline migration rules.
   A native SQL index or generated column has its own durability/upgrade
   contract and does not follow automatically from the type metadata.
5. Application capabilities are **versioned typed contracts**;
   component definition, provider, instance, requirement alias and
   application session identity are different things. Linked, managed
   and external placements do not share implied authentication or
   ownership guarantees. One component can expose multiple contracts
   with common state; two separately selected provider instances
   need not share anything.
6. `#[query]` means a **read-designated capability operation**,
   not an existing general predicate IR, query planner or transactional
   read language. Riesel's execution graph is a different authority.

This verified baseline is a basis for experiments, **not** a new
platform-wide Query product acceptance.

## Evidence inherited from Todo, with its limitations

[Draft #130](https://github.com/aschenrot/riesel/pull/130) records
reproducible Memory/linked-SQLite research at head
`958d4f2e087311ebef1e1cdcae0ac1dc3543c59f`, with
[12/12 research tests](https://github.com/aschenrot/riesel/actions/runs/44963419374)
and separate linked workspace/browser validation at that revision.
This report **does not rerun those tests**. The relevant claims are:

- When UUID pages contain no filtered matches but later records do,
  a filtered frontend cannot report a globally empty result.
  10,000 records with only the last UUID open require **313**
  32-record pages for full v2 traversal, even without RPC/UI costs.
- A *scratch* SQLite expression index improved an SQL-only selective
  predicate for that fixed case; the measurement is **not** a
  provider-native Riesel indexed Query API, benchmark SLA, or approved
  physical index upgrade. Generalizing it to range, text or optional
  fields requires independently specified semantics.
- A scratch SQLite JSON comparison between `u64::MAX` and
  `u64::MAX-1` demonstrates an unsafe potential for full-width numeric
  comparison through `json_extract`; missing versus explicit
  JSON null is also collapsed by `json_extract` unless presence
  is queried separately. Existing v2 has **no** such filtered
  operation; do not describe v2 as already incorrectly filtering.
- Read/query and writer must address the **same authoritative
  state** if a consumer promises read-your-writes. Two named managed
  placements can resolve to separate instances despite similar
  definitions; advertised metadata is not authenticated identity.
- Two revision-fenced writes are not a multi-record transaction.
  Large asset blobs, world checkpoints and ECS frame-local borrowing
  are not solved by record-filter API expansion.

The Todo research's prior inclination toward additive Record Query
is a **candidate hypothesis only** under #138. It is not inherited as
an architectural conclusion.

## New targeted experiment: indexed live keyset correctness

**Reproduction:** `python3 research/query-architecture/probes/sqlite_live_keyset.py`.
The research script uses only Python's stdlib `sqlite3`, an
**in-memory scratch DB**, `CREATE INDEX ... (priority,id)` and
`SELECT ... WHERE (priority,id) > (?,?) ORDER BY priority,id`.
It is isolated from Riesel's provider and has no production migration.
Test environment for the initial run: Python `sqlite3` linked to
**SQLite 3.46.1**; **3/3** deterministic assertions passed.
The dedicated research CI workflow reruns the same fixture on its
own runner; its version and run must be recorded separately.

| Deterministic case | Observed result | Inference / limit |
| --- | --- | --- |
| No inter-page mutation | `a,b,c,d` once in order | Baseline demonstrates the expected keyset result in this scratch DB, not a snapshot contract |
| Seen record `a` changed from sort priority 1 to 5 after page one | `a,b,c,d,a` | **Duplicate**: a mutable sort key can move an already seen row past the cursor |
| Unseen record `d` changed from priority 4 to 0 after page one | `a,b,c` | **Omission**: a mutable sort key can move an unseen row behind the cursor |

**Decision consequence:** Even indexed, provider-native keyset pages
do not imply an exactly-once, complete, point-in-time ordered traversal
when the sort key changes between calls. A query contract must state
live/non-snapshot behavior, or explicitly provide a correctly owned
snapshot mechanism; an opaque cursor alone cannot make a snapshot.
A stable immutable sort key avoids this particular movement but not
all concurrent record-membership/filter changes. This experiment
is **not** a Riesel native-query parity, throughput, cancellation
or transaction test.

## Comparative candidate synthesis — derived *after* discovery

These are **design families, not mandated alternatives**. Each remains
open to newly discovered hybrids. The same intended immediate outcome
is **correct, bounded filtered reads over a registered durable model**;
broader read workloads are evaluated as optional growth, not
requirements invented to force one candidate.

| Discovered design family | Immediate correctness / authority pathway | Reuse, evolution and implementation cost | Status of material gap |
| --- | --- | --- | --- |
| **Current typed domain capability or Todo-owned adapter on v2** | Can honestly report partial pages and scan budgets; owner remains Todo, existing v2 intact. Efficient selective pages are not established. | Zero new published platform abstraction but repeat scanning or build app-specific index and cross-backend maintenance. | **Viable baseline**, filtered global completeness and bounded work under sparse matches require explicit design |
| **Optional shared query authoring/validation helpers; provider/domain APIs stay independent** | Can unify operator vocabulary and type checks while each owner truthfully specifies its consistency/ordering, possibly using its own backend queries. | Low runtime coupling; portability of query *semantics* and codegen may still diverge per provider. | **Viable candidate**, measure duplication and check if consistent SDK results are required |
| **Versioned typed read/query capability, executed by authoritative provider** | Potential common schema/operator/cursor/error semantics; enforce bound same-state writer/query and no implicit mutation permission. | Reuse across registered-model applications without universal execution; needs support negotiation, runtime validation, managed/export lifecycle and correct provider-native adapters. | **Viable candidate**, not accepted; identity/authorization and type-precision costs substantial |
| **Versioned unified record interface with read/query operations** | One provider handle chooses both reads and writes, reducing same-instance pairing burden; co-expose v2 during transition. | Broader versioned API and potential read-only consumer permission problem; possible compatibility/migration costs. | **Viable candidate**, compare security facet and API authoring costs rather than assuming separate Query wins |
| **Reuse existing SQL DSL, ORM or typed SQL macro *within provider*** | SQL engine already handles many ordinary predicates/indices; record-schema full-width numeric/null semantics must still be proven and app/auth boundaries enforced. | Avoids new low-level SQL AST builder; ties implementation to DB dialect and schema mapping; doesn't itself establish a Riesel protocol. | **Potential implementation choice**, not automatically competing with a common logical contract |
| **Relational IR/engine or federation (Substrait, DataFusion, similar)** | Expressive query planning if relational/analytics functionality becomes a genuine outcome. Transactional snapshot and user authorization still need Riesel-owned contracts. | Extra planning, memory/CPU, Arrow/schema conversions and engine lifecycle; may provide large reuse if heterogeneous analytics is accepted. | **Growth candidate / unknown**, no accepted need or conformance proof for first record-filtering delivery |
| **GraphQL-style API projection, search index, local lazy frame or incremental view** | Each solves a distinct selection/search/batch/live-update concern through owner-defined semantics and materialization. | Potentially useful optional adapters; forcing them into ordinary CRUD query is neither shown necessary nor safe. | **Distinct-product comparators**, reconsider if user product requirements demand them |
| **ECS/world or spatial engine-owned query** | Preserves borrow/epoch, thread access, spatial-index and frame-state authority under specialized runtime. | Optional reflective or inspection adapter could translate a *bounded projection*, not arbitrary World borrows. | **Domain boundary comparator**; no authority to replace RunenECS query |

Independent sources and precise claims are catalogued in
[DISCOVERY.md](DISCOVERY.md). No performance head-to-head or license/MSRV
audit has yet been executed for these family-level alternatives.

## Cross-domain and game-engine pressure test

An editor may need to find material assets with selected tags, sort
a metadata catalog and display bounded results; this resembles the
registered-record Query case **if metadata's authoritative owner and
schema/consistency guarantees are defined**. Full asset-byte transfer
and live GPU residency are separate concerns.

A game simulation may query mutable `(&mut Position, &Velocity)` under
ECS borrow and scheduling rules; forwarding a database-style page
cursor to it is not safe or sufficient. RunenECS's own architecture
assigns World, components, relation stores, and dense/epoch-scoped
queries to RunenECS. A query **inspection view** of game metadata might
be feasible through a separately authorized, explicitly versioned
projection but does not grant Riesel execution authority over the ECS.

A game save or editor checkpoint needing coherent multi-component/
multi-record state cannot be inferred from a sequence of live query
pages. A transaction/snapshot capability would require its own
owner-defined consistency boundary. It may compose with a query API;
it is not equivalent to one.

## Architectural risks, bias checks and what would change the choice

**Short-term patch bias:** a Todo-only workaround may create repeated
semantic drift for other Rust-derived model applications. A fair
comparison must account for the *cumulative* cost of duplicate predicates,
index changes, invalid cursors, and SDK versioning across credible consumers,
even if new shared infrastructure costs more initially.

**Speculative abstraction bias:** 16 upstream mechanisms do not imply
16 required Riesel features; neither does a shared name `query` justify
an AST/engine. A generic contract must improve a real multi-consumer
outcome without misrepresenting each domain's authority/consistency.

**Prior-result bias:** PR #130 favors additive Record Query and existing
SQLite. The report deliberately keeps alternative integrated/faceted
record interfaces, domain APIs and provider-local SQL builders alive
until ownership, read-only consumer and type-parity evidence distinguish them.

**Selection bias:** sources include well-known Rust ecosystem projects,
but not a systematic full market/library search. Independent extension
must remain possible. No scoring matrix pretends to rank unavailable
license, toolchain, resource-budget or runtime tests.

**Hidden consistency bias:** indexing/typed DSLs improve expressiveness
or access paths but do not create a snapshot. The new keyset experiment
is a concrete falsifier of that implied guarantee.

**Security/identity bias:** matching provider IDs, definitions, named
placements, loopback endpoints or Rust type signatures are not
authentication/authorization of independent readers or proof of
shared authoritative state.

### Current outcome: do not accept a platform-wide API yet

**Evidence supports researching** a shared contract for registered
persistent-record selection, and preserving domain-owned execution
boundaries. It does **not yet justify selecting one universal Query
representation/engine or freezing the Record Query-vs-Storage v3
interface**. That is an evidence-limited **defer architecture
selection** result, not a conclusion that a shared foundation is
undesirable.

The decision *could* change with:

1. A real second registered-model consumer proving equivalent typed
   operations, not merely contrived analogies to Todo;
2. A contract pressure test of complete `u64`, `i64`, missing/null,
   string/UUID ordering, operator rejection, projection and cursor/
   version/restart behavior for concrete interface variants;
3. A measured/practical comparison of **same-instance binding and
   read-only authorization** versus co-exposed unified storage;
4. Real bounded-selectivity/cancellation behavior on linked Memory/
   SQLite, with managed placement tested before claiming support;
5. If broad analytics, search, federation or ECS adapter becomes an
   accepted requirement, its distinct owner/consistency/performance
   scenario and decision-changing evidence.

**Provisional delivery guidance, not implementation authorization:**
Todo can fix honest loaded/partial UI behavior independently of a
platform query contract. A reusable indexed read capability, if
accepted later, must ship against precise Rust-derived model semantics
and separate provider parity/authorization gates. No game engine,
federation engine or analytics runtime is a prerequisite for Todo.

## Reproducibility and completion criteria

- Riesel baseline: source revision stated above; #107 tests and CI
  are linked rather than silently rerun or copied into this PR.
- Scratch probe: `python3 research/query-architecture/probes/sqlite_live_keyset.py`;
  Python stdlib only, deterministic seed, in-memory database and
  no user filesystem side effects. Initial SQLite **3.46.1** run
  asserted all **3/3** outcomes; GitHub research workflow result
  must be linked after the exact changed SHA completes.
- Comparative claims: upstream documentation links in
  `DISCOVERY.md`. **Not verified** here: legal licensing audit,
  upstream package MSRV at pinned Rust 1.100, actual integration
  into Riesel, performance SLA or remote authorization.
- The investigation should stop (and a final recommendation be
  submitted to #138) **only after** the decision-critical interface
  ownership/semantics tests distinguish the viable options. If
  the missing evidence remains material, document a precise deferral,
  not an invented winner.

**No owner accept/revise/defer/reject decision has been recorded by
this research report; #138 remains open and this PR remains draft.**
