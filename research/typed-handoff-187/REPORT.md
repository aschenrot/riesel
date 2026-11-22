# S3 typed handoff and Grid Workbench node-graph boundary — research #187 / #175

**Status:** bounded research fixture only. No accepted S3 runtime API, document format, typed graph interpreter, editor, transport, or new Grid capability. Created against Riesel `main` `59658d58fed2b790759e2ed3db9dd67e816f2d89` on 2026-11-22. See [specification #187](https://github.com/aschenrot/riesel/issues/187) and [Grid Workbench #175](https://github.com/aschenrot/riesel/issues/175).

## 1. Original outcome, context and accepted non-regressions

Riesel already offers S2 control-dependency plans over exact-version registered Rust operations and a bounded session-owned runner. Owner-accepted #111 C+S2+V2+A2+B2+C3 protects one semantic authoring source, immutable semantic plan revisions separate from layout, app-owned editor-safe metadata and independently authorized publication/activation. **#111 is closed and accepted, not a missing architectural specification.** The first A2 trusted-local `OperationRegistry::register_editor_operation` / `bind_editor_plan` increment is present on this baseline. The future S3 typed value port/edge model is **not implemented or accepted**; #187 is the owner of that investigation.

The concrete user-facing reference is the editable `Noise2D -> Threshold -> CellularAutomata2D -> DualGrid2D -> [optional] Render2D` graph. A node is an operation; a capability is a separately versioned behavior contract and may implement multiple operations. The first data types are `ScalarGrid2D`, `OccupancyGrid2D`, `VisualTileGrid2D`, with explicit version/coordinates/shape and error semantics. `Noise2D` should be an easy eager sampled scalar producer; later field-valued/lazy alternatives are independent optional node types. Preview/render output is not the sole source of canonical data.

## 2. Alternatives against the same reusable platform outcome

| Alternative | Owner of node/value semantics | Advantage | Critical limitation / proof needed |
| --- | --- | --- | --- |
| A. App-owned typed Rust artifacts, existing Riesel S2 outer control graph | App/domain owns value snapshots and transformations; Riesel owns only operation admission | Immediate real Rust performance and no protocol for each grid | Typed port validation, propagation/failure, fan-out retention, recompute and revision-fencing risk duplication across apps; cannot call it S3 |
| B. Optional typed handler adapters in the SAME Riesel runner (S3) | Riesel validates data/control connections, schedules eligible outer nodes; app/domain owns underlying large artifacts | Strong reusable node semantics and S2 compatibility; candidate path to generic authoring | Must test `Send`/`Sync`/`!Send` affinity, data readiness, cancellation, allocation budgets, versioned codecs, stale result handling, errors and S2 overhead |
| C. Coarse domain-owned dataflow, Riesel outer admission | Domain graph owns its internal schedule, Riesel only owner-boundary controls/inspection | Best data locality and domain-suitable GPU/ECS/frame semantics | Duplicated authoring conventions may arise; cannot create competing executable plan authority for the same Riesel-owned operations |
| D. Capability / Query / artifact-reference handoff | Data owner exposes bounded typed read or region references; runner coordinates contracts | Avoids transporting entire grids between providers | Reference lifetime, consistency/grants, ownership, backpressure and remote authorization are nontrivial; record Query is not universal GPU/grid data transport |

**Provisional direction, not owner acceptance:** keep the first real Grid pipeline as typed Rust/domain code; use it as a reference workload to evaluate B versus C (with D for managed data locality). Do not build a second general-purpose editable node scheduler inside #175 while #187 is open. An application-local *read-only* graph/preview and fixed typed operations can be implemented before S3 without claiming visually editable executable value edges.

## 3. Executable experiment and what it proves

`src/main.rs` uses **the production Riesel `Application`, `OperationRegistry<AsyncOperation>`, `ExecutionPlanDraft`, `OperationNode.after`, `context.execution_graph`, run report and observer**, not a research scheduler.

It registers three operations: synthetic sampled scalar source; occupancy threshold; and a separate **non-dungeon scalar/heat summary**. The latter two depend on the source's control completion and consume an **application-owned `Arc` immutable scalar snapshot**. The runner's control fan-out and report identity are real; the typed data, revision check and outputs are kept in ordinary Rust `Arc<Mutex<...>>` state. The fixture explicitly demonstrates that `.after("scalar")` does **not** itself represent a typed value edge. It compares successful Riesel report/revision with app-owned result snapshots. A toy preflight rejects incompatible port types and an explicit conversion test checks scalar->occupancy; neither type check belongs to the current SDK. This toy does not own scheduling or publication.

Reproduce with Rust 1.100.0 from a checkout:

```sh
cargo test --manifest-path research/typed-handoff-187/Cargo.toml
cargo run --manifest-path research/typed-handoff-187/Cargo.toml
```

**Executed focused tests (source SHA `06acf302d5f0c2e7e43de512861c85a89b6461f2`):** [Research S3 Typed Handoff #44996709031](https://github.com/aschenrot/riesel/actions/runs/44996709031) **PASS** — **3/3 tests** passed and the headless executable ran. The tests proved the real runner admitted a source and two dependent callbacks with a matching run/report identity; the application, not Riesel, owned the shared `Arc<Vec<f64>>` sampled values, produced occupancy and a separate heat summary, checked simple source revision, and rejected one implicit port type conversion in toy local validation. The success does **not** prove S3 executable ports, cross-process/shared-memory access, authoritative persistent revision CAS or a production reusable dataflow runner. Full Linux Validate/smoke is a separate check and must be reviewed on the final PR head.

## 4. Critical gaps / falsification plan

1. **Prove actual headless Grid semantics separately.** The private `aschenrot/grid` PR #1's domain tests were reported passing locally by the owner; its GitHub Actions job never started, so no exact-head CI proof from that repo. `grid_generation` currently fuses noise and threshold into occupancy; a real scalar Noise2D stage does not yet exist. The Grid's production CA helper does not yet expose the configurable border-safe rule. Local tests use test-only code. Do not equate synthetic field samples, test-local CA, production Grid API or shipped Riesel capability.
2. **Correct cross-process claims.** `aschenrot/riesel` research PR #184 tested real *async TCP* with **two listeners and two dispatchers within one process**, not two independent OS processes. It proved serialized frame handling, not managed-process lifecycle, authenticated peer selection, crash recovery or safe external publisher. The `512x512` full-precision JSON example exceeded the 4 MiB reference frame, but is not a universal dimension threshold.
3. **Test ownership and placement.** Evaluate domain-local `Arc`, explicit region/chunk/artifact handles, bounded out-of-process codec, and `!Send` domain delegation. A component exposing two capability traits does **not** eliminate return/retransmit when an external app calls each RPC and receives full grid values.
4. **Test S3-required semantics against production runner.** Separate port/feature IDs and input/output schema, combined control+data cycle rejection, data-ready admission, missing/failed/unknown producer effects, fan-out retention and release, bounded byte budgets and cancellation, stale run/revision snapshots, no cross-owner borrowing, immutable editor layout/source authority and zero change to S2 runner meaning. Measure S2 baseline and app-sidecar alternatives before selecting any common value-transfer mechanism.
5. **Headless vs GUI product boundary.** Render2D likely acts as an optional output/presentation adapter, not an obligatory pure data transform or a new capability. RunenRender owns 2D composition/execution, not native window/presentation. An independent editor/export must respect #60/#75 authorization and #196 source ownership; do not use unauthenticated loopback to publish mutation controls.
6. **End-user acceptance:** the first visually editable 5-node canvas should have type-safe ports, explicit config, immutable stage/result revisions, selective recomputation, branch previews, and inspectable failures. An app-specific viewer of a fixed Rust graph does not complete the *editable* experience. Conversely a generic S2 control-plan GUI without S3 value edges does not satisfy this node-based product outcome.

## 5. Boundary between #175 and #187

**#175 owns** first-domain use case, genuine Grid data/algorithm contracts, app-owned snapshot/recompute policy, optional renderer adaptation and user-facing UX evidence. **#187 owns** reusable executable typed data port/edge semantics, value lifetime, runner evolution, S2 preservation and all-application authoring compatibility. #111 accepted authoring authority semantics remain normative. #191/#192/#193/#195/#196 implement S2 safe authoring/publication and GUI independently and should not be blocked or silently redefined by this research.

The next owner decision must compare genuine end-to-end developer authoring of A/B/C/D, real Grid/heat-map fan-out, measured memory/copies and S3 migration. No decision should depend only on a 64x64 room generator or speculative universal GPU graph.


## 6. Source and evidence correction after audit

- #111's C+S2+V2+A2+B2+C3 architecture is **owner accepted and integrated**; do not treat it as still open. Initial headless A2 editor-safe binding has entered the SDK, while document import, persistent V2 fencing, C3 activation and GUI #196 remain separate implementation workstreams.
- The user reported running Grid's local `grid_generation::workbench_175` and `tile_topology` tests successfully. Those results are **user-reported local pass**, not a GitHub Linux CI pass because the private repository's job ended before launching a runner. Exact local toolchain/commit evidence beyond the supplied command/output is not independently recorded.
- The first Workbench visual node graph is a **product UX preference**. The choice of application-owned bounded node authoring versus future generic S3 executable ports has not yet been accepted. Do not conflate a read-only S2 graph inspector, a control-only S2 editor and an executable typed-dataflow editor.
- The `Render2D` entry should be modeled as an **optional effectful output/presentation boundary** unless an actual reusable render-result contract is justified. Rendering cannot silently become canonical editable data or hide the distinction between grid data revisions and displayed frames.


## 7. Candidate S3 port-preflight experiment (second isolated proof)

`src/typed_ports.rs` is a **research-only, app-side metadata adapter**, which:
- Declares exact node/port IDs, direction via input/output lists, versioned semantic type names and bounded output/consumer capacities.
- Rejects mismatched semantic types, mismatched versions, unknown/missing nodes or ports, missing required inputs, competing input producers, duplicate identifiers and over-budget declarations *before* constructing a Riesel plan.
- Performs cycle detection over the **union** of S2 control prerequisites and proposed typed edges (not independently per edge kind).
- Allows one producer output to fan out to two consumers without counting a source's declared static output budget twice when referencing the same output.
- Projects **only ordering** into the existing production `ExecutionPlanDraft::validate()`. Riesel remains the authoritative S2 plan validator; these metadata tests do **not** certify actual Rust handler signatures, per-run artifact publication, real memory residency, native affinity or downstream readiness.

**Critical negative finding:** A naive translation of an S3 value graph to an S2 control-only plan discards required value-transfer meaning. It must **never** be publishable/executable through an S2-only authoring or runtime path. The second candidate therefore wraps the translated `ValidatedExecutionPlan` as a *read-only projection* whose `for_s2_execution()` rejects when any typed value edge is required; a legacy graph with zero data edges still permits S2 execution. This wrapper is only a test-local design probe and is **not** a security barrier against someone separately reconstructing a plan; actual B2 required-feature negotiation, trusted descriptor-to-handler binding, immutable V2 publication and runtime admission must enforce fail-closed semantics in production.

**Validation boundaries:** Static `max_bytes` metadata is not a runtime byte budget or proof of zero copies; `TypeId { name, version }` is not authoritative schema provenance or proof of `Send`/`Sync`/`!Send` safety; translating port requirements to `.after` control edges is not value transfer. This is a **falsification of unsafe S2-as-S3 fallback**, not acceptance of this implementation.

Reproducible fixture checks: `cargo test --manifest-path research/typed-handoff-187/Cargo.toml`. [Research CI run #44997446383](https://github.com/aschenrot/riesel/actions/runs/44997446383) **PASS 11/11** at source `4fd8c56c3adbdefdb55ce14524582e209611d8a4` before the fail-closed wrapper extension. The final wrapper revision needs its own exact-head verification.

## 8. Grid/renderer source audit and non-dungeon growth

On the private Grid `main` source revision `dd05a2c857c9d0410cb375dbebc108bdaa11dc5c`, `grid_generation::fill_with_noise` uses `FastNoiseLite` OpenSimplex2 + FBm, 4 octaves, domain warp, a `f32` frequency/threshold and a seeded noise sampling/transformation, then directly writes `DenseGrid2D<CellType>`. It **does not** currently return `DenseGrid2D<f32>` or `DenseGrid2D<f64>`. The research Riesel DTO `Vec<f64>` and synthesized scalar values are not semantically equivalent to this real noise implementation; algorithm version, sampling-space, interpolation/normalization, precision and seed conversions require explicit contracts before asserting replay equivalence. The current production CA helper is fixed rule and treats outside cells as occupied; the configurable protected-border implementation exists only in the private research test, not yet as an accepted API.

`tile_topology` owns the 16-mask classification and rotation tests; the Grid Workbench research test additionally exercises each actual formed central mask but does not separately assert all kind/rotation expected mappings. This is appropriate source-of-truth reuse, but do not overclaim what the research test independently checked.

**Documentation drift requiring review:** Grid's `docs/generation-boundary.md` says its Godot World Lab generator and chunk lifecycle do not belong in `grid_generation`, while that crate's current source also contains `LabGenerationSettings`, `LabGeneratedChunk`, `build_owned_halo_formation_layers` and other lab/chunk functions. This does not prove those APIs are actually imported from a particular app, nor does it authorize moving them, but the contradiction must be resolved before presenting `grid_generation` as a settled neutral abstraction or designing a general `Noise2D` provider around the entire crate. The relevant public reusable **small neutral functions** and app-specific/lab functions need a truthful boundary audit.

RunenRender `composition_2d` can express immutable source-neutral painting and its renderer executes through a caller-owned GPU host; neither it nor Riesel owns the native Present loop by default. For the initial node graph, `Render2D` should be an explicit *effectful optional sink* with a separate presentation/host adapter, not a required pure stage that converts tile descriptors to canonical grid data.

**Second consumer:** the same scalar output can feed both `Threshold` occupancy and a **scalar heat/mean-statistic pipeline**. The existing real-runner test demonstrates shared source fan-out with a synthetic scalar field but is not real thermal simulation, GPU workload, chunked world or interactive editor validation.

## 9. Next research decisions, without conflating acceptance

1. Real local/private Grid algorithm API proof: sample scalar values with declared coordinate/seed/precision guarantees and explicit threshold, plus configurable synchronous border-preserving CA; ensure upstream tests and acceptable CI evidence.
2. #187: evaluate actual optional typed handler adapter against accepted S2 and B2 features *without allowing an S3 plan to be executed as stripped S2*. Define per-run artifact ownership, consumer readiness and missing/failed/unknown upstream states, byte budgeting, stale-write fencing and affinity; measure dynamic memory/copies, not just declared fields.
3. #175: owner decides final product UX, data ownership/capability granularity and implementation increment sequence based on the multi-domain comparison. #187 separately owns platform-level S3 acceptance. Both require recorded owner accept/revise/defer decisions before freezing public contracts or creating S3 production issues.

No architecture decisions above are silently approved by passing research CI.

## 10. Actual concurrent-run stale-output counterexample (third proof)

An additional test now runs **two actual `context.execution_graph` instances** for the *same logical plan ID* with semantic revisions 1 and 2 in the same application session. The test holds the older callback at a deterministic async gate, allows the newer revision's callback to publish a domain-owned result, and then releases the older callback. The domain-local snapshot owner checks its expected revision and **rejects the stale write**; the older Riesel graph report is failed while the newer report is successful, and its newer value remains intact.

The experiment proves an app author *can* protect a local result cache when multiple graph runs overlap, but **the existing S2 runner does not itself provide the artifact slot, check the producer's output revision, or decide which completed result becomes current**. No true S3 per-run port values, persistent source/plan CAS, authenticated publisher or automatically safe recomputation follows. Different operations lacking declared shared resources may run concurrently by design; this test is not evidence of an SDK scheduling defect.

**Focused verification:** [CI #44997722305](https://github.com/aschenrot/riesel/actions/runs/44997722305) **PASS**, **13/13 tests and headless application run** at the exact source SHA `f42c8fc392edd689868730d8ab15d790b3da6759`. This expands the earlier 11/11 typed-port test run and 12/12 S2 fail-closed test run. Final report/head normal Linux Validate/smoke evidence must be checked separately before any integration.
