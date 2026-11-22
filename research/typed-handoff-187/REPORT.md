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
