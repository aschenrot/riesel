# General Query architecture — evidence and cross-domain assessment (#138)

**Research date:** 2026-11-22 (Europe/Berlin). **State:** **decision-review comparative research**, not an owner-accepted architecture, an exhaustive systematic literature review or an implementation. The optional shared semantic direction is recommended; the record-interface packaging remains explicitly undecided.
**Code/contract baseline:** PR #140 originally branched from `aschenrot/riesel` `main` `93b5188d122f5fa35a3bf047d6d068995c1d74b5`. Rechecked current `main` `29c90f5285f99022badac4af50aa3e57f719d3cc` on 2026-11-22: its changes affect execution-plan/runner examples, tests, Cargo metadata, product documents and roadmap, **not** the Storage/model/provider contracts exercised by these fixtures. The research branch merged that main revision in commit `09c6144acc843664d550629915a793971d726370`. The merged-tree validation is tracked independently from earlier research commits. [spec #138](https://github.com/aschenrot/riesel/issues/138); [plan](PLAN.md); [discovery log](DISCOVERY.md). Other research cited below is **not merged into this baseline**.
**Audience/outcome:** Rust application authors requiring honest selective reads over capability-owned data, starting with Todo. Whether other users need one shared cross-domain Query interface is **not yet an accepted requirement**.

## Method and evidence classification

Candidates were collected in the separate [discovery log](DISCOVERY.md),
which records public upstream documentation for **22 discovered mechanisms
and comparators**; the discovery categories are not a mandatory shortlist.
Read `README.md`, `CONTRIBUTING.md`, `docs/project/delivery.md`,
relevant accepted Riesel product and authoring/Storage/lifecycle
contracts, the #107 research branch, and independent RunenECS architecture.
Cross-repository Runenwerk ADRs 0017/0018 were consulted as
**external comparison**, not Riesel acceptance.

