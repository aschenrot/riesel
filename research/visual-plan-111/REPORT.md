# Riesel #111 — visual plan edit/round-trip research on the current plan API

**Status:** RESEARCH ONLY; no owner acceptance, implementation contract or public
document format is claimed. Baseline: `main` at
`b8ffe4b6d6400315c65d0649527721654c4bff87` (2026-11-22).
Associated specification: [#111](https://github.com/aschenrot/riesel/issues/111).
Independent user-facing graphical inspection: [#114](https://github.com/aschenrot/riesel/issues/114).

## 1. Original outcome and growth horizon

A Rust developer should be able to **visually compose, inspect, validate, and
deliberately publish a supported graph of executable registered operations**
without treating a graphical node as arbitrary executable source. The same
semantics must run headlessly. Existing code-authored Riesel applications must
not lose handwritten Rust behavior or become implicitly editable after
inspection. Longer-term uses include headless services, workflow tools,
event-driven processing, simulation/game frames under separate domain owners,
multiple application workspaces, future versioned typed data edges and optional
trusted editor processes.

An initially read-only inspector fulfills a **different** accepted outcome;
it is not grounds to remove graph editing from this comparison.

## 2. Verified production baseline (source audit)

- `crates/riesel_app/src/execution_plan.rs` defines
  `ExecutionPlanDraft`, `OperationNode`, immutable
  `ValidatedExecutionPlan`, exact-key `OperationRegistry<H>`,
  `BoundExecutionPlan<H>` and process-local `PlanRevisionCatalog`.
- `ExecutionPlanDescription` and its nodes implement **Serialize, not
  Deserialize**. The description explicitly disclaims executable imports,
  a document ABI and visual-editor round-trip. This is a *read-only
  inspection projection*, not an approved source document.
- Validation is side-effect-free, checks identities, cycles, signals,
  resource declarations and single domain-admission ownership. Binding is
  a separate exact-version registered-handler lookup; it neither proves
  a process principal nor starts a provider. Plan publication rejects reuse
  of a logical plan/revision pair only within one local catalog's lifetime.
- Running plans are immutable and owned by existing Riesel sessions; outer
  execution and observation are delivered in #110/#112/#113. #146/#153
  add optional owner-published nested *read-only* observation, not editable
  ECS system schedules or domain-internal controls.
- `docs/product/riesel/execution-plan-contract.md` explicitly defers
  code-vs-document ownership, layout/version boundaries, typed dataflow and
  editor publication. Current app/Provider `AppSchema` is NOT this graph.
- Earlier [#83 research report Gate G](https://github.com/aschenrot/riesel/blob/chore/83-scheduler-research/research/execution-scheduler/REPORT.md)
  tested a **separate research runner** with a versioned document,
  registered operations, negative cases, mock typed-edge checks and known
  weaknesses in combined layout/semantic revisions. The research format
  and its sample runner are not the production SDK.

## 3. Competing source-of-truth architectures (same outcome)

| Candidate | Correctness/round-trip | Developer ergonomics and evolution | Main limitation |
| --- | --- | --- | --- |
| **A. Rust-code authoritative only** | Existing Rust plan and handlers stay authoritative; introspection is straightforward; safe edits must patch a specifically recognized Rust declarative region or generate code and receive owner review | Native Rust toolchain, optimal for handwritten and non-serializable behavior | General graphical edit cannot faithfully reverse arbitrary Rust control flow, macros, closures or borrowed state; AST/source patching is fragile |
| **B. Document authoritative only** | Explicit versioned operation document can round-trip a bounded declarative graph, with compiled registered Rust handlers and strict import/binding | Good offline editor and independent-workspace interoperability; uniform workflow | Cannot make existing code-authored graphs editable without migration; document/schema version policy and publisher trust become new application obligations |
| **C. Shared validated semantics, *one declared authoring authority per plan*** | Code-authored and document-authored inputs converge through the same validator and binder; editor edits may be published only for an explicitly document-authoritative plan; code-owned graphs are inspectable/read-only unless deliberately converted | Preserves simple Rust authoring while leaving a gradual, reversible path to independent editors and future generated declarative APIs | Requires explicit ownership, provenance, operation-catalog/version policy, one-way migration/hand-back rules and preventing ambiguous dual writers |

**Research preference for owner review: C, not accepted.** This is **not**
a generic dual-source-of-truth design. At any moment, one publishing
authority owns the executable semantic revision. A second editor cannot
quietly overwrite it. Compare the implementation/maintenance cost of
C against B before acceptance, rather than using "future-proof" as a
reason to add untested layers. A remains valid for code-only applications.

## 4. Narrow test prototype against the *actual* production plan

`crates/riesel_app/tests/visual_editor_roundtrip_research.rs` defines a
**test-only** explicitly strict document adapter that reconstructs real
`ExecutionPlanDraft` / `OperationNode` values and binds them through the
existing `OperationRegistry`. No production type, parser, public API or
runner is changed.

Research questions and expected assertions (CI must verify them):

1. A code-authored headless/event-driven pipeline and a JSON-round-tripped
   declarative equivalent yield **identical actual production plan
   descriptions**. Its simulation `DelegatedFrame` remains domain-owned.
2. Parsing/validating/binding a registered operation is passive; executable
   callbacks are not called. Missing or changed Rust operation versions
   fail closed. A document never supplies executable Rust code.
3. Unknown semantic fields, undeclared typed data edges, unsupported
   document versions, dependency cycles and conflicting domain owners
   fail before binding.
4. Mutating the semantic graph requires a *new* immutable plan revision.
   Re-publishing an existing ID/revision is rejected in the current
   process-local catalog. The previous validated plan stays unchanged.
5. Presentation layout is modeled as a separately versioned, nonexecuting
   sidecar, anchored to plan ID/revision; a layout-only move cannot change
   the real plan description. Unknown nodes and stale layout revisions fail.

This is an **adapter feasibility probe**, not proof of arbitrary Rust
code round-trip, durable storage, concurrent editors, permission checks,
authenticated external publication or safe code generation. Test fixtures
cannot establish a stable public format merely by successfully serializing.

**Reproduction:** run `cargo test --locked -p riesel_app --test
visual_editor_roundtrip_research`, followed by `cargo validate --check`
and `cargo smoke` at the exact branch head. An independent Rust toolchain
is not available to the session executing this research; GitHub CI must
be checked before marking tests passed. Do not merge the research branch
as production architecture.

## 5. Candidate bounded executable subset (specification proposal only)

- Logical plan ID and immutable semantic revision; fixed registered operation
  key and version per node; stable logical node ID.
- Add/remove node, select existing registered operation, add/remove dependency,
  modify *admitted* signal/resource/domain metadata and validate the entire
  graph again. No partial execution of an invalid edit.
- An operation catalog must expose **editor-safe authoring descriptors**
  controlled by the application publisher. Existing `OperationRegistry<H>`
  exposes executable closures by key; it is not by itself an authenticated,
  independent-process catalog, operation permission declaration or
  schema for editable parameters.
- Only declared, versioned, statically understood fields can be edited.
  Runtime handles, arbitrary Rust code, closures, unknown operation
  parameters, provider-deployment choices, native borrowed state,
  delegated ECS internals, clock ownership and executable typed value
  edges are **outside the first executable subset**. The UI can render
  unknown/unsupported constructs read-only or reject editing; it must
  never silently strip them.
- Semantic execution revisions and presentation-layout revisions are
  distinct. A layout can refer to one semantic revision, be rebased with
  explicit diagnostics or be retained as a read-only historical view; it
  cannot redefine running semantics.
- For code-owned plans: inspection is read-only by default. Permitting visual
  edits requires an explicit source migration or a narrowly generated,
  round-trippable Rust declarative region; no claim of lossless reverse
  engineering arbitrary Rust source.
- For document-owned plans: decode *untrusted bytes* into metadata without
  invoking registered handlers, validate syntax and semantics, resolve
  exact allowed bindings under the owner, and deliberately publish a new
  revision. External publisher authorization is separately gated by
  #75/#60. A valid graph is **not** sufficient permission to execute.

## 6. Publication, safety and compatibility questions still open

1. **Publishing authority and identity:** which application artifact/source
   owns a code/document plan; how a reader verifies it without launching
   untrusted code; where a persistent semantic ID/revision is stored; how
   concurrent editor compare-and-set and distinct app installations are
   differentiated. A `PlanRevisionCatalog` is process-local, not a
   persistent globally authoritative registry.
2. **Operation-catalog descriptors:** which parameters and resource/
   ownership declarations are safe to expose and edit, and whether metadata
   can be obtained from an inert signed manifest rather than running code.
   Registered operation name/version alone does not prove safe binding
   authority across application versions.
3. **Failure/recovery:** edit preflight vs deliberate publication, running
   revision stability, crash between write and activation, stale editor
   revision, app restart with different registration set, invalid migration
   and rollback. Never claim hot replacement, retry of unknown effects or
   exactly-once behavior.
4. **Typed edges:** introduce port types, lifetime/ownership, codec and
   domain-affinity semantics only as a later explicit **executable** contract.
   Merely checking string type names, as Gate G did, does not implement
   transport or safe Rust borrowing.
5. **Source compatibility:** code-owned plan -> document-owned plan
   conversion must require author approval and report unsupported constructs.
   Reverse code generation, format migration and cross-process signed
   publication are separate decisions, not obligations of the first editor.
6. **Independent UI:** #114 may show read-only outer/nested snapshots before
   #111 acceptance, but a *real independent client* still requires #60's
   supported interface and authorization. This research does not authorize
   a browser bridge, native renderer, Manager endpoint or new daemon.

## 7. Bias and decision gate

- **Against short-term bias:** do not choose A solely because current
  `ExecutionPlanDraft` is Rust-only; that would fail accepted long-term
  visual edit/execution.
- **Against speculative abstraction bias:** C must not become a universal
  workflow language, second provider resolver, process manager, typed
  streaming engine or generic Rust serializer. Authoring IR and adapter
  reuse must demonstrate real parity and less migration cost than B.
- **Against successful-prototype bias:** a fixture using the real validator
  is stronger than a standalone sample, but it does not verify untrusted
  publication, cross-version migration or arbitrary source round-trip.
- **Against GUI-first bias:** a read-only #114 inspector and a #111
  executable editor have different acceptance criteria and need not
  share one frontend implementation.

**Proposed specification disposition:** keep #111 **open** pending owner
accept/revise/defer after reviewing (a) the actual-API round-trip probe,
(b) explicit single-writer source of truth, (c) executable editing subset,
(d) security/publisher and rollout boundaries. If accepted, update the
existing execution-plan contract and product definition through a separate
documentation PR, then create independently verifiable implementation work.
Do not treat this research report or CI as architectural acceptance.
