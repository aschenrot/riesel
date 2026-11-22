# #138 — Query architecture decision review

**Research status:** recommendation for the owner to **accept, revise or defer**, not an accepted contract or an approved implementation. **Date:** 2026-11-22. 
**Decision source:** [specification #138](https://github.com/aschenrot/riesel/issues/138). **Evidence:** [REPORT.md](REPORT.md), [DISCOVERY.md](DISCOVERY.md), [PLAN.md](PLAN.md), [Todo research PR #130](https://github.com/aschenrot/riesel/pull/130).
**Code baselines:** PR #140 originates at `93b5188`; current `main` `29c90f5` changed execution-plan/runner sources, tests and product docs, not the tested Storage contracts. Research branch merge commit `09c6144` includes that revision. Verify its exact merged-tree CI independently.

## 1. Decision to be made — distinguish the product direction from mechanics

**Product outcome to accept or revise:** Riesel applications should be able
to request selective, bounded, correctly typed reads from capability-owned
data through reusable authoring/validation semantics, **without** adopting
one engine for every data domain or transferring authoritative data ownership.
The first testable application is durable registered records; Todo is one
real consumer. Editor/asset metadata is a credible reuse scenario evidenced
by a research-only model in #130, **not** an accepted editor-product feature.
Game-engine/ECS/spatial/live analytics remain boundary tests or optional
future consumers, not a mandate to adopt relational semantics everywhere.

Accepting that outcome is a **product owner choice**. Current accepted
Riesel products do not already require universal SQL, federation,
stream subscriptions, distributed snapshots or cross-process query access.

### Conditional recommendation

**Recommend accepting an optional, transport-neutral typed query
*semantic foundation* with domain/provider-owned execution**, without
requiring one universal planner or every capability to implement it.
**Do not yet select the durable-record interface packaging.** A
separately versioned read-designated facet co-exposed with Storage v2
is the leading *linked-only candidate*, but tested SDK authoring shows
it still needs an independent typed model declaration path and a
fail-closed read/write owner relationship. A unified, attenuated or
composite record interface could prove better on the same criteria.
Keep logical query descriptions and supported features versionable;
do **not** copy SQL's implicit conversions into a Riesel contract.
Physical co-location can realize the first local profile, but is not
a universal requirement for future authorized proxies or replicas.

This is a **direction**, not an approved API name, Rust trait, wire
format, planner, grammar, set of SQL dialects or implementation issue.

**Why conditional:** If the owner chooses a *Todo-only* product outcome
rather than general model selection, domain-owned typed filtering may
be a better outcome/cost fit. Record that scope explicitly instead of
using synthetic examples to declare a general Query engine mandatory.

## 2. Research-derived comparison: alternatives remain open

The original fixed A/B/C/D options were removed from the research
protocol. [DISCOVERY.md](DISCOVERY.md) records 22 independently examined
mechanisms and domain comparators; external SQL builders, CEL/JSONPath,
GraphQL, query engines and spatial/ECS facilities solve different levels
of the problem. The following are **synthesized feasible designs after
discovery**, compared to the same **bounded, truthful registered-model
selection** outcome and credible longer-term reuse. Neither rows nor
ordering are a closed shortlist.

| Design family | Correctness and ownership | Scalability and future reuse | Material disadvantage / decision impact |
| --- | --- | --- | --- |
| Domain-owned queries on v2 | Can return honest partial pages; avoids platform API changes; Riesel's storage authority remains | Existing UUID pages can find any record by scanning, but sparse status matches require all pages; repeated app-owned implementations | If the *only* accepted outcome is Todo, this is valid; as a platform answer, it duplicates type/cursor/index semantics and creates costly future migration |
| Shared optional typed query vocabulary / validation SDK only | Source-level reuse, no new provider selection or RPC, safe domain autonomy | Provider-local SQL, Memory or other index implementation remains possible | No **common** query capability to consume: two providers may vary in supported selectors, errors, budget, cursor and revision semantics |
| Optional read-designated typed capability co-exposed by provider | Authoritative owner runs validated descriptors; query consumers need not get mutation methods; preserves v2 | Independent interface versions, common record selection and optional domain adapters, and native indexed execution possible | Independent **linked aliases can already resolve to different actual MemoryStore instances**, even when they use the same model. Co-exposure can produce shared state but is not automatic. Declare and enforce an authority/consistency relation for read-your-writes; managed/external trust and authorization remain unresolved |
| Unified new version of record Storage with rich query operations | One handle intrinsically chooses one provider; may simplify query/writer co-location | Strong in-Storage consistency boundary; broad version can include all operations | Extends/version-couples read and mutation API, costs migration/co-exposure, and does **not** provide read-only permission without a separate attenuated facet |
| Hybrid contract facets / owner-defined query adapter over shared semantics | Could expose read-only projection independently while using shared core contract validation | Preserves specialized domains and one owner; allows pluggable memory, SQL, search and editor realizations | Additional authoring/host facilities are worth the cost **only** if multiple consuming applications need the same observable behavior |
| General planner / relational engine / federation, e.g. DataFusion/Substrait | Powerful analytical planning when accepted requirements demand it; owner/security still needs Riesel-specific enforcement | Excels for heterogeneous analytical datasets, columnar operators and optimization | No accepted initial need for federation, giant query engine or ECS borrowed-data translation; more resource, schema, runtime and migration responsibilities |
| Existing SQL/ORM/DSL, CEL/JSONPath, GraphQL/OData | Useful **physical engines, expression evaluators or exposed API adapters** in the domains for which their semantics fit | May reduce code for SQL translation, UI-facing APIs or evaluator development | No external library automatically implements exact Riesel `u64`, `Uuid`, optional presence, cursor, owner/session, authorization or budgets. Screen licensing/MSRV if actually selected. |

**Evaluation evidence:**
- The real Todo and research editor-metadata models expose the same
  sparse-match behavior on v2; #130 verifies this on Memory/SQLite.
- A test-local typed predicate over actual Memory/SQLite records can
  define exact full-width equality and optional-presence semantics,
  but must scan v2 pages.
- Direct **scratch SQL after releasing the real provider** can produce
  exact equality and maintain a forced expression index, but a scalar
  SQL conversion fails on large `u64`; lexical decimal strings
  misorder `10`/`2` and raw UUID strings miscompare alternate casing.
- A live indexed sort over a **mutable** field can duplicate or omit
  rows between pages; no snapshot may be promised.
- A scratch 10,000-row sparse search took 1,200 SQLite progress
  callbacks of 50 VDBE instructions without an expression index and
  zero callbacks (fewer than 50 instructions) with an index; no
  managed RPC, wall-clock SLA or real provider-native Query was measured.
- The existing #130 linked co-location fixture shares one provider
  state; a separate managed alias can point to another runtime instance,
  even if metadata matches. Current provider identity pins are not
  remote authentication; read-only methods are not client authorization.

**Relative cost, not invented totals:** Each new public contract adds
SDK/transport/version/testing and selection complexity. Storage v3 adds
compatibility and privilege-coupling costs across all consumers.
Maintaining domain-specific query APIs incurs ongoing duplicated
validation and inconsistent pagination risks. A universal query engine
adds substantial executor/runtime and data-owner complexity that the
known record-selection requirements do not justify. No cost is expressed
as a fabricated engineering-hour estimate.

### New counterexample: co-exposure is possible, not guaranteed (2026-11-22)

The source of truth for this finding is a **research-only compiled test** in
[`probes/instance_binding.rs`](probes/instance_binding.rs), imported by a
Todo test wrapper. At [research CI #44974111817](https://github.com/aschenrot/riesel/actions/runs/44974111817),
**3/3 tests passed** with the existing SDK and actual `MemoryStore`:

1. Two **separately registered linked components** can satisfy the
   `RecordStore` and read-query capability requirements independently.
   A record created through the writer remains **absent from the reader**;
   the application runs without an authority-pairing error.
2. A component **co-exposing both contracts** on one `MemoryStore`
   instance sees the newly written record immediately.
3. A read-designated contract contains **no mutating commands**, but
   that absence is a contract *surface*, **not** external caller
   authorization or a promise about side effects.

This is an intentional compositional capability of Riesel's resolver,
**not a Storage defect**. It falsifies the stronger assumption that
declaring two aliases or merely supporting `ComponentDefinition::exposes`
automatically gives same-instance semantics. Independent Counter/
CounterObserver contracts are an existing example of Riesel's optional
read-facet pattern; a record read facet must additionally connect the
correct model/Storage owner or declare weaker, honest consistency.

**Implementation consequence:** A separately versioned read facet
requires an **explicit and verified relationship** when the application
needs read-your-writes. Candidate realizations include a fail-closed
application schema/provider relation, a composite SDK authoring handle,
or another proved implementation. Do not force every caller to handwire
two unverified aliases. A generic `same authority` invariant may be
useful for other capability pairs, but **no new platform schema API is
accepted here**. A unified Storage contract avoids this particular
inter-interface pairing but not read-only privilege exposure or
contract-version coupling.

**Future-proof distinction:** same **logical authoritative data**
and read consistency do **not** imply that every future query executor
is the same *physical* process. A secured proxy or read replica may be
valid with its declared freshness/consistency profile; it must never
silently claim immediate read-your-writes. Physical co-location is the
first linked realization, not a universal Query axiom.

### New authoring evidence: typed read-only versus dual bindings (2026-11-22)

[Six test-only SDK experiments](probes/instance_binding.rs) passed in
[research CI #44976893605](https://github.com/aschenrot/riesel/actions/runs/44976893605)
at prior revision `b74f5276d02fa10c8f39043d87215baa7a554033`.
These use Riesel's actual `Application`, `ApplicationParam`, model
derive, handler inference, resolver and linked `MemoryStore`; their
`ResearchReadView<T>` is **not** a public API.

| Tested authoring shape | Declared requirements | Derived model metadata | Demonstrated trade-off |
| --- | --- | --- | --- |
| `Records<TaskData>` alone | One Storage requirement | Model and `model_bindings` registered | Ergonomic, but the handle exposes create/update/delete as well as reads |
| Research-only typed `ResearchReadView<TaskData>` alone | One read-designated requirement | **No model registration** in `AppSchema.models` or `model_bindings` through that adapter | Can run against the linked provider but has no automatic high-level model/schema declaration or model-scoped authorization |
| `Records<TaskData>` plus `ResearchReadView<TaskData>` | Two requirements, two aliases | Model bound to writer's Storage alias, not reader's query alias | Does not itself declare the query's authoritative data relationship |
| Writer/read pair with `depends_on("read", ["writer"])` | Two requirements plus startup edge | No special owner relation | **Still reads from separate providers** despite successful preflight and correct lifecycle order |

**This is a concrete SDK constraint, not a failure of a deployed Query product.**
The current `Application::records_at<T>` registers the model **and**
the mutating `RecordStoreContract` requirement together; generic
`Application::rpc<C>` registers a capability but **does not** infer a
Rust model declaration. The underlying `AppSchema` separates
`models`, `model_bindings` and `requires`; choosing how a
query-only application's typed model is registered is an interface
design problem, **not** evidence that typed read-only queries are
inherently impossible.

**Packaging criteria for the next interface design:**

1. A read-only application must declare and validate its typed model
   **without** being forced to request mutation authority.
2. An application combining read and write must declare and validate
   **same logical data authority and its actual consistency profile**
   without manually maintaining two potentially divergent aliases.
3. A lifecycle dependency or a provider-ID/name match must **not**
   be mistaken for an authenticated runtime data-owner relationship.
4. Separate read/write facets, a first-party composite model handle,
   an attenuated Storage facade and an evolved unified interface must
   be compared using **one common authoring task**, actual schema
   output, least-privilege exposure, deployment preflight and restart
   behavior. No concrete public method/trait/macro is accepted here.

**Revised architectural recommendation:** Accept the *optional shared
typed Query semantic direction* **separately** from its concrete
record-interface packaging. A co-exposed read facet remains an
attractive first linked option, **not a fully established winner**:
it needs a model-only registration path and enforceable owner
relationship, or an alternative combined/attenuated authoring design
with equivalent guarantees. A unified Storage version remains a
valid comparator. Do not authorize a public API based merely on the
current research-only `ResearchReadView<T>` wrapper.

## 3. Recommended *semantic invariants*, not a public AST

1. **Logical owner and schema first.** A query identifies the
   authoritative data owner, registered model/schema, and declared
   read-consistency profile. The first linked implementation binds to
   one real provider instance; a future externally authorized proxy
   or explicitly stale replica needs separate trust/freshness guarantees.
   The owner validates model, field, operator, value type, feature
   support and budgets **before executing**. Unknown or
   unsupported semantics fail explicitly; never silently scan without
   honoring an advertised work limit.
2. **Typed scalar truthfulness.** `u64` and `i64` use exact full
   domains; UUID equality uses parsed canonical values, not raw text;
   optional **missing** and **present-null** need an explicit semantic
   distinction or a deliberately documented normalization. String
   collation/Unicode, `f64` including exceptional values, nested JSON,
   lists and user-defined types require separate profile decisions.
   A simple first profile need not falsely promise every operator.
3. **Portable logical description, provider-owned execution.**
   The shared facility may describe validated selectors, stable sort,
   bounded result/projection and supported feature sets; Memory/SQLite,
   search, graph/ECS or other domain owners choose suitable physical
   engines. An ECS World borrow is **not** serializable by declaration.
   Avoid forcing all providers to implement any one generic DSL.
4. **Explicit result truthfulness.** An empty **partial** result is
   not an empty collection. Report progress/exhaustion, incomplete
   budget outcomes and whether pages are live or consistent snapshots.
   Default first behavior should permit bounded live UUID traversal
   with documented mutation effects; **do not** promise exactly-once
   general mutable-sort pagination.
5. **Cursor binding and invalidation.** Treat continuation as
   provider/session/query/schema/order/profile-bound; reject stale
   generation, foreign predicate or schema, exhausted/invalid cursor
   and tampering. Actual authenticated instance/session identity and
   cursor integrity must come from an accepted operational trust
   mechanism; a caller-supplied `u64` generation is not a credential.
6. **Work and memory budgets.** Validate requested limits, scan/CPU
   work, concurrent queries, output/RPC bytes, cancellation and
   unsupported indexed access. A 32-row return limit does not make
   a 10,000-row table scan cheap. On budget exhaustion report an
   explicit incomplete/failure outcome without fabricating no-matches.
7. **Trust, authorization and exposure.** A read-only capability
   **surface** can remove mutation methods, but it does not itself
   authorize external readers. Enforce explicit app/model/read scope,
   deny by default for independently connected clients and do not
   leak unrestricted records through inspection. Co-located local
   consumption may use the existing trusted application boundary;
   remote/GUI export depends on the accepted #75/#60 trust semantics.
8. **Versioning and migration.** Preserve `riesel.storage.records@2`,
   model identity/revisions and existing offline evolution guarantees.
   A query profile/contract and provider feature support evolve
   independently of physical index layout. No automatic SQLite DDL
   on normal startup; any index installation/rebuild/rollback belongs
   to a separately approved, bounded, owner-exclusive offline
   operation, not an unreviewed extension of model migration.
9. **Error/provenance semantics.** Distinguish invalid query/schema,
   unsupported feature, unauthorized, source not ready, stale binding,
   budget exceeded/incomplete, cancelled, transport unknown and
   provider error. Treat read failure/retry differently from writes,
   but do not infer that timeouts imply cancellation of already
   dispatched work.

These invariants could be represented by different concrete Rust APIs,
standardized expressions or a small custom format. **Do not freeze
any names, AST nodes or error enum from this research document.**

## 4. Concrete first increment and extensions (if direction accepted)

**First delivery candidate — optional linked registered-record queries
after interface packaging acceptance:** Preserve Storage v2. Evaluate
a separately versioned **read-designated capability** co-exposed by one
Memory/SQLite owner against a unified/attenuated or composite model
interface; select the winner only after demonstrating model registration,
least privilege and fail-closed authority binding.
A read-only application may bind that facet without a writer; an
application requiring read-your-writes must explicitly declare the
relation and have the SDK/resolver validate it before side effects.
The authoring model must avoid duplicative, error-prone manual alias
selection. Demonstrate model-scoped typed equality / presence, stable
UUID order, honest bounded pages and work/cancellation budgets.
Only expose a predicate/operator when native execution meets its
declared guarantees on both supported providers or explicitly
negotiate non-support. Query-only clients should not get write
methods through the same bound capability.

**Not a first-release commitment:** arbitrary SQL, arbitrary
user-supplied script/AST execution, cross-model joins/federation,
GraphQL as the core protocol, fulltext, cross-shard snapshots,
general numeric range sorts, ECS borrows over RPC, speculative
planning optimizer or unbounded background indexing.

**Future extension points:** typed range/sort profiles with correct
order-preserving keys; provider-native indexes with explicit
physical-migration policy; authorized managed/external read surfaces;
domain-specific search/spatial/graph adapters; subscription/snapshot
and query-plan introspection, each only when the relevant owner and
consistency/budget requirements are accepted.

**Implementation slices only after owner acceptance:** define exact
logical/profile semantics and fail-closed error matrix; implement a
co-exposed read-only facet with strict owner/session and model
selection; add Memory and SQLite native parity/limits tests;
validate physical index maintenance and migration procedure;
integrate a generated typed SDK and relevant Todo outcome;
then expand deployment/trust profile when #75 provides evidence.
Separate independently testable issues; do not manufacture
one issue for each helper or turn a research test into a public
interface accidentally.

## 5. Residual risks and decision gate

- **Product scope:** Existing Todo is an accepted consumer, an editor
  metadata model is a research probe. The owner must decide the
  platform-wide reuse outcome; a research fixture cannot accept it.
- **Remote trust:** #75 has not established authenticated external
  application/session principals, permission scope or revocation.
  This blocks claiming remote query security, not choosing an
  architecture with an explicit fail-closed boundary.
- **Index lifecycle:** new expression indexes alter persistent
  database physical schema; #41's accepted model-only offline
  migration contract does not approve them. Implementation
  requires an explicit indexed-storage evolution decision.
- **Coverage:** No production-native Memory/SQLite Query operation,
  managed-provider query authorization test, benchmark SLA or
  cross-domain Query adapter is delivered. Those are acceptance
  gates for *implementation*, not evidence that a universal engine
  is required.
- **Upstream library selection:** 22 families screened but no
  external production Query dependency selected. License/MSRV/
  maintenance validation is required **if one is selected**, not
  a reason to choose a custom language or guess future upgrades.
- **Bias audit:** An over-generalized engine would wrongly
  constrain ECS/simulation domains; an under-generalized Todo-only
  adapter would duplicate typed correctness/pagination semantics.
  This recommendation chooses a reusable *semantic* boundary and
  an optional first read capability with domain-owned execution.
  It does not claim the first adapter meets every future domain.

### Requested recorded owner decision (in #138)

**Recommended: ACCEPT SHARED SEMANTIC DIRECTION; REVISE OR DEFER THE
RECORD-INTERFACE PACKAGING UNTIL ITS MODEL/OWNER AUTHORING CONTRACT
IS VERIFIED.** Confirm or revise two independent statements:

1. **Product:** Riesel should provide *optional reusable typed Query
   semantics* for capability-owned application data across plausible
   consumers, while preserving specialized/domain-owned execution.
   It is not required that all capabilities use one Query protocol.
2. **Architecture (conditional):** prefer a separately versioned
   read-designated facet co-exposed with Storage v2 for the initial
   *linked* record profile, **provided its authority pairing and
   authoring ergonomics are explicitly specified and tested**. The
   interface specification must compare this against a unified or
   attenuated Storage facet on those criteria. No mandatory universal
   planner, immediate Storage v3 migration, or physical co-location
   requirement for future trusted proxies/replicas is implied.
   Remote authorization and index lifecycle are distinct supported
   gates and must fail closed until approved.

**Reasonable revision (now favored by authoring evidence):** Accept
only the optional semantic foundation, and require a targeted interface
specification to prove read-only typed model registration and fail-closed
query/writer authority binding before selecting a separate read facet,
unified Storage or a composite/attenuated variant. **Reasonable deferral:** if the intended
product scope truly ends at Todo, record that constraint and
deliver a domain-owned read contract rather than an unjustified
platform-wide Query requirement.

No owner decision is inferred from a green test or this research PR.