**Expanded discovery after the first pass:** CEL-style expression
engines, JSONPath, embedded scripting, OData and native spatial/physics
query providers are additional **candidate mechanisms**. They show why
a fixed ORM-vs-universal-engine shortlist was inadequate. Their API
descriptions are documented in `DISCOVERY.md` and are not
Riesel compatibility or security proof.

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
The dedicated [research CI run #44967915491](https://github.com/aschenrot/riesel/actions/runs/44967915491) reran the exact fixture at PR head `403ecaedaa74b270ef145a802606c941fa19c78f` using **SQLite 3.46.1**, with **3/3 tests passed**; this is still a scratch-only probe.

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

## New typed-provider research: semantics and continuation (2026-11-22)

**Reproduction:** Run the Rust test target via **cargo test -p todo --test query_architecture_research -- --nocapture**. The wrapper includes [probes/typed_record_contract.rs](probes/typed_record_contract.rs) outside the production source directories.

**Actual scope:** Two Rust tests register an independently derived **ResearchMetadata** schema on the *real MemoryStore and linked SqliteStore*, seed 65 records, and inspect reads/mutations through unchanged published RecordStore v2. A tiny **research-only Rust interpreter** applies validated typed predicates **after v2 page retrieval**. It is **not** a native indexed provider, production query protocol, new public interface or benchmark of SQLite predicate pushdown.

| Probe | Reproduced result on both providers | What it does not prove |
| --- | --- | --- |
| Full-width unsigned/signed values | u64 maximum and maximum minus one remain distinct on Storage round-trip; i64 minimum and maximum round-trip | Direct SQLite json_extract filter correctness, indexed numeric ordering or a new Query provider |
| Optional field presence | Explicit JSON null matches a different filter than a missing registered optional field | Rust Option<T> preserves presence; it normally collapses both to None on typed decode |
| Invalid/unsupported operator | U64 predicate on boolean, missing-check on required field and unsupported range fail before scan | A complete public expression language or provider support negotiation |
| Sparse matches | First two 32-record scan budgets return **empty partial pages**; continuation finds the only match at UUID position 65 | Bounded latency, native index selectivity or globally empty result from an empty partial page |
| Cursor query/session mismatch | A test-local cursor rejects changed model, predicate or manually supplied generation | Authenticated runtime session, tamper-proof cursor, restart or managed lifecycle proof |
| Known exhaustion | A 64-record bounded scan with an early match is still *partial* until the remaining 65th record is inspected | Snapshot-consistent results across pages |

**Verification:** [Research CI #44968670157](https://github.com/aschenrot/riesel/actions/runs/44968670157) passed **2/2 Rust tests** (each exercises both actual providers) and **3/3 Python SQLite tests** on the earlier revision **a3aa116c639a2b0be4bd3faffd88e892453dc66a**. The first compilation revealed real SDK constraints: RieselModel derives reject Serde field modifiers, so optional missing/null tests must use valid raw record data; test-only direct SQLite dependency registration was also required. These corrections do not change the production contract. Normal workspace validation is an **independent** gate.

**Architectural significance:** Riesel needs an explicit choice between *portable logical semantics* and *native provider acceleration*. A bounded output count does not bound scan cost. Full-width integers, optional-field presence and cursor provenance must be defined before freezing a shared Query contract. These tests strengthen semantic feasibility but do **not** decide between independent domain APIs, shared validation, separate Record Query or Storage evolution.

## Native SQLite equality, optional-field semantics and interruption research (2026-11-22)

**Scope:** [`sqlite_native_predicates.rs`](probes/sqlite_native_predicates.rs) is imported by a **test-only** integration wrapper in `packages/storage/sqlite/tests/`. The fixture calls the real, unchanged `SqliteStore::open`, writes 65 schema-registered records through `RecordStore v2`, revision-checks targeted updates, **drops the provider to relinquish the exclusive cooperative owner**, and only then opens the disposable database using raw `rusqlite`. It **does not** add query methods to Riesel or execute SQL through a live authenticated provider handle.

**Reproduction:** `cargo test -p riesel_storage_sqlite --test query_architecture_research -- --nocapture` on Rust 1.100.0, with the repository's pinned `rusqlite 0.43.0` bundled SQLite. [Research CI #44971511998](https://github.com/aschenrot/riesel/actions/runs/44971511998) passed **4/4 native SQL Rust tests** on revision `2404ec84b62f2d47fb23ea644a8930e14b4ce411`, plus **2/2** Memory/SQLite typed-reference tests and **3/3** Python SQLite continuity tests. The Rust fixture prints bundled SQLite **3.54.1**; the separate Python runner's SQLite version is **3.46.1**. These are exact tested versions, not cross-version portability certification. Later changes must receive their own exact-head check.

| Scenario | Verified behavior | What is **not** proved |
| --- | --- | --- |
| Exact unsigned equality | `(CAST(data AS TEXT) -> '$.big') = ?` with a bound **decimal string** returns `u64::MAX` and `u64::MAX - 1` individually from actual provider-serialized JSON | Universal correctness for every numeric model/operator, range order or other database providers |
| Unsafe extraction falsifier | `json_extract(...,'$.big') = ?` with a bound `f64` matches **both** distinct large values | A defect in the published Storage v2 interface (which does not expose these predicates) |
| Signed extremes | JSON numeric **text tokens** match persisted `i64::MIN` and `i64::MAX` individually | Full numeric sorting/collation, mixed signed/unsigned or conversion/normalization guarantees |
| Numeric ordering counterexample | The JSON text tokens for valid `u64` values `2` and `10` sort lexically as `10, 2` | An equality-compatible index is **not** automatically a valid numeric sort/range index; require a typed, order-preserving representation or a different plan |
| UUID equivalence counterexample | Two registered valid UUID fields containing uppercase and lowercase spelling parse to the **same Rust `Uuid`**, but SQLite default text equality for lowercase returns only the lowercase row | Raw JSON string comparison does **not** implement typed UUID equality; plan canonicalization or typed comparison explicitly |
| Missing versus explicit null | `json_type(...,'$.note') IS NULL` finds omitted fields; `= 'null'` finds explicitly null properties | An automatically chosen public semantic mapping for `Option<T>`; typed Rust decoding may erase presence |
| Boolean sparse lookup | `json_type(...,'$.completed')='false'` identifies the 65th record | A resource/latency budget for unindexed scans or a stable snapshot |
| Scratch index | A disposable expression index on `(model, (CAST(data AS TEXT)->'$.big'), id)` returns the same exact records; **forced** `INDEXED BY` plan reports `USING COVERING INDEX` | Automatic planner choice, production index migration, transparent provider integration, or an end-to-end performance/SLA claim |
| Cooperative cancellation | `Connection::progress_handler(1, ...)` interrupts one test-only read and is then removed; a subsequent read succeeds | Application session cancellation, cross-process/RPC abort propagation, bounded owner-lock hold time, or cancellation of a mutation |

The expression-index proof is deliberately **not a benchmark**: 65 records, query plan forced, SQL executed after releasing the authoritative provider, no concurrent writer or network transport. Physical index installation is expressly **outside** the accepted model-only offline migration contract and would need its own authorization, backup/rollback, compatibility and lifecycle decision. See the upstream [SQLite JSON operators](https://www.sqlite.org/json1.html) and [rusqlite progress-handler API](https://docs.rs/rusqlite/0.43.0/rusqlite/struct.Connection.html).

**Security and correctness consequence:** Any logical query layer would need a model-aware typed comparator and a controlled provider-owned translation. The numeric-sort and UUID-equivalence counterexamples show why copying a familiar SQL/JSON expression library into an unrestricted generic query representation would silently change accepted Rust model semantics. The exact SQL approach depends on validated, canonical integer JSON encoding and a *bound string* value, not arbitrary raw SQL or unchecked runtime JSON paths. Equality on these scalar types does **not** establish correct numeric ordering, UUID normalization, Unicode collation, float special cases or arbitrary nested JSON search. A Riesel Query API cannot authorize an external connection merely because it can read the same SQLite file.

### Query/read versus mutation authority — implementation-grounded comparison

From `crates/riesel_app/src/deployment.rs`, `runtime/bootstrap.rs`,
`plan.rs`, `crates/riesel_protocol/src/session.rs`, the accepted authoring
contract and #107's **research-only** co-location fixture:

- `RecordStore v2` publishes **get/list_page and create/update/delete on one capability interface**. A client holding the typed handle exposes both operation families; it is **not** an enforced read-only facet. The current loopback transport is unauthenticated.
- Local aliases selecting one registered provider ID share its **component instance**; this is a feasible same-state realization. A managed `instance: Some(name)` permits multiple requirements to reuse a child with consistent launch parameters. However, the planner's metadata pins are not authenticated remote identity, and two unpinned/unnamed managed selections can be distinct.
- `SessionToken::shares_session_with` compares **in-process** shared revocation ownership; it is explicitly not a remote principal credential, durable session generation or proof that a browser/IPC caller may inspect a model.
- A separately versioned read interface allows a *potential* contract-level read-only facet but introduces an explicit pairing requirement when its writer is a separate selection. Co-exposing two interfaces from **one actual component instance** can solve linked same-state identity; managed/external session, startup authorization and invalidation remain separate guarantees.
- A unified versioned Storage interface chooses one provider for both read and mutation methods, eliminating the *inter-interface pairing* problem for one handle. It still does not automatically provide read-only permission, and it couples query evolution to the storage contract; an optional **read-only facet of a unified provider** remains a viable hybrid, not a forbidden alternative.
- Domain-owned typed query methods and reusable **authoring/validation helpers** avoid imposing a universal execution contract. They may reduce runtime coupling but cannot guarantee the *same* semantics, performance or SDK behavior across independently implemented providers unless those are defined and verified separately.
- A centralized planner or federated engine cannot derive cross-provider snapshot, data-sharing authority or safe access to untrusted provider data from existing metadata alone. Demonstrated engine expressiveness does not resolve Riesel's operational identity/authorization specification #75.

| Viable architectural packaging (not a fixed shortlist) | Main benefit for registered records | Additional verified or outstanding obligation |
| --- | --- | --- |
| Domain-owned retrieval with shared optional helpers | No new cross-contract pairing, preserves full domain authority | Repeated operator, pagination and index semantics; unindexed reads can be costly |
| Independent read/query capability co-exposed with Storage v2 | Read-only *interface shape* with independently evolvable version; one component can own both | Need fail-closed pairing in deployment, real session/instance proof and **actual** read authorization; local evidence from #107 is not remote certification |
| Unified versioned Storage replacement or co-exposure | Intrinsically same handle for read/write, fewer cross-interface relationships | Query grammar versioning, v2 client co-exposure, contract surface and mutation privilege/read-facet design |
| Hybrid read-only facet with shared logical vocabulary and domain-owned execution | Separates operator portability from physical execution and permits narrow read privilege | Higher authoring/selection complexity if each facet needs its own contract; must prove reduction in long-term duplication |
| Broader planner, third-party language or federation | Potential expressiveness for separately accepted analytical/integrated workloads | No demonstrated first-release need; additional owner, cost, security and cross-domain consistency proof |

**Assessment:** The SQLite-native results remove one **feasibility uncertainty**
for exact equality and optional-field presence, but **do not** decide
between these packaging choices. Security and single-authority
guarantees require actual operational controls, not only a DSL.
The lowest-cost Todo fix and the platform's longest-lived abstraction
remain separate architectural questions. No architecture is accepted.

## Query cost, budget enforcement and index maintenance (2026-11-22)

**Reproduce:** `python3 research/query-architecture/probes/sqlite_cost_budget.py` (Python standard library, SQLite; **synthetic in-memory scratch records**, not a running Riesel provider or RPC service). The same predicate selects exactly **one** `completed=false` record at the end of the UUID-ordered model, at size 100 or 10,000. `EXPLAIN QUERY PLAN` confirms a primary-key scan without a secondary index and use of a scratch SQLite **expression index** after installation. Python SQLite's `set_progress_handler` is invoked every **50 SQLite VM instructions**; callback counts are a deterministic *work proxy*, **not** wall-clock CPU time, elapsed latency, RPC cost or a generalized cross-version benchmark.

[Research CI #44972111565](https://github.com/aschenrot/riesel/actions/runs/44972111565), commit `dc8a4a4cc29742376cdb6f17170382af44e51728`, passed **all 3 new Python tests**, in addition to prior Python and Rust research probes. Exact runner's Python SQLite version: **3.46.1**.

| Rows | Without extra index: callback counts | With index: callback counts | Retrieval and query plan |
| ---: | ---: | ---: | --- |
| 100 | 12 | 0 (fewer than 50 VM instructions) | Both return the same lone row; unindexed uses `SEARCH ... USING PRIMARY KEY`; index uses `research_status_idx` |
| 10,000 | **1,200** | 0 (fewer than 50 VM instructions) | Same match and confirmed index use; sparse scan work grows with dataset |

A **30-callback** interruption threshold rejects the unindexed
10,000-row query but accepts the indexed query with the same result.
The scratch expression index also responds to predicate-field
updates: a changed `false`→`true` record leaves the index result;
a different `true`→`false` record enters it. This verifies
**SQLite's scratch physical index maintenance**, not the Riesel
provider's authorized indexed-query path.

**Architectural consequence:** A generic Query facade should not
equate result-count limits with work limits; it needs explicit provider
budgets, interruption semantics, supported feature negotiation and
optional native indexes when efficient selective reads are required.
A portable logical query layer must not promise indexed execution
simply because one SQLite experiment is fast. Source/provider-owned
index creation, backup/rebuild, model/schema drift and recovery are
a **separate accepted physical migration contract**, not incidental
normal startup behavior.

### Additional linked binding counterexample — query/read authority is not automatic

[The standalone instance-binding research test](probes/instance_binding.rs)
examines the existing SDK's generated read-only capability and **actual
`MemoryStore`**. At [research CI #44974111817](https://github.com/aschenrot/riesel/actions/runs/44974111817)
all **three** research tests passed:

- A writer and a read-only query contract resolved from two
  **independent linked providers** run normally but observe different
  state. A successful writer `create` is invisible to the reader.
- Co-exposing both contracts from one linked `MemoryStore` instance
  makes read-after-write visible.
- A query-only generated contract advertises no mutation methods;
  this **does not** authenticate readers or enforce client permissions.

The existing [CounterObserver contract](https://github.com/aschenrot/riesel/blob/main/examples/counter/contract/src/lib.rs)
provides a real precedent for optional read-only facets, while #130's
research checks planned linked/managed affinity. This new negative
test reveals that **co-exposure ability is not an automatic resolver
invariant**; an application can legitimately select unrelated
providers when no owner relation is declared.

**Consequences:** the recommended first *linked* record read facet
must either declare and fail-closed verify the owner relation when
read-your-writes is required, provide a composite SDK authoring
projection with equivalent safeguards, or choose a different interface
shape. An extra query alias should not silently pick a different
provider with the same model definition. A query-only consumer
can be useful without mutation access, but its caller's authority
must still be enforced at the application/export boundary.

**Do not overgeneralize physical co-location:** long-term read-only
proxies, authorized read replicas and materialized views can be valid
*if* the logical data source, consistency/freshness and session
trust profile are explicit. Requiring every future query executor to
share a process with the writer would be speculative. The first
linked Memory/SQLite implementation can use literal co-exposure
as its conservative consistency baseline.

### Application SDK ergonomics and authority are decision-critical

The [research-only instance-binding fixture](probes/instance_binding.rs)
now exercises **six** linked SDK cases, including two typed parameter
authoring variants and a lifecycle-order counterexample. [Research CI
#44976893605](https://github.com/aschenrot/riesel/actions/runs/44976893605)
passed all **6/6** at revision `b74f5276d02fa10c8f39043d87215baa7a554033`.

- Plain `Records<T>` registers the model and a single Storage
  requirement, but its surface contains mutation methods.
- A custom typed **read-only** `ApplicationParam` can inject a
  generated query contract with a single read requirement and no
  Storage writer. It **does not register** the typed model in the
  generated `AppSchema`. This is an existing SDK *authoring gap*,
  not an inability of the schema format to represent models.
- Combining the existing `Records<T>` and research read-only
  parameter creates **two** independently selected requirements;
  only the Storage alias receives `model_bindings`. No explicit
  consistency/owner relation is generated.
- An actual `depends_on("read", ["writer"])` startup edge
  succeeds while the handlers still read/write **different MemoryStore
  instances**. Lifecycle ordering is **not** data authority.
- Co-exposure from one component instance remains a valid first
  linked strategy; it does not answer cross-process authentication
  or independent read-only permissions.

This evidence changes the **relative maturity of packaging options**:
a separate read-only facet remains plausible, but calling it
architecturally ready without a model-only declaration path and a
fail-closed owner relation would overstate what the SDK supports.
A unified Storage contract avoids inter-interface pairing at the cost
of version/permission coupling. A composite typed handle could improve
syntax but would not **by itself** prove pairing. The next interface
specification must compare those options against identical authored
applications, rather than picking a crate or AST from the prior
discovery list. See [DECISION_REVIEW.md](DECISION_REVIEW.md).

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
| **Portable non-SQL predicates (CEL, JSONPath or constrained expression evaluator)** | A bounded expression engine could validate/evaluate selectors on provider-owned records without a full SQL/Arrow engine, **if** precise Riesel type/null semantics and limits can be represented. | Reuse of existing parsers may reduce new-language work; translating expressions to SQLite indexes and proving `u64` parity/cancellation can be difficult. | **Newly discovered independent candidate**, requires typed semantic and pushdown proof |
| **Standardized remote resource filtering (OData, GraphQL-style APIs)** | Mature API selection ideas are relevant if remote UI/client interoperability is an actual goal; they do not define Riesel's authenticated provider identity. | Adds another schema/API surface and potential transport coupling, so requires explicit consumer and security benefit. | **External API candidate**, not a mandatory baseline |
| **Spatial/domain-owned indexing (rstar/Rapier)** | Native geometric query and validity rules remain with the domain; an optional bridge might expose bounded inspection metadata. | Strong evidence against treating all `Query` operations as relational record filters; may offer specialized adapters. | **Specialized pressure test**, not inherently incompatible with shared inspection vocabulary |
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

**Speculative abstraction bias:** 22 discovered mechanisms do not imply
22 required Riesel features; neither does a shared name `query` justify
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

### Current outcome: recommendation ready for owner review, not architecture acceptance

**Research supports recommending an optional typed, transport-neutral semantic foundation with data-owner-controlled execution**, particularly for registered
persistent-record selection. It does **not** justify one mandatory
universal planner/engine, a public Query grammar, unrestricted
cross-process Query access, or immediate Storage v3 migration.
[DECISION_REVIEW.md](DECISION_REVIEW.md) now gives a **conditional
recommendation** for a reusable semantic boundary plus an optional
separately versioned, provider-co-exposed **read-only record-query
facet**, with explicit linked-first trust and feature bounds.
The owner may accept/revise/defer this direction. Green CI is not
acceptance of the public interface.

The decision *could* change with:

1. A real second registered-model consumer proving equivalent typed
   operations, not merely contrived analogies to Todo;
2. Build on the now-verified scratch SQLite **native expression** proof:
   resolve provider-owned execution without raw connection bypass,
   complete signed/UUID ordering and query version/projection semantics,
   native Memory/SQLite parity and real authenticated cursor/session
   fencing. The SQL and Rust interpreter fixtures do not publish a
   Query contract or prove managed-provider access;
3. Complete the now-tested SDK authoring comparison with explicit
   model-only registration, same-data-authority preflight/session checks,
   and read-only permission enforcement versus co-exposed/unified
   Storage; 6/6 research binding/authoring tests are not an accepted API;
4. Establish real provider-native bounded index and cancellation behavior under Riesel ownership, not just scratch SQLite; then test managed placement and authorization before claiming remote support;
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
  asserted all **3/3** outcomes; [research CI #44967915491](https://github.com/aschenrot/riesel/actions/runs/44967915491) passed **3/3** tests at earlier commit `403ecaed`. A later doc update requires an independent **new exact-head** CI check; do not transfer the old run's pass status automatically.
- Typed provider research: run **cargo test -p todo --test query_architecture_research -- --nocapture**. [Research CI #44968670157](https://github.com/aschenrot/riesel/actions/runs/44968670157) passed two Rust tests against Memory and SQLite at earlier commit **a3aa116c**. The interpreter filters retrieved v2 data in Rust, not inside either provider.
- Provider-authored SQLite native SQL experiment: `cargo test -p riesel_storage_sqlite --test query_architecture_research -- --nocapture`. [Research CI #44971511998](https://github.com/aschenrot/riesel/actions/runs/44971511998) passed **4/4 native SQL Rust tests** at head `2404ec84b62f2d47fb23ea644a8930e14b4ce411`, covering precise integer tokens, missing/null, incorrect lexical numeric sort, UUID spelling mismatch, forced scratch index and connection-local interrupted read. Bundled SQLite is **3.54.1**. SQL is executed **directly on a released disposable provider file**, not through a published Query operation.
- Sparse-query cost: `python3 research/query-architecture/probes/sqlite_cost_budget.py`. [Research CI #44972111565](https://github.com/aschenrot/riesel/actions/runs/44972111565) passed 3/3 work-budget, parity and scratch index-maintenance tests at earlier commit `dc8a4a4`, Python SQLite 3.46.1; not a provider-native/query-transport SLA.
- Decision-review document: [DECISION_REVIEW.md](DECISION_REVIEW.md), authored for owner acceptance/revision, **not** an approved product contract.
- SDK authoring and linked owner semantics: `cargo test -p todo --test query_instance_binding_research -- --nocapture`. [Research CI #44976893605](https://github.com/aschenrot/riesel/actions/runs/44976893605) passed **6/6 tests** at revision `b74f5276`. The fixture demonstrates a read-only model declaration gap, extra aliases, and the fact that `depends_on` never implies shared data authority.
- Comparative claims: upstream documentation links in
  `DISCOVERY.md`. **Not verified** here: legal licensing audit,
  upstream package MSRV at pinned Rust 1.100, actual integration
  into Riesel, performance SLA or remote authorization.
- The investigation should stop (and a final recommendation be
  submitted to #138) **only after** the decision-critical interface
  ownership/semantics tests distinguish the viable options. If
  the missing evidence remains material, document a precise deferral,
  not an invented winner.

**Research-stage inference:** A precise shared *typed* query vocabulary could plausibly support durable model operations across Memory and SQLite **without** imposing a universal execution engine. It cannot delegate type safety to SQL's ordinary numeric conversions, JSON text ordering or string UUID equality. This supports specifying a coherent logical semantics/evaluation boundary first, but **does not select an independently published Query capability, Storage v3, a read-only facet or any production API**. The external trust and session gap remains material.

**No owner accept/revise/defer/reject decision has been recorded by
this research report; #138 remains open and this PR remains draft.**
