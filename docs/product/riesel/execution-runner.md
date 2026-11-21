# Session-owned execution graph runner (incremental delivery)

**Status:** First bounded headless async runner increment merged; shared session-level admission implemented in a follow-up increment for implementation [#112](https://github.com/aschenrot/riesel/issues/112); not complete acceptance of #112 or authoritative live control [#113](https://github.com/aschenrot/riesel/issues/113).

The runner consumes an immutable `BoundExecutionPlan<AsyncOperation>` from the existing [plan authoring API](execution-plan-authoring.md) through an existing application session. The graph constructor is internal; callers use `RunningApp::execution_graph` or `Context::execution_graph`, and cannot substitute a self-created session token. It does not launch providers, create a Tokio runtime, reinterpret `AppSchema` or own an inner domain scheduler. `RunningApp::execution_graph(plan, limit)` and `Context::execution_graph(plan, limit)` derive the session from the actual application instead of inventing another owner.

One-shot callback registration uses `async_operation(|| async { OperationOutcome::Completed })` and `OperationRegistry::<AsyncOperation>`. The same binding registry can hold callbacks capturing previously resolved typed capability handles. A domain-owned simulation/world remains in its own thread or executor; the callback may send a tick/frame admission message through its domain adapter but must not move a non-`Send` world into Tokio.

## Admission and outcome behavior

- Bounded parallel operation execution (maximum 32 active per run and **32 active across all graph runs of one application session**, with at most **16 outstanding runner instances**) under the awaited caller's Tokio session. A run is consumed by `execute()`; its internal `JoinSet` aborts outstanding tasks if the owning future is dropped.
- Validated dependencies and `Signal` triggers gate admission. `signal_sender().fire(name)` uses a bounded channel (64 pending messages), refuses undeclared signals and rejects an expired session. A paused signal-gated tick does not prevent unrelated eligible async nodes from running.
- Declared read/write conflicts serialize conflicting operations **across all graph runs in the same application session**, with release notifications waking capacity-blocked runs. Admission is nonblocking and releases reservations on completion, panic or cancellation. These declarations **do not** confer Rust borrow safety; the resource namespace is application-session-wide. Panics become failed operations, and unknown/cancelled/failed prerequisites block success-only dependents.
- No automatic retries; unknown outcomes stay unknown. On session invalidation, in-flight operations are reported conservatively as outcome unknown even if cancellation dropped the future. Confirmed committed effects and safe-point stepping require the #113 contract.

The `GraphRunReport` is a local post-run result with plan identity/revision and terminal node statuses. It is **not** an authoritative live observation store, stable run/session/attempt address, persisted execution log, replay contract or cross-process authorization mechanism.

## Verification and deliberate exclusions

Run `cargo test -p riesel --features async-tcp --test execution_runner`, `cargo validate --check` and `cargo smoke` on the exact revision. The SDK tests cover overlapping branches and joins, independent progress during signal gating, read/write conflicts, failed/unknown effects without implicit retry, blocked dependents, bounded parallel admission and session cancellation. A separate crate-level integration fixture exercises a real Riesel application session, a resolved capability and a thread-confined `!Send` simulation actor with explicit two-tick admission.

Session-wide admission provides a conservative shared resource budget; it does not guarantee fairness between competing runs or claim that unrelated external applications share the same resource namespace. Runner creation fails closed with `SessionRunLimitExceeded` when the 16-instance cap is reached. Application work outside the graph (including separately spawned tasks) is not charged against these graph-only slots.

This increment deliberately does not claim fixed-rate timing guarantees, reusable tick loops, nested ECS-system stepping, callback portability to other processes, typed dataflow, managed-service recovery, process-global authority or a graphical inspector. Full #112 acceptance still requires realistic domain delegation and application-session end-to-end fixtures before closure.

## Runnable headless workflow and session shutdown

Run `cargo run --locked -p riesel --features async-tcp --example execution_graph`. The example executes two deliberately overlapping async branches, an explicit `start` signal and a completion join, using a single real `Application::run` session. It awaits the entire graph on the caller-owned Tokio executor **before** `Application::run` tears down the session. It neither performs a second provider-resolution pass nor creates a second Riesel application.

The runtime's structured `AsyncTasks` interface is separate from arbitrary `tokio::spawn`. A graph future detached into a caller-created background task is **not** automatically registered with `AsyncTasks` or guaranteed joined before managed provider teardown. Do not detach graph runners in a session that may be stopped or recovered; await them within the application entrypoint. Session expiry stops admission and conservatively classifies an already-dispatched but unfinished operation as `OutcomeUnknown`, while preserving previously confirmed completion. This is not automatic rollback, retry or safe interruption of noncooperative threads.

## Fixed-tick ownership acceptance fixture

The feature-gated [two-policy domain fixture](../../../crates/riesel/tests/execution_domains.rs) compares the same two committed simulation ticks (four proposed, catch-up budget two) under **Riesel-admitted outer ticks** and a **domain-owned nested frame**. Both run alongside an overlapping two-branch async workflow and final join. The native `Rc<Cell<...>>` world never leaves its own actor thread. Per-tick nodes appear only in the central-admission projection; the delegated graph exposes one frame, not fictitious internal ECS-system steps.

The fixture also rejects stale domain permits and explicit owner mismatches before mutating state. A domain-owned frame cancelled after its first committed tick yields `OperationOutcome::Cancelled`, blocks its dependent final node and preserves the first tick in the domain's actual world. **This is a tested adapter/fixture invariant, not a published operation-authority metadata contract or a general-purpose fixed-rate clock policy.** Riesel currently cannot inspect arbitrary user code and guarantee every operation has declared the correct domain owner. Authoritative committed-prefix accounting, run/attempt identity and cooperative external controls still belong to [#113](https://github.com/aschenrot/riesel/issues/113).
