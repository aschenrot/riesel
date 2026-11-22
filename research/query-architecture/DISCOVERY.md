# Query architecture — independent candidate discovery log (#138)

**Research date:** 2026-11-22 (Europe/Berlin). **Status:** preliminary primary-source discovery and screening inputs, **not** a ranked shortlist, an accepted product outcome, conformance certification, or an architecture decision.
**Repository baseline:** `aschenrot/riesel` `main` `93b5188d122f5fa35a3bf047d6d068995c1d74b5`.
**Decision and protocol:** [Issue #138](https://github.com/aschenrot/riesel/issues/138); [PLAN.md](PLAN.md).
**Motivating evidence, not accepted platform scope:** [Todo #107](https://github.com/aschenrot/riesel/issues/107) and draft [PR #130](https://github.com/aschenrot/riesel/pull/130).

## Research question and source discovery procedure

For Rust application authors who need selective/ordered/bounded data reads over
one or several authoritative Riesel capability providers, investigate what
reuse is warranted and what remains domain-owned. The sole **verified immediate
problem** in this research is current Todo's filtered paging over UUID-only
record pages. Editor/asset, remote services, relational, graph, ECS, and
analytic scenarios are plausible **pressure tests**, not accepted mandatory
features or promises of a universal interface.

Discovery was conducted **before forming a new recommendation**:

- Read Riesel's product/authoring/delivery and `RecordStore` contracts plus
  #107's draft research; classify existing capabilities separately from
  possible extensions.
- Query primary official documentation for *Rust-local typed query APIs,
  dynamic SQL builders, object/relational access, database interface
  libraries, embedded/remote data engines, analytical planners, interoperable
  relational formats, search, incremental computation, GraphQL API
  selection, and ECS/world queries*. Search terms included these categories
  and major known project examples, not just `Record Query` and `Storage v3`.
- Consult current documentation URLs below (retrieved 2026-11-22).
  These are public project claims/available API descriptions, **not**
  Riesel compatibility measurements; version, license, MSRV, maintenance
  and deployment facts need verification **for shortlisted integrations**.
- Include `no new query abstraction` / ordinary Rust iterators as an
  independently relevant baseline. Categorize **by where semantics live**
  rather than force every candidate into one A–D taxonomy.

**Coverage limitation:** This pass is a discovery survey, *not* an
exhaustive crates.io/library index, formal systematic literature review,
license audit, binary-compatibility study, or test run against these
external candidates. Unfound alternatives remain possible; expand the
search if a missing mechanism could alter the decision. A link's presence
in this log is not a recommendation to adopt the library.**

## Baseline eligibility before screening

These criteria follow **accepted Riesel architecture** for any candidate
**that becomes a shared platform facility**, not mandatory capabilities
of every external library:

1. Preserve typed capability/provider ownership, explicit local/managed/
   external placement, and the existing provider/session preflight authority.
   A third-party DB/ORM may live **inside** a provider; its raw client does
   not automatically become Riesel's published cross-provider contract.
2. No incorrect results for explicitly supported model types and operators;
   unsupported or unprovable semantics fail explicitly. No implicit
   compile-time Rust closure serialization or hidden second data source.
3. Do not silently alter `riesel.storage.records@2`, its UUID/revision
   behavior, or the accepted physical-model/offline migration boundaries.
4. Provide a reasoned, versionable error/compatibility surface; no
   unspecified authentication from matching metadata and no silent
   escalation of read permission to mutation authority.
5. If an option makes a **specific** efficiency, consistency, snapshot,
   cursor or result-bound guarantee, it must be implementable and testable
   for supported placements. Lack of such a guarantee is not an automatic
   exclusion if the product outcome does not require it.
6. Consider total cost and reversibility versus plain domain-owned typed
   capabilities. A reusable abstraction must produce more than
   matching syntax across unrelated domains.

A library's lack of all six properties does not mean the library is
defective; it may be a **provider implementation detail**, a boundary
comparator, or simply irrelevant to the selected product outcome.

## Discovered mechanism families and primary sources

| Mechanism discovered | Verified upstream API/semantics (not Riesel implementation) | Possible role; material question | Primary source |
| --- | --- | --- | --- |
| **Existing typed Riesel `#[query]` operations and ordinary Rust methods/iterators** | `RecordStore` v2 already provides model-scoped read operations, UUID pages and typed `Records<T>` SDK; ordinary Rust code can filter locally, but that does not imply remotely executable predicates or complete selective pages. | **Baseline** for independent domain-owned query APIs. Could suffice if the accepted outcome is narrow; cost of repetitive adapters/read scanning must be quantified. | [Riesel Storage v2](https://github.com/aschenrot/riesel/blob/main/packages/storage/contract/src/storage.rs), [SDK](https://github.com/aschenrot/riesel/blob/main/crates/riesel/src/records.rs) |
| **Rust typed SQL expression DSL — Diesel** | Query DSL uses SQL-clause-like trait methods and backend-aware compile-time query typing. | Possible provider/SDK inspiration; tied to relational SQL and its schema/connection model. Verify how `RieselModel` derives and managed contract descriptions would interact. | [Diesel query DSL](https://docs.rs/diesel/latest/diesel/query_dsl/index.html), [guide](https://diesel.rs/guides/getting-started/) |
| **Dynamic SQL expression/AST builder — SeaQuery** | Construct SQL expressions, condition trees and statements for SQLite, PostgreSQL and MySQL dialects. | Potential **physical SQLite/provider translation** or Rust-local builder; SQL AST is not automatically transport-neutral typed query semantics or cross-provider consistency. | [SeaQuery docs](https://docs.rs/sea-query/latest/sea_query/) |
| **Entity ORM and ordered cursor facilities — SeaORM** | Entity query interface includes partial-model projection and ordered keyset/cursor pagination. | Reference for typed `Records<T>` ergonomics and efficient keyset queries; entity/SQL model might duplicate Riesel's Rust-derived models. | [SeaORM entity API](https://docs.rs/sea-orm/latest/sea_orm/entity/index.html), [cursor](https://docs.rs/sea-orm/latest/sea_orm/struct.Cursor.html) |
| **Compile-time SQL access without a DSL — SQLx** | SQLx supplies compile-time checked SQL macros and async database access; validation depends on DB metadata and SQL backend. | Alternative to new AST for a **concrete relational provider**, but static SQL and live schema-check implications differ from dynamic cross-provider query descriptions. | [SQLx README](https://github.com/transact-rs/sqlx/blob/main/README.md), [query macro](https://docs.rs/sqlx/latest/sqlx/macro.query.html) |
| **Embedded/remote database query language — SurrealDB family** | Upstream Rust clients expose their database/query API; embedded/server query engine can be used under appropriate deployment assumptions. | Engine/operational substitution candidate; examine only if its richer domain semantics and data ownership provide credible net benefit over Riesel's accepted storage boundary. | [SurrealDB Rust docs](https://docs.rs/surrealdb/latest/surrealdb/) (verify package/version), [project docs](https://surrealdb.com/docs/) |
| **Embedded analytical SQL — DuckDB Rust binding** | Rust binding can issue SQL and access Arrow-like batch results; documented Arrow virtual-table integration. | Candidate for analytics/asset-catalog off-line workloads or provider internals, **not** automatically a transactional replacement for current Storage v2. | [duckdb-rs](https://docs.rs/duckdb/latest/duckdb/), [Arrow virtual-table docs](https://docs.rs/crate/duckdb/latest/source/README.md) |
| **Relational query planner / Arrow execution — Apache DataFusion** | Extensible Rust SQL/DataFrame query engine with logical/physical plans and columnar execution. | Reference/possible engine for heterogeneous analytical or large read-only datasets; integration cost, resource profile, full `u64` semantics and transactional authority require proofs. | [DataFusion introduction](https://datafusion.apache.org/user-guide/introduction.html), [project](https://github.com/apache/datafusion) |
| **Local lazy frame expressions — Polars** | `LazyFrame` supports expression filters and deferred collection over frames. | Useful boundary comparator for tabular/batch analysis; a lazy Rust expression API need not be a session-bound remote provider capability. | [Polars lazy guide](https://docs.rs/polars/latest/polars/docs/lazy/index.html), [LazyFrame API](https://docs.rs/polars/latest/polars/prelude/struct.LazyFrame.html) |
| **Portable relational plan representation — Substrait** | Standardized serialized relational algebra with an extension mechanism, aimed at cross-engine/language compute interoperability. | Nontrivial existing **interchange option** for relational scenarios; verify whether its complexity/type/extension semantics fit Riesel records. Not a universal ECS/spatial query ontology. | [Substrait specification](https://substrait.io/spec/specification/), [extensions](https://substrait.io/spec/extending/) |
| **SQL-over-transport / columnar read protocol — Arrow Flight SQL** | Arrow Flight SQL defines client/server SQL metadata and query/response transport over Flight. | Possible future read/analytics transport boundary, **not** a generic replacement for Riesel capability session/authorization by virtue of being an RPC standard. | [Flight SQL specification](https://arrow.apache.org/docs/format/FlightSql.html), [Rust Flight](https://github.com/apache/arrow-rs/blob/main/arrow-flight/README.md) |
| **API selection and resolver graphs — GraphQL** | Schema-driven request selection/projection and resolver execution at an API boundary; not itself a persistent data store. | Distinct external/API query pattern for independent UI; consider projection and security lessons without presuming a GraphQL server or user-facing API is wanted. | [GraphQL queries](https://graphql.org/learn/queries/), [introduction](https://graphql.org/learn/introduction/) |
| **Full-text indexed retrieval — Tantivy** | Rust text-search index/query APIs with parsed search queries. | Relevant when text relevance/search is a real product requirement; secondary index synchronization, per-document source truth and index persistence must be established independently. | [Tantivy Rust docs](https://docs.rs/tantivy/latest/tantivy/) |
| **Incrementally maintained computations — Differential Dataflow** | A framework for efficiently maintaining derived computations as inputs change. | Strong counterexample to equating finite page reads and *subscribed/live* query results. A change-feed/retraction/version model would be a separate product outcome. | [Differential Dataflow documentation](https://timelydataflow.github.io/differential-dataflow/) |
| **Borrow-safe ECS component/world query — Bevy ECS** | Rust system parameter `Query<'world, 'state, D, F>` borrows and filters component data from a World. | Specialized domain comparator; borrowing/structural epochs and task-access constraints are not serializable record predicates by default. | [Bevy ECS Query](https://docs.rs/bevy_ecs/latest/bevy_ecs/system/struct.Query.html) |
| **RunenECS authoritative world/relation query** | RunenECS independently owns its component, relation, world and scheduler semantics; dense spans are borrow/structural-freeze scoped. | Actual Dornglut integration comparator; investigate interoperable *metadata/inspection* only where semantics remain owner-local. | [RunenECS architecture](https://github.com/dornglut/runen-ecs/blob/main/ARCHITECTURE.md) |

The last two rows are **not** the same mechanism as RecordStore. The
analytic, search, incremental, relational and API/GraphQL groups may be
alternatives for **different** user outcomes, or provider-local
implementations beneath a Riesel contract. Treating them as identical
replacements would create false equivalence.

## Coverage and cross-cutting discoveries

- **Logical representation vs executor:** typed SQL builders, remote
  schemas, relational algebra and query execution engines are different
  layers. Adopting SeaQuery, Substrait, SQLx or DataFusion for one layer
  neither authorizes nor automatically solves the others.
- **No universal query guarantees from one term:** `query` also names
  Riesel `OperationKind::Query`, ECS borrowed views, SQL selection,
  GraphQL field projection, and change-maintained results. Each needs a
  stated source of truth, validity and consistency model.
- **A combined solution remains possible:** a Riesel provider could
  implement a bounded typed record-read interface using SQL/SeaQuery
  internally, or an editor could use a specialized search index with
  explicit update guarantees; neither mandates one platform-wide AST.
- **A strong general solution remains possible:** if multiple accepted
  consumers need compatible, transportable typed filtering and ordering
  and common provider negotiation, an extensible shared query
  interface might reduce lifetime integration cost. Evidence is still
  missing for the required semantics and scale.
- **No-abstraction remains possible:** when consumers' query meanings
  differ and cannot be truthfully unified, independently versioned
  domain APIs can be more durable than a weak universal facade.
- **Requirements-coverage gap:** no accepted Riesel-wide analytical
  SQL, federated multi-source query, general full-text search,
  subscription, distributed snapshot, or ECS-query-over-RPC product
  outcome was established by this source survey.

## Next screening work and decision-critical missing evidence

1. Validate what Todo needs beyond the reproducible #107 failures; use
   a separate registered model to test cross-application semantics.
   Establish whether the general platform **product outcome** is
   accepted before implying API acceptance.
2. Build and test a common **semantic pressure-test matrix** for
   distinct mechanisms: typed `u64`, optional/missing/null, UUID,
   ordering under mutation, cursor/session validity, unsupported
   operators, read-only authorization and query-cost rejection.
3. Quantify query/writer **same-instance** enforcement and API/SDK costs
   against a co-exposed versioned record capability, and examine
   real remote constraints without claiming unsupported parity.
4. Only shortlist concrete external dependencies after checking
   licensing, release/source history, MSRV 1.100 and actual integration
   viability. A well-known library is not a verified candidate for
   inclusion in Riesel's production workspace.
5. Determine what would change the decision between independent
   domain APIs, partial common semantics, and a reusable transported
   Query capability; do not assume the answer ahead of experiments.

**Research artifact status:** discovery performed, cross-domain
correctness and cost comparisons **not yet complete**. No production
library selection, Query contract, physical data/index migration,
authorization behavior or performance guarantee approved.
