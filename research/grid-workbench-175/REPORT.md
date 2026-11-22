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

**Gate 3 — Cross-boundary cost:** quantify linked DTO vs direct call, then provider-managed preflight and latency/memory costs where permitted, at 64, 512 and a justified larger dimension. Report the exact toolchain, CPU/platform, source SHAs, payload bounds, serialized bytes, wall-clock and memory; never infer performance from raw byte arithmetic. If private grid access is not available to Riesel CI, keep the experiment in an authorized isolated/private environment and publish only non-sensitive results.

**Gate 4 — Native preview:** use a distinct, tested RunenRender 2D descriptor-to-geometry adapter with actual RunenGPU host presentation. Verify frame/surface lifetime, input/zoom/pan, updates, size limits and failure/disconnection handling before choosing a reusable rendering capability contract. No production independent UI export/authorization is implied.

**Gate 5 — Product decision:** propose exact versioned public types (or a justified decision to retain direct calls), record A–D evidence, second non-room consumer, migration and S2->S3 compatibility implications, then obtain the maintainer's accept/revise/defer decision in #175. Only then update accepted product/architecture contracts and plan independently verifiable implementation work.

## 7. Verification state and exact evidence

**Completed source audit:** Riesel capability macros, public schema types, execution graph and RPC framing; authorized upstream grid/topology semantics; RunenRender host/presentation boundary. Comparative A–D architecture remains an investigation, **not an accepted architecture decision**.

### Gate 1 — actual generated linked-capability contract proof

The standalone `research/grid-workbench-175` Cargo workspace uses generated Riesel capability and component macros with three **test-only** contract alternatives:

- A synthetic, deterministic Noise provider returning a bounded 2D scalar DTO (not a real noise algorithm).
- A CA provider applying genuine binary synchronous Moore-neighborhood research rules, explicit out-of-bounds read policy, configurable birth/survival thresholds and protected border writes (not the authoritative production grid library).
- A coarse WholeGridPipeline provider executing the same synthetic source, ordinary threshold and CA rules behind one capability contract.

The Riesel application **actually resolves all three linked providers**. It compares the result of separate Noise -> Rust threshold -> CA calls with one coarse pipeline call. Its topology is not derived in this fixture; the separate private grid tests own that proof.

Reproduce with Rust 1.100:

```sh
cargo test --manifest-path research/grid-workbench-175/Cargo.toml -- --nocapture
cargo run --manifest-path research/grid-workbench-175/Cargo.toml
```

