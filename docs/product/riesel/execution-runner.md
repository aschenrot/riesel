# Session-owned execution graph runner (incremental delivery)

**Status:** First bounded headless async runner increment proposed in implementation [#112](https://github.com/aschenrot/riesel/issues/112); not complete acceptance of #112 or authoritative live control [#113](https://github.com/aschenrot/riesel/issues/113).

The runner consumes an immutable `BoundExecutionPlan<AsyncOperation>` from the existing [plan authoring API](execution-plan-authoring.md) and the *already active* `SessionToken`. It does not launch providers, create a Tokio runtime, reinterpret `AppSchema` or own an inner domain scheduler. `RunningApp::execution_graph(plan, limit)` and `Context::execution_graph(plan, limit)` derive the session from the actual application instead of inventing another owner.

One-shot callback registration uses `async_operation(|| async { OperationOutcome::Completed })` and `OperationRegistry::<AsyncOperation>`. The same binding registry can hold callbacks capturing previously resolved typed capability handles. A domain-owned simulation/world remains in its own thread or executor; the callback may send a tick/frame admission message through its domain adapter but must not move a non-`Send` world into Tokio.

## Admission and outcome behavior

- Bounded parallel operation execution (maximum 32 active per run) under the awaited caller's Tokio session. A run is consumed by `execute()`; its internal `JoinSet` aborts outstanding tasks if the owning future is dropped.
- Validated dependencies and `Signal` triggers gate admission. `signal_sender().fire(name)` uses a bounded channel (64 pending messages), refuses undeclared signals and rejects an expired session. A paused signal-gated tick does not prevent unrelated eligible async nodes from running.
- Declared read/write conflicts serialize conflicting operations, but declarations **do not** confer Rust borrow safety. Panics become failed operations, and unknown/cancelled/failed prerequisites block success-only dependents.
- No automatic retries; unknown outcomes stay unknown. On session invalidation, in-flight operations are reported conservatively as outcome unknown even if cancellation dropped the future. Confirmed committed effects and safe-point stepping require the #113 contract.

The `GraphRunReport` is a local post-run result with plan identity/revision and terminal node statuses. It is **not** an authoritative live observation store, stable run/session/attempt address, persisted execution log, replay contract or cross-process authorization mechanism.

## Verification and deliberate exclusions

Run `cargo test -p riesel --features async-tcp --test execution_runner`, `cargo validate --check` and `cargo smoke` on the exact revision. The SDK tests cover overlapping branches and joins, independent progress during signal gating, read/write conflicts, failed/unknown effects without implicit retry, blocked dependents, bounded parallel admission and session cancellation. A separate crate-level integration fixture exercises a real Riesel application session, a resolved capability and a thread-confined `!Send` simulation actor with explicit two-tick admission.

This increment deliberately does not claim fixed-rate timing guarantees, reusable tick loops, nested ECS-system stepping, callback portability to other processes, typed dataflow, managed-service recovery, process-global authority or a graphical inspector. Full #112 acceptance still requires realistic domain delegation and application-session end-to-end fixtures before closure.
