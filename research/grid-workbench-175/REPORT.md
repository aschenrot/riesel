# Grid Workbench #175 — capability and typed-data composition investigation

**Status:** research-only, non-normative. No owner acceptance, production API, core engine, editor, renderer integration or implementation milestone follows from this report.

**Baseline read:** `aschenrot/riesel` main `5c6e9983992e4b94bbc4424816ad2f52bfd6d1ac` (2026-11-22); `aschenrot/grid` and `dornglut/runen-render` default-branch source inspected on 2026-11-22. The latter repositories remain independent authorities; this public report deliberately does not reproduce private grid source. [Specification #175](https://github.com/aschenrot/riesel/issues/175); [execution/editor specification #111](https://github.com/aschenrot/riesel/issues/111).

## 1. Product outcome, not a preselected implementation

A Rust author builds a reusable, Riesel-powered procedural workflow: sampled seeded noise -> scalar samples on a configured 2D grid -> explicit scalar-to-occupancy classification -> configurable synchronous cellular automata with protected border writes -> authoritative dual-grid descriptors -> optional visual preview. The initial proof uses 64x64 logic cells and 65x65 visual corners. The user can inspect/compare results at every stage and rerun exact configurations. Room generation is one application, not a reason to put room/CA/grid types into Riesel core.

**Developer question to answer:** Which behavior must be a Riesel capability, which objects are typed domain data, and what can remain ordinary Rust operations? Assess a second consumer (e.g. noise-derived simulation material/temperature maps or procedural texture masks) rather than claiming generality from the room use case alone.

## 2. Verified boundaries and previously underemphasized constraints

| Boundary | Source-grounded finding | Consequence |
|---|---|---|
| Riesel capability | The SDK derives a versioned contract from a Rust trait; components provide declared contracts and can run linked or through approved managed/external placements. | Capability != library, data type, graph node, process or universal execution engine. |
| Public types | The current `RieselType` scheme contains `u64`, `i64`, `f64`, `bool`, strings, UUID/JSON, `Vec<T>`, `Option<T>` and named non-generic structs/enums. It does **not** directly expose `f32`, `u32` or generic public structures through the generated type schema. | Do not promise direct generated capability signatures returning `DenseGrid2D<f32>`; adapter DTOs or deliberate SDK evolution require investigation. |
| Grid and field | Existing engine-neutral grid code provides typed finite dense 2D storage, coordinates, read/write access and neutral dual-grid topology. | A grid is normally a *value/container*, not a mandatory Riesel service. A sampled scalar field may use grid storage; it is not synonymous with a continuous noise sampler. |
| Current domain helpers | Existing generation produces occupancy from seeded noise/classification in one call; current CA is a fixed rule. | An independently inspectable scalar stage and configurable CA/border policies are actual functionality gaps; do not label them delivered. |
| Visual topology | Existing domain-owned topology has a known 64x64 -> 65x65 mapping and authoritative descriptor semantics. | Reuse upstream authority; a new Riesel-side classification would duplicate and risk drifting from it. |
| Riesel execution graph | Current session-owned graph runner coordinates admitted operations and records outcomes, but its callback signature is outcome-oriented and **has no executable typed value ports**. | An `after` dependency is not a data-transfer edge. Plain Rust can pass values today; full S3 dataflow remains future #111 work. |
| Native rendering | RunenRender offers source-neutral 2D composition, while native window/surface presentation belongs to a host using an appropriate GPU integration. | A real interactive native renderer requires a separate bounded proof; an image export or offscreen render is not proof of a complete window host. |
| GUI security | Deliberately exported application-facing contracts and independent UI authentication/attachment semantics are not the same as exposing arbitrary internal providers. | External GUI publication is not free merely because an internal capability exists. |

Sources: [Riesel README](../../README.md), [authoring contract](../../docs/product/riesel/api-authoring-v3.md), [high-level app](../../docs/product/riesel/high-level-app.md), [execution-plan authoring](../../docs/product/riesel/execution-plan-authoring.md), [execution runner](../../docs/product/riesel/execution-runner.md), [shared operational contract](../../docs/product/riesel/operational-contract.md), #111, #175. The verified `RieselType` coverage comes from `crates/riesel_schema/src/capability_contract.rs` and `crates/riesel_macros/src/derives/types.rs`; the generated capability trait rules from `crates/riesel_macros/src/capability/expand.rs`.

## 3. Candidate capability boundaries — fair comparison

All approaches must deliver the **same** reusable headless pipeline, inspectable data, versioned configuration, deterministic reference behavior, optional provider substitution and a credible future GUI/dataflow path.

| Alternative | Authoring model | Benefits | Costs / limitations |
|---|---|---|---|
| A. Domain libraries + Riesel app lifecycle | Direct typed Rust calls for noise, classification, CA, topology. | No extra DTO/RPC boundary; ordinary Rust types and borrow rules; least duplicated schemas; good correctness baseline. | Little runtime provider substitution for individual algorithms; generic capability-composition story untested. |
| B. Behavior-specific Noise and CA capabilities; field/grid are data | Riesel resolves compatible noise and CA providers; typed Rust transformations connect outputs; topology remains domain operation. | Reusable contracts, provider substitution, runtime-managed behavior, distinct semantics for noise versus CA. | Public generated types/serialization are currently narrower than domain types; local and remote payload cost and schema ownership need proof. |
| C. One coarse procedural-grid capability | A provider performs the whole configured transformation chain; app requests finished artifact/snapshot. | One coherent data owner, fewer large transfers, easier service placement and caching. | Coarse-grained substitution, less independent reuse, risks hiding intermediate semantics and stage editability. |
| D. A capability for every operation/data transition | Each sample, threshold, CA iteration, topology step is independently resolved. | Uniform-looking diagram; possible algorithm-level substitution. | Excess contracts/versioning/DTOs and potential RPC/copy churn; no current generic value-edge runtime; does not make execution nodes automatically capabilities. |

**Provisional research preference: B when the supported contract/type and transfer proof succeeds, otherwise A plus an explicit reusable interface at the domain boundary.** C may win for independently managed high-volume processing; D requires unusually strong empirical evidence. This is NOT an owner decision. Equally test the cost of B's type-adaptation/serialization against A's composition simplicity and C's locality.

### What the developer should think

- *"I want noise"* -> select a behavioral provider if choosing algorithms/execution independently adds value; otherwise call a library function.
- *"I want a field"* -> specify mathematical sampling semantics and/or a concrete sampled-value artifact.
- *"I want a grid"* -> use a typed domain data structure; a `GridStorage` capability becomes relevant only with identity, persistence, shared/query access or managed ownership.
- *"I want CA"* -> use a configurable rule-execution behavior; a capability is plausible if providers genuinely interchange.
- *"I want to combine them"* -> author the data transformations in ordinary Rust today; do not interpret control-only `ExecutionPlan` dependencies as typed value transfer.

## 4. Minimum contract semantics to test (not a frozen Rust API)

**Noise source:** explicit dimension, coordinate space/origin, sampling spacing, seed, algorithm/algorithm-version, finite-value/range policy, boundary/continuity semantics and a documented reproducibility profile. Separate (a) sampling a function from (b) materializing samples. A materialized 2D scalar artifact must carry shape/layout, sample type, coordinate mapping, provenance and resource bounds.

**Occupancy conversion:** explicit classification operation from sampled values to occupancy states; threshold comparison (>, >=), NaN/non-finite behavior, equality rule, normalization and coordinate propagation are specified rather than silently chosen.

**Cellular automata:** state alphabet and typed input, neighborhood (Moore/von Neumann or custom validated offsets), transition policy, synchronous previous-state reads, out-of-bounds *read* policy, write-exclusion mask/border thickness, iteration count, result snapshots and cancellation-safe iteration boundaries. Border **preservation** is not equivalent to forcing borders to walls. Every protected cell is unchanged **after each iteration**, including when its original value is empty.

**Dual-grid:** preserve the existing upstream coordinate, occupancy-to-corner, mask-bit, kind, rotation and out-of-bounds semantics; verify the 16 masks and 64x64 -> 65x65 case from real domain code. Keep actual tile descriptors as data and renderer-specific mesh/color/geometry selection in presentation adapters.

**Snapshots and edits:** each stage result binds to the exact source input revision, stage config/algorithm version, execution attempt and owner. A changed upstream parameter invalidates dependent results but cannot overwrite a newer successful revision after a delayed completion. Bound snapshot retention and failures are visible; no unbounded per-iteration history by default.

## 5. Local data versus published payloads: decision-critical test

Using 64x64 cells, a dense 32-bit scalar array has a **raw payload lower bound** of `4096 * 4 = 16,384 bytes` (16 KiB). A 512x512 scalar array is 1 MiB, and a 2048x2048 array is 16 MiB. These are arithmetic values **not measured allocation, JSON/wire payload size, GPU throughput or serialization timings**. A sequence retaining ten 2048² scalar snapshots would require at least 160 MiB of raw scalar data alone, excluding other stages, metadata and copies.

Therefore compare:
1. Ordinary in-process `DenseGrid2D` values passed by ownership/borrow.
2. Riesel linked capability handle using supported derived domain DTOs; quantify adapter copies even when there is no TCP.
3. Approved managed provider with bounded serialized payloads; validate total message size, memory peak, timeouts, error semantics and compatibility. Avoid promising arbitrary grid transfers if framed-RPC limits reject them.
4. Coarse-grained provider preserving internal grid locality and publishing a bounded snapshot/region, plus explicit client caching/invalidation.

**No automatic zero-copy, process transparency, generic GPU handles, borrowed-grid transfer or typed dataflow is established by the existing SDK.**

## 6. Reproducible next proof / acceptance gates

**Gate 1 — Contract conformance:** write one minimal candidate Noise and CA Riesel trait/component in an isolated research fixture; prove that actual current supported public types compile and generated app handles resolve with linked providers. Also compile-check a deliberately unsupported generic/f32 form and record the expected error. Compare an equivalent direct-Rust baseline and a coarse component. Do not alter public SDK types to conceal limitations.

**Gate 2 — Real domain correctness:** use the *actual* grid/topology implementation, not duplicate private code, in an authorized isolated environment. Seeded scalar 64x64 samples -> explicit threshold -> 4 configurable synchronous CA steps with mixed initial border values -> 65x65 visual corner output. Test exact replay per declared determinism profile, protected cells after *each* iteration, OOB read alternatives, 16 topology mask cases, invalid configs and stale revisions.

**Gate 3 — Cross-boundary cost:** measure linked DTO vs direct call, then a managed provider where permitted, at 64, 512 and a justified larger dimension. Report the exact toolchain, CPU/platform, source SHAs, payload bounds, serialized bytes, wall-clock and memory; never infer performance from raw byte arithmetic. If private grid access is not available to Riesel CI, keep the experiment in an authorized isolated/private environment and publish only non-sensitive results.

**Gate 4 — Native preview:** use a distinct, tested RunenRender 2D descriptor-to-geometry adapter with actual RunenGPU host presentation. Verify frame/surface lifetime, input/zoom/pan, updates, size limits and failure/disconnection handling before choosing a reusable rendering capability contract. No production independent UI export/authorization is implied.

**Gate 5 — Product decision:** propose exact versioned public types (or a justified decision to retain direct calls), record A–D evidence, second non-room consumer, migration and S2->S3 compatibility implications, then obtain the maintainer's accept/revise/defer decision in #175. Only then update accepted product/architecture contracts and plan independently verifiable implementation work.

## 7. What is / is not completed

**Completed by this report:** repository source inspection, explicit responsibility model, source-grounded SDK type constraint, fair preliminary alternatives and executable acceptance plan.

**Not completed:** compiled Noise/CA capability fixture, independently tested real Rust 64x64 implementation, CA rule extension, benchmark, native host experiment, GUI interaction, owner architecture acceptance or production code. Existing Riesel tests and grid topology tests do not substitute for these cross-repository integration proofs.

This report is deliberately compatible with one-authoritative-plan-source and S2-to-S3 evolution under #111. New data-artifact and revision semantics must not silently create another executable plan publisher.