**Exact test evidence:** [Linux Research Grid Workbench workflow 44994302679](https://github.com/aschenrot/riesel/actions/runs/44994302679) **PASS**, source SHA `b3aa6062a4159b9c5e1f12cd757b32417400086b`. All **9 tests passed**, including separate/coarse semantic equality, strict thresholding, invalid dimensions/non-finite values, CA read-boundary alternatives, preserved mixed borders after *every* step, synchronous transition ordering, invalid rules, measured payload serialization and a real two-endpoint generated asynchronous TCP RPC test. The linked-provider Riesel application launched and printed `Riesel #175 SDK composition + rule CA proof: 64x64 cells`. Tests do not establish local/managed performance parity, production grid domain behavior, authorized remote clients, replay after crash or S3 executable dataflow. The first fixture commit failed because macro-required direct Cargo dependencies were omitted; subsequent revisions fixed that and passed.

### Gate 3 partial — real serialized size evidence and framed limit

The test emitted the following **Serde JSON DTO bytes**, for a deterministic repeated full-precision `f64` sample, including JSON field names but **excluding RPC envelope**:

| Artifact | Measured serialized size | Source |
| --- | ---: | --- |
| 64x64 `ScalarGridDto` | **81,956 bytes** | [same CI 44994122494](https://github.com/aschenrot/riesel/actions/runs/44994302679) |
| 512x512 `ScalarGridDto` | **5,242,918 bytes** | [same CI 44994122494](https://github.com/aschenrot/riesel/actions/runs/44994302679) |

The current Riesel `crates/riesel_protocol/src/tcp/wire.rs` sets `MAX_FRAME_BYTES = 4 * 1024 * 1024`, and asynchronous client/server paths reject oversized framed request/response JSON. Therefore, **this valid 512x512 scalar DTO cannot fit into one current RPC frame**. Some other grids or encodings can fit; no universal dimension limit or automatic zero-copy is inferred.

**Real transport experiment, not a multi-process test:** The fixture created *two separate loopback TCP listeners within the same Rust process* with actual generated `NoiseGenerationAsyncRpcDispatcher` and `CellularAutomataAsyncRpcDispatcher`. A separately bound generated Noise client retrieved a 64x64 scalar grid, the ordinary-Rust threshold conversion produced typed occupancy data, and a separately bound generated CA client returned a 64x64 result matching the direct algorithm. A separate full-precision Noise provider returned a 512x512 scalar grid and the actual remote client observed a `CallError::Rpc(RpcError::Transport(_))` when the oversized response was rejected. This test exercises **actual TCP framing, serialization and generated RPC**, not simulated frame rejection; **both hosts and clients were in the same OS process**, so it does not prove independent-process lifecycle/isolation, process-managed placement or externally authorized caller access. It uses intentionally unauthenticated test-only loopback clients; it is not an application-managed provider-selection, retry, authenticated-export, stream/chunk protocol, benchmark, cross-platform guarantee or domain-library integration.

This directly illustrates why capability providers should not require routine transfer of every intermediate domain grid over IPC. Preserving local typed values, an owner-side coarse pipeline, bounded chunking/streaming or separate typed data transfer may be considered if evidence justifies them. Do not bypass frame limits or silently replace typed binary semantics with arbitrary JSON.

### Gate 2 — actual upstream grid domain

A separate **private draft grid research PR** exists, testing the real existing noise-to-occupancy helper, actual authoritative dual-grid topology and a test-local configurable CA rule. The private GitHub Actions [run 44993418430](https://github.com/aschenrot/grid/actions/runs/44993418430) failed on *both* attempts **before a runner started any steps**, leaving no Rust compiler/test evidence. No exact source-level failure or cause of CI rejection has been established. The owner subsequently reported that both the focused `grid_generation` research tests and the `tile_topology` tests **passed locally**. That is useful local execution evidence but **not independently recorded exact-head CI evidence**; the remote GitHub workflow still did not run any steps. Do not merge on this record alone.

### Remaining work before owner decision

- Run the actual private grid-domain tests on an authorized Rust environment, then evaluate integration with live Riesel handles without copying private source into this public repository.
- Compare **developer authoring cost**, local data ownership, DTO copying, Riesel-managed provider placement and failure semantics for A, B and C. Separate/coarse parity and generated two-endpoint TCP tests establish interoperability/bounds, not comparative measured performance or managed-process orchestration.
- Demonstrate a second consumer not limited to room layouts, define noise sampling and field/grid coordinates, algorithm versions, snapshot/recompute authority and stage configuration.
- Establish the separate RunenRender 2D/native-host proof only against its accepted product scope and enforce #60/#75 external-session authorization requirements when relevant.
- Record the owner accept/revise/defer decision in #175. Maintain #111's one semantic plan-author and S2->S3 compatibility without asserting executable typed value edges today.

**No public Noise, Field, Grid, CA, topology or renderer capability has been accepted.** No test is evidence of production-ready security, UI, performance, managed transport or immutable revision persistence.


## 8. Critical graph-authoring correction (follow-up audit)

The accepted visual editor specification [#111](https://github.com/aschenrot/riesel/issues/111) is now **closed**: C+S2+V2+A2+B2+C3 were owner accepted and integrated. The first editor-safe trusted-local SDK catalog/binder has an implementation increment. The separate future *executable typed value ports* and compatibility requirements belong to [#187](https://github.com/aschenrot/riesel/issues/187), not reopening #111 or inventing a competing `Grid Workbench` scheduler. See [research PR #200](https://github.com/aschenrot/riesel/pull/200) for a current production-runner investigation.

The Workbench user's desired **editable node-and-typed-port UI** is a product UX direction, not proof that S2 `after` edges are executable value connections. A 5-node read-only graph projection is not the complete editor. `Render2D` is optional and effectful with a host/presentation boundary; grid artifacts remain authoritative. Ordinary Rust processing and a domain-owned snapshot/preview can make useful headless progress without creating a second general-purpose graph executor. Strong typed editing may need #187 S3 admission and ownership mechanisms to be accepted before selecting the final app/editor implementation.
