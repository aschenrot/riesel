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

## Explicit domain admission ownership (opt-in)

The validated plan can declare a domain's **outer** execution boundary using
`OperationNode::domain_tick("domain.id")` for Riesel-admitted tick operations,
or `OperationNode::delegated_frame("domain.id")` for a Riesel-admitted frame
whose *internal* ticks and systems are owned by the domain's own scheduler.
These are inspectable plan semantics, not arbitrary-closure serialization,
automatic fixed-rate clocks or permissions to control an ECS.

- The plan validator rejects invalid domain IDs and a single plan that mixes
  these two owners for the same domain, **even when the nodes are ordered**.
- The existing session admission table claims each plan's declared domain
  modes atomically when a runner is created. Another outstanding run cannot
  claim the opposing mode for the same named domain. The typed
  `GraphRunError::ConflictingDomainAuthority` fails **before any callback**.
  The mode claim remains pinned by any still-active graph-owned Tokio
  callback lease even if its outer runner future is abruptly dropped;
  only after these leases are released may Riesel admit a different
  declared owner. Actual external/domain-native work may continue after
  its async adapter is dropped: the application/domain must independently
  establish a safe handoff before using that newly available mode.
- Each operation declaring the same domain is admitted exclusively, **even
  without a manually declared read/write resource key**, across runs in the
  same Riesel application session. Other domains and independent async
  operations retain their own admission eligibility.
- The application provides its own tick/frame source and domain adapter.
  An `on_signal` trigger supports explicitly released outer ticks; there is
  **no runtime wall-clock source, catch-up policy, jitter guarantee or
  automatic loop**. A `DelegatedFrame` is one outer graph node, not a
  fabricated list of internal ECS systems.
- This opt-in declaration can coordinate *Riesel-admitted operations*;
  it cannot intercept opaque Rust callbacks that omit/misstate the domain,
  independently spawned threads, or an external domain's undisclosed work.
  Domain adapters must enforce their internal ownership and lifecycle.
  Session-local mode matching is **not authorization** for privileged
  inspection or control, nor #113's run/attempt safe-point identity.

The real thread-affine actor fixture below now uses these declarations
for both policies and continues to verify equal final world semantics,
cooperative cancellation after a committed tick, and independent async
work. A future accepted clock/pacing policy and #113 safe-point controls
can build on this same explicit boundary without creating a second
provider resolver, ECS scheduler or process owner.

## Runnable headless workflow and session shutdown

Run `cargo run --locked -p riesel --features async-tcp --example execution_graph`. The example executes two deliberately overlapping async branches, an explicit `start` signal and a completion join, using a single real `Application::run` session. It awaits the entire graph on the caller-owned Tokio executor **before** `Application::run` tears down the session. It neither performs a second provider-resolution pass nor creates a second Riesel application.

The runtime's structured `AsyncTasks` interface is separate from arbitrary `tokio::spawn`. A graph detached into a caller-created task is **not** automatically registered or guaranteed joined before provider teardown. Use **either** `runner.execute().await` within the session entrypoint **or** `context.async_tasks().spawn_graph("name", runner)` inside an active async scope. The latter uses the existing bounded named application-task owner, not a second scheduler. A runner from another application session is rejected before task admission via an **in-process, clone-based session identity check**; this is not remote authentication or proof of executable provenance. Normal scope shutdown requests a cooperative stop, aborts and **joins** all graph-owned Tokio operation futures before their provider instances are released. A completed unsuccessful graph propagates `GraphExecutionFailure` (including its local `GraphRunReport`) as a named `RuntimeError::TaskFailed` and revokes the application session; planned shutdown itself is not a task failure. It neither automatically retries nor rolls back previously sent effects.

A graph interrupted during a structured stop cannot claim that dispatched effects did not occur. Async callback futures are dropped and joined, but operations that start independent native threads or external mutations must arrange their own teardown, fencing and reconciliation. Noncooperative synchronous code on a Tokio worker can still block shutdown. Authoritative committed effects and live reports remain part of [#113](https://github.com/aschenrot/riesel/issues/113).

## Real managed-process crash and uncertainty fixture

The opt-in [managed graph integration fixture](../../../crates/riesel_app/tests/managed_graph_lifecycle.rs) runs with `cargo test --locked -p riesel_app --features async-tcp,test-fixtures --test managed_graph_lifecycle`. Its crash-service executable lives under `tests/fixtures/` and is **not built by ordinary application/SDK builds**. The fixture uses **production managed-service spawn, ephemeral TCP readiness, a real OS process exit, session monitoring, and the actual graph runner**. Its small private command protocol (`COMMIT`, `CRASH`) is fixture-only, not Riesel RPC or an example of authenticated provider identity. Both the directly awaited graph and the registered `AsyncTasks::spawn_graph` path are exercised.

The test first obtains a provider-confirmed acknowledgement, then dispatches another command whose child intentionally exits without replying. The already-confirmed operation remains completed, the in-flight graph operation is conservatively outcome unknown, success-only dependents do not run, stale signals are rejected, and the actual managed-service exit remains the primary failure. The structured variant verifies graph callback futures are dropped/drained before the owner scope finishes without creating a secondary graph-task error. A fresh managed application session is distinct and does not *implicitly* reexecute an old runner. **This does not prove cross-process exactly-once effects**, generic transport authorization, or that an opted-in application restart will refrain from repeating its *own* entrypoint logic.

## Fixed-tick ownership acceptance fixture

The feature-gated [two-policy domain fixture](../../../crates/riesel/tests/execution_domains.rs) compares the same two committed simulation ticks (four proposed, catch-up budget two) under **Riesel-admitted outer ticks** and a **domain-owned nested frame**. Both run alongside an overlapping two-branch async workflow and final join. The native `Rc<Cell<...>>` world never leaves its own actor thread. Per-tick nodes appear only in the central-admission projection; the delegated graph exposes one frame, not fictitious internal ECS-system steps.

The fixture also rejects stale domain permits and explicit owner mismatches before mutating state. A domain-owned frame cancelled after its first committed tick yields `OperationOutcome::Cancelled`, blocks its dependent final node and preserves the first tick in the domain's actual world. **This is a tested adapter/fixture invariant, not a published operation-authority metadata contract or a general-purpose fixed-rate clock policy.** Riesel currently cannot inspect arbitrary user code and guarantee every operation has declared the correct domain owner. Authoritative committed-prefix accounting, run/attempt identity and cooperative external controls still belong to [#113](https://github.com/aschenrot/riesel/issues/113).
