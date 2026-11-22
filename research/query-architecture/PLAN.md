# Riesel general-purpose Query — independent research plan (#138)

**Status:** research scope proposal; **not** an accepted product requirement, architecture, API or implementation plan.
**Baseline:** `aschenrot/riesel` `main` at `93b5188d122f5fa35a3bf047d6d068995c1d74b5`; 2026-11-22.
**Decision owner:** Riesel maintainer/product owner through [specification #138](https://github.com/aschenrot/riesel/issues/138).
**Related motivation:** [Todo specification #107](https://github.com/aschenrot/riesel/issues/107), [research PR #130](https://github.com/aschenrot/riesel/pull/130). Those experiments are evidence, not acceptance of a query interface.

## Why this research exists

Riesel already defines versioned typed capabilities, Rust-derived model metadata,
and `riesel.storage.records@2` with CRUD and UUID-ordered bounded record pages.
Todo exposed sparse-match paging limitations; an editor or other application
might need typed filtering of durable metadata. It is **not yet an accepted
product requirement** that every heterogeneous data source share one
query DSL, protocol, planner or runtime.

The decision is whether a **reusable query foundation** is justified for
credible Riesel application needs, and if so, what should be shared
without transferring correctness, scheduling or mutable-state authority
away from the real data owner. The answer may be **none, partial reuse,
or a general typed capability**, with a separately justified delivery scope.

## Reading and existing facts to verify at the start of substantive work

Read the actual current `main`, including:

- `README.md`, `CONTRIBUTING.md`, `docs/project/delivery.md`;
- `docs/product/riesel/riesel.md`, `api-authoring-v3.md`,
  `implementation-roadmap.md`, `provider-catalog.md`,
  `runtime-lifecycle.md`, `async-execution-contract.md`,
  `execution-plan-contract.md`;
- `docs/product/todo/todo.md`, `http-local-browser-contract.md`,
  `packages/storage/contract/src/storage.rs`,
  `crates/riesel/src/records.rs`, `crates/riesel_schema/src/*`;
- `docs/product/riesel/durable-record-storage-architecture-proposal.md`,
  `sqlite-offline-migration-architecture.md`;
- #107 and draft PR #130, their exact revision/CI and research-only fixtures;
- #75 (shared operational boundaries), #83 and accepted
  `execution-plan-contract.md`, for vocabulary/authority overlap;
- actual independently versioned RunenECS/Runenwerk architecture if
  assessing a game- or editor-specific scenario. External repository
  decisions are **comparators**, not automatic Riesel requirements.

Confirm whether the accepted requirements or implementation have changed
since the dated research baseline; distinguish intended outcomes from tests
that prove only current reference behavior.

**Documentation-status discrepancy to reconcile:** at the audited `main`
revision, `docs/product/riesel/riesel.md` and the introductory status in
`durable-record-storage-architecture-proposal.md` still describe the
operator preview/consent workflow as undelivered, while the
`implementation-roadmap.md` says its bounded first supported increment
was delivered and [implementation #51](https://github.com/aschenrot/riesel/issues/51)
is closed with merged-main CI evidence. For any proposed index/physical
migration, verify exact accepted invariants and implemented behavior
against the issue, commit and tests; do not silently choose the more
convenient status statement. This is a documentation-consistency finding,
not authorization for automatic index migration or a new query provider.

**Terminology trap:** Riesel's `#[query]` / `OperationKind::Query` classifies
a capability **operation**. It is not evidence that Riesel has, or needs,
a generic selection/query language. An execution-plan graph, an ECS
borrowed-component query, a storage predicate, a spatial query, and an
observation/inspection view are not automatically the same abstraction.

## Discovery stage: establish the *outcome* before selecting a mechanism

Evaluate at least three clearly distinct use classes:

1. **Required motivating case:** Todo Open/Completed retrieval with bounded
   and honest paging, correct revision semantics and no false empty state.
2. **Credible reuse probe:** schema-registered *other-app* records such as
   asset/editor metadata, settings or search. Establish actual query
   predicates, reads, consistency and operational needs rather than
   inventing them from aspirational product labels.
3. **Boundary challenge:** independent ECS/world, spatial, graph, streaming,
   event log or snapshot case. Examine whether any **shared semantics**
   genuinely reduce duplication or whether an owner-specific adapter is
   clearer, faster, safer and more versionable.

State **required outcomes**, plausible growth scenarios, preferences and
explicitly unaccepted product directions separately. Do not require all
example cases to use one interface; nor presume they must be separate.

Ask which shared semantics are useful to an application author, not only
which words (`filter`, `query`, `graph`) appear in domain APIs.
Evidence of consistent behavior across multiple consuming applications
matters more than superficial similar syntax.

Explicitly compare **Rust-local expressive predicates** (including
closures or borrowed state that cannot be transported) with
**serializable, versioned query descriptions** consumed by managed or
external providers. Neither model is a transparent substitute for the
other. Distinguish compile-time Rust typing and generated schema
metadata from runtime schema/version negotiation; state which
unsupported combinations fail at binding, preflight or call time.

## Open alternative discovery before any shortlist

**Discover candidates rather than treat the initial architecture sketches as
the set of answers.** The examples that triggered #138 (Todo, RecordStore v2,
separate Query, Storage v3, specialized ECS) are sources of questions and
known reference designs, **not a preselected evaluation shortlist**.

**Step 1 — independent discovery log.** Work from required outcomes and
plausible growth scenarios. Search relevant primary sources on Rust
query/ORM/DSL libraries, embedded and service query interfaces,
typed expression builders, domain-specific query systems, standards and
general data-processing approaches. Inspect Riesel's existing
capabilities and relevant independent engines. Capture the date,
source/version, claimed capabilities, ownership/placement assumptions,
and why each materially distinct idea could be applicable. State what
search scope was actually covered and what remained unsearched.

**Step 2 — transparent screening.** Establish minimum semantic/security/
compatibility requirements *before* excluding an approach. Separate
requirements from preferences. Screen candidates using the same criteria;
label insufficient evidence **unknown**, not unsuitable. Keep a record of
the origin, inclusion, exclusion and important open risk for every screened
family. Do not infer that the most familiar four examples are exhaustive.

**Step 3 — construct alternatives.** Synthesize concrete candidate designs
from the discovered approaches, including non-obvious hybrids and
compositions. Allow an independently discovered architecture to replace
all prior sketches. A solution may reuse vocabulary without a common
execution API; use a common protocol for a narrow domain; combine
specialized query interfaces with optional inspection; or take a quite
different supported form. Do not force a candidate into a taxonomy just
because it mentions `query`, `filter` or `plan`.

**Step 4 — analyze independent questions only after discovery.**
For each viable candidate, separately report:
- **Semantic sharing:** if anything is reusable, what exact guarantees
  and consumers make it valuable?
- **Record integration:** if durable-record querying is needed, what
  interface/compatibility shape makes it safe and maintainable? Existing
  v2 adapters, a separate versioned Record Query and an updated Storage
  contract are merely reference examples to screen, not required winners.
- **Physical realization and ownership:** what executes/validates a query,
  where, for whom, with what resource/consistency/session guarantees?
  Local, managed and external placements must not silently share guarantees.

The three questions are **analytical dimensions, not three mandatory APIs,
packages, layers or phases**. If the best candidate does not fit these
questions cleanly, document why and adjust the analysis. Existing
provider/session/authorization authority remains an explicit compatibility
constraint, not an argument for a particular query abstraction.

## External alternatives and applicability

The discovery log must be prepared before ranking or narrowing proposals. Prior
proposals are reference cases, not the candidate boundary. Do not design a homegrown Query DSL from memory. Discover and screen
materially different existing Rust data-query APIs, typed query builders,
relational/document query approaches, and standardized query representations
or engines **where their semantics could satisfy the stated product
outcome**. A candidate being familiar, popular, SQL-based, Rust-only or
compatible with SQLite is neither sufficient nor disqualifying.
Separate logical API libraries, provider/backend engines, transport
protocols and physical indices. For any shortlisted candidate, verify
its current API, licensing, maintenance, Rust 1.100 compatibility,
deployment model and actual provider boundary from primary sources.
Give brief evidence-based reasons when excluding an approach. Do not
add every library or standard to the test matrix without evidence it
could change the architecture decision.

## Common comparison criteria and explicit falsifiers

Compare candidates on the **same** accepted/credible outcomes:

- **Correctness:** typed predicates, exact `u64`/UUID comparisons,
  optional/missing/null semantics, ordering, invalid predicates,
  type/schema drift, contradictory filters and cursor provenance.
- **Consistency:** explicitly supported per-page/live or snapshot meaning,
  concurrent updates, multiple readers, completeness and stable/exhausted
  reports, no implicit multi-record atomicity.
- **Authority/security:** who can inspect, execute or mutate; provider and
  instance identity, session fencing, authorization, costs from delegated
  or externally connected readers, no unintended data disclosure.
- **Scalability/operations:** indexed and unindexed selectivity,
  query memory/CPU/time/lock budgets, backpressure, RPC frame bounds,
  cancellation and denial-of-service amplification; reproducible data
  size and concurrency profiles, not guessed throughput.
- **Durability/evolution:** independent capability/protocol versions,
  migration/rollback and index upgrades, stored model identity, online/offline
  compatibility, reversibility and platform placements.
- **Developer experience/maintenance:** generated Rust typing, query
  composition, diagnostics and inspection, optionality for simple apps,
  number of required interfaces and total long-term maintenance burden.

**Falsifiers / pressure tests:** a generic contract that reports incorrect
numeric matches, promises unsupported snapshots, hides an O(N) scan behind
a bounded page, gives separate writer/query aliases inconsistent state,
requires ECS runtime values to cross RPC/serialization, or cannot be
versioned without changing unrelated Storage semantics does *not* satisfy
its advertised guarantee.

## Bounded evidence plan

First **inventory existing evidence**; do not rerun PR #130's tests unless
their exact SHA, inputs or reliability is materially in doubt.

Use paper/typed-schema contract sketches to pressure-test the *discovered viable designs* against
two concrete registered models and a specialized-domain counterexample.
Sketch old/new query versions, incompatible cursors, missing/null,
full-width numeric equality and unsupported feature responses.
Illustrative names/types are **not** proposed public interfaces.

Only then run the smallest focused experiment that changes the choice:

- Native SQLite/Memory predicate parity for two model shapes and
  high/low selectivity, including exact-number and missing/null cases.
- Provider selection/provenance failure and stale session/cursor tests
  where a reusable interface claims writer/query co-location.
- End-to-end RPC/load/cancellation tests **only if** remote managed
  performance or ownership is necessary to distinguish viable targets.
- A representative RunenECS/Runenwerk case only if the comparator
  could change the evidence-backed boundary between shared semantics and domain-owned execution. No mandatory game dependency.

Record toolchain, machine/CI, provider versions, dataset, exact source
revision, commands, timings (if measured), failures and limitations.
Unmeasured extrapolations must be labeled analytical. Physical indexes
must not be installed in live user databases by an experiment.
Research-only code belongs in an isolated/dedicated research path and
must not silently publish product APIs.

## Bias checks, evidence rule and stopping condition

- **Short-term bias:** do not dismiss a reusable foundation because it
  costs more than a Todo patch; estimate duplication and migration costs
  across credible later consumers.
- **Abstraction bias:** do not accept generality because query syntax
  looks unified; test semantics, heterogeneous owners and reuse value.
- **Implementation and shortlist bias:** existing SQLite, RecordStore v2,
  SDK macros, RunenECS and prior #130 recommendations are evidence, not
  required winners. Recheck the candidates and exclusions independently
  of the earlier A0–A3/B0–B3 scheme; disclose which proposals were found
  only after seeing the preferred answer.
- **Example bias:** one Todo testcase cannot define platform contract;
  hypothetical massive datasets or games do not establish accepted
  functionality either.
- **Security/portability bias:** never infer authentication, process
  identity, portable locking or consistent numeric operators from local
  metadata/JSON features.

Deliver a dated **REPORT.md** or comparably reviewable research findings
in this draft research PR, not only a chain of issue comments. Include:
proven baseline, candidate map, comparison with the same criteria,
falsifying experiments, costs/unknowns, an explicit recommendation
**or deferral**, out-of-scope semantics, and a first-implementation
vs extension policy.

**Stop** when all viable materially distinct candidates have been
compared sufficiently to make the product/reuse decision, and
additional experiments are unlikely to change it. Do not keep adding
data engines, DSLs or tests to satisfy a universal coverage checklist.
Defer honestly if a decisive capability/ownership gap remains.

## Separate gates and lifecycle of this research artifact

**Research-plan review:** Maintainer can review the outcome, boundaries,
candidate axes, evidence rules and stopping condition in this document.
No architecture selection or implementation authorization follows.

**Research-results review:** The draft PR must contain reproducible
evidence, a fair comparison and explicit recommendation/deferral,
then link the conclusion to #138. Whether this research-only branch
is merged into `main` depends on whether its documentation is
durable and appropriate there; experimental crates/APIs are not
production code and may instead be closed unmerged while preserving
the review and CI trail.

**Specification decision (Issue #138):** Maintainer records
accept/revise/defer/reject with rationale. Only accepted requirements
and technical invariants move into authoritative product/architecture
documents. Implementation issues are created separately against the
accepted *initial* subset. Do not manufacture an additional Research
Issue merely because this investigation has multiple steps.

**Todo #107 coordination:** A general-query investigation is **not**
a blanket prerequisite for fixing misleading Todo pagination.
Before publishing a cross-application Query API that depends on the
same semantic abstraction, explicitly coordinate the shared decision;
other independent Todo UX work may proceed on its accepted boundary.
