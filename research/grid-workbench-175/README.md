# Headless Grid Workbench — real Grid/Riesel integration

This is a **separate Cargo application** using the actual Riesel session-owned execution graph and the pinned upstream [Grid #2 implementation candidate](https://github.com/aschenrot/grid/pull/11). It contains **no GUI, renderer, window host, Manager, or transport**. It is a reference implementation for [Riesel #175](https://github.com/aschenrot/riesel/issues/175), stacked on experimental typed local graph [PR #246](https://github.com/aschenrot/riesel/pull/246). It does not establish a stable S3 API, document ABI or external authoring permission.

## Run

Use Rust 1.100.0. From the Riesel repository root:

```sh
cargo test --manifest-path research/grid-workbench-175/Cargo.toml
cargo run --manifest-path research/grid-workbench-175/Cargo.toml -- --seed 42 --width 64 --height 64 --threshold 0.55 --iterations 5 --border 1 --revision 1
cargo run --manifest-path research/grid-workbench-175/Cargo.toml -- --seed 42 --width 64 --height 64 --threshold 0.70 --iterations 0 --revision 2
```

The app accepts `--width`, `--height`, `--seed`, `--frequency`, `--threshold`, `--iterations`, `--border`, and `--revision`. All numeric arguments are checked before creating the application. Dimensions are limited to 1–512 in each axis and iteration count to 0–16. The first supported scope is an in-process, one-run-at-a-time reference; it does not claim a generic worker cluster, high-precision world-region sampler or GPU execution.

## Graph and ownership

```text
Noise2D: DenseGrid2D<f32> ──┬──> Threshold: DenseGrid2D<bool> ──> CA: history ──> DualGrid: 65×65
                            └──> ScalarStats: min/max/mean
```

The sample is a real legacy-compatible warped OpenSimplex2 scalar field, with an explicit strict threshold. A CA node owns synchronous iterations, retains each immutable state including iteration zero, freezes a selected border and arbitrary interior stripe, and reads outside neighbors according to its own independent rule. The dual-grid node calls `tile_topology::form_visual_tiles`; no mask classification or rotation logic is copied.

Riesel owns the **outer execution graph**, real callback registration, dependency admission, run/session identity and failure propagation. Values use opt-in process-local typed immutable references; a failed producer blocks consumers. A node is an operation, **not automatically a capability or OS process**. Large data never has to be turned into JSON merely to traverse a local graph. Source-owned artifacts and original snapshots remain available to the finished run result. The separate scalar-statistics consumer demonstrates reuse outside room generation.

## Proven versus not yet delivered

The dedicated [GitHub Actions workflow](../../.github/workflows/headless-grid-workbench-175.yml) compiles the actual pinned Grid crates and Riesel, tests deterministic replay and the CA border/mask on every step, runs two distinct semantic revisions and rejects invalid configuration, then verifies existing S2 graph tests. Exact CI evidence and any remaining blockers belong to [PR #247](https://github.com/aschenrot/riesel/pull/247).

The CLI permits configuration changes between invocations, but **not yet graph-edit documents or a graphical node editor**. This research does not implement durable plan publication, authorized activation, undo, hot node reconfiguration, branch re-use cache, selective recompute, cross-process typed artifacts, cancellation-safe persistent storage, or `!Send`/ECS/GPU values. The accepted V2 semantic and layout revision rules remain normative; this test-only in-memory graph does not replace them. The first GUI adapter is explicitly outside this scope. Grid #2 and Riesel #187 require their separate API and failure-ownership acceptance gates before an ordinary production release.
