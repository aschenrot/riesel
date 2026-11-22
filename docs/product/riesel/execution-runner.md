# Session-owned execution graph runner (incremental delivery)

**Status:** Validated plans, session-owned async graph execution, explicit domain boundaries and bounded fixed-step proposals are integrated under [#112](https://github.com/aschenrot/riesel/issues/112). Read-only authoritative run observation is integrated under [#113](https://github.com/aschenrot/riesel/issues/113). A bounded attempt-scoped owner-confirmed-prefix API is implemented as a #113 increment; general effect reconciliation, cooperative control and externally authorized inspection remain outstanding.

The runner consumes an immutable `BoundExecutionPlan<AsyncOperation>` from the existing [plan authoring API](execution-plan-authoring.md) through an existing application session. The graph constructor is internal; callers use `RunningApp::execution_graph` or `Context::execution_graph`, and cannot substitute a self-created session token. It does not launch providers, create a Tokio runtime, reinterpret `AppSchema` or own an inner domain scheduler. `RunningApp::execution_graph(plan, limit)` and `Context::execution_graph(plan, limit)` derive the session from the actual application instead of inventing another owner.

One-shot callback registration uses `async_operation(|| async { OperationOutcome::Completed })` and `OperationRegistry::<AsyncOperation>`. The same binding registry can hold callbacks capturing previously resolved typed capability handles. A domain-owned simulation/world remains in its own thread or executor; the callback may send a tick/frame admission message through its domain adapter but must not move a non-`Send` world into Tokio.

## Admission and outcome behavior

- Bounded parallel operation execution (maximum 32 active per run and **32 active across all graph runs of one application session**, with at most **16 outstanding runner instances**) under the awaited caller's Tokio session. A run is consumed by `execute()`; its internal `JoinSet` aborts outstanding tasks if the owning future is dropped.
- Validated dependencies and `Signal` triggers gate admission. `signal_sender().fire(name)` uses a bounded channel (64 pending messages), refuses undeclared signals and rejects an expired session. A paused signal-gated tick does not prevent unrelated eligible async nodes from running.
- Declared read/write conflicts serialize conflicting operations **across all graph runs in the same application session**, with release notifications waking capacity-blocked runs. Admission is nonblocking and releases reservations on completion, panic or cancellation. These declarations **do not** confer Rust borrow safety; the resource namespace is application-session-wide. Panics become failed operations, and unknown/cancelled/failed prerequisites block success-only dependents.
- No automatic retries; unknown outcomes stay unknown. On session invalidation, in-flight operations are reported conservatively as outcome unknown even if cancellation dropped the future. Attempt-scoped owner-confirmed effect prefixes are available under #113; broader reconciliation and safe-point stepping remain outstanding.

The `GraphRunReport` is a local post-run result with an identity that also correlates to its **live, in-process** `GraphRunObserver` snapshot. Neither is a persisted execution log, durable replay contract or cross-process authorization mechanism. A confirmed effect prefix is a separate, explicit owner assertion; an operation terminal status never proves a remote mutation.

## Verification and deliberate exclusions

Run `cargo test -p riesel --features async-tcp --test execution_runner`, `cargo validate --check` and `cargo smoke` on the exact revision. The SDK tests cover overlapping branches and joins, independent progress during signal gating, read/write conflicts, failed/unknown effects without implicit retry, blocked dependents, bounded parallel admission and session cancellation. A separate crate-level integration fixture exercises a real Riesel application session, a resolved capability and a thread-confined `!Send` simulation actor with explicit two-tick admission.

Session-wide admission provides a conservative shared resource budget; it does not guarantee fairness between competing runs or claim that unrelated external applications share the same resource namespace. Runner creation fails closed with `SessionRunLimitExceeded` when the 16-instance cap is reached. Application work outside the graph (including separately spawned tasks) is not charged against these graph-only slots.

This increment deliberately does not claim fixed-rate timing guarantees, reusable tick loops, nested ECS-system stepping, callback portability to other processes, typed dataflow, managed-service recovery, process-global authority or a graphical inspector. Full #112 acceptance still requires realistic domain delegation and application-session end-to-end fixtures before closure.

## Authoritative live run observation (read-only; #113 increment)

Call `let observer = runner.observer();` **before** consuming the runner
with `runner.execute().await`, or before handing it to the existing
structured `AsyncTasks::spawn_graph`. Cloned observer handles retain the
one run's in-memory snapshot and bounded transition history even after
the runner future returns or is abandoned. This observer cannot execute,
cancel, pause, resume, step or authorize access to other processes.

- Each successfully admitted graph obtains a runtime-issued, random UUID
  **session generation** and a monotonically allocated per-session **run
  sequence**, associated with the exact immutable plan ID and revision.
  Within one application generation, run sequences never alias, including
  across separate repeat-tick executions. A new session obtains a fresh
  generation. These are correlation values **not credentials**, stable
  persistence IDs, signed provenance, or a public remote protocol.
- `observer.snapshot()` is copied atomically from the coordinator's
  authoritative in-process state: `Created`, `Running`, `Finished`
  or `Abandoned` phase; per-node `Pending`, `Running`, `Completed`,
  `Failed`, `Cancelled`, `OutcomeUnknown` or `Blocked` state; and
  admission **attempt** (zero until admitted, one for the current one-shot
  runner). Snapshot state is not reconstructed from lost trace events.
- `observer.changes_since(sequence)` reads a capped ring of at most
  **128** run-phase/operation transitions. Every transition carries the
  run identity and a monotonic per-run sequence; operation events also
  carry node ID and attempt. A future cursor returns
  `GraphObservationError::FutureCursor`. If the caller falls behind
  retention, `truncated: true` explicitly requires a fresh authoritative
  snapshot and a new cursor. Overflow never blocks or backpressures the
  scheduler, and a trace **is not a committed-effect ledger**.
- The runner records each admission, completed/failed/cancelled/unknown
  outcome, blocked dependency and terminal phase. A dropped graph future
  without a report is `Abandoned`; operations recorded as running are
  conservatively `OutcomeUnknown`, success-only dependents become
  `Blocked`, unrelated never-admitted operations become `Cancelled`.
  A clean completed run is `Finished` even if its graph operation report
  contains failure/unknown. No automatic retry or effect rollback follows.
- Observation remains **local and read-only**. It does not verify the
  provenance of remote applications, protect against same-UID hostile
  callers, preserve history after all handles disappear, infer exact
  effects from missing replies, or provide nested ECS-system safe points.
  Confirmed effect recording and cooperative, owner-fenced pause/step/
  resume are remaining #113 work; any external inspector must satisfy
  #75's independently accepted authorization/export rules first.

The `execution_graph` example now checks a post-run snapshot against
its authoritative report and reads a correlated transition batch.
The managed-child crash test verifies this same observation path under
real session revocation, preserving the confirmed completed prefix and
classifying the dispatched crash operation as unknown.

## Attempt-scoped owner-confirmed effect prefixes (#113 increment)

A registered operation may use `async_operation_with_effects(|effects| async move { ... })` instead of `async_operation(|| async { ... })`. Only **after** the authoritative owner confirms a committed result may it call `effects.confirm_prefix("simulation.ticks", cumulative)`. The first confirmed tick is `1`, then `2` and so on **within one operation attempt**. This is not a global tick number, general unordered mutation ledger, or a synthetic RPC acknowledgement. Callbacks that never report effects remain supported.

- The reporter is passed on **actual dispatch**, never during binding or validation. It belongs to one operation attempt and becomes inactive after the operation terminates or the run is finished/abandoned. A retained reporter cannot rewrite terminal history.
- `GraphRunSnapshot.operations[id].confirmed_prefixes` retains the authoritative high-water mark for each stream, even if a later outcome is Failed, Cancelled or OutcomeUnknown. `GraphRunReport.confirmed_prefixes` retains the same completed-run data. An unknown later dispatch does **not** erase a confirmed prefix or establish any additional commit.
- Stream names are 1–128 ASCII letters, digits, dots, underscores or hyphens. Cumulative progress must be positive, monotonic and at most 32 distinct streams per attempt. An equal report is idempotent; regressing, invalid, over-budget and stale reports fail.
- `GraphRunChange::ConfirmedPrefix` emits correlated notifications through the existing lossy 128-event ring. **Do not reconstruct effects from events.** Readers must resynchronize from the authoritative snapshot on loss.
- Riesel accepts an **explicit assertion from trusted application callback code**, not independent proof of remote mutation. An external domain/service adapter must await an actual owner acknowledgement. Timeout, crash, unknown reply and unowned/detached work do not establish confirmation. No rollback, business-command deduplication, durable log, automatic retry, cross-process authorization or exactly-once guarantee follows.

The [fixed-step headless example](../../../crates/riesel/examples/execution_fixed_step.rs) records a local confirmed tick prefix per one-shot run. Regression tests cover partial confirmation followed by failure, abrupt abandonment, invalid/bounded streams, regression, idempotency and stale reporters. General reconciliation and cooperative pause/step/resume remain outstanding #113 requirements.

## Explicit domain admission ownership (opt-in)

The validated plan can declare a domain's **outer** execution boundary using
`OperationNode::domain_tick("domain.id")` for Riesel-admitted tick operations,
or `OperationNode::delegated_frame("domain.id")` for a Riesel-admitted frame
whose *internal* ticks and systems are owned by the domain's own scheduler.
These are inspectable plan semantics, not arbitrary-closure serialization,
automatic fixed-rate clocks or permissions to control an ECS.

- The plan validator rejects invalid domain IDs, repeated boundary assignments
  on one operation (no silent last-wins authority), and a single plan that
  mixes these two owners for the same domain, **even when the nodes are ordered**.
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

## Bounded fixed-step proposal policy (opt-in)

`FixedStepPolicy::new(interval, max_ticks_per_frame, backlog_policy)` and
`FixedStepAdmission::advance_frame(elapsed)` provide deterministic, bounded
logical tick proposals to an **explicitly chosen clock owner**. These types
are also usable without the async TCP feature, so a native/domain thread can
calculate its own frame budget without acquiring a Tokio executor.

- `interval` must be strictly positive; `max_ticks_per_frame` must be
  1–1024; an optional retained backlog is limited to 65,536 ticks.
  `FixedStepBacklog::DiscardExcess` or
  `FixedStepBacklog::RetainUpTo(limit)` **must be selected explicitly**.
- `FixedStepFrame` reports the number of currently eligible proposals,
  deferred ticks, explicitly discarded ticks (`u128` to handle very long
  elapsed times), and fractional elapsed time. Large stalls are counted in
  bounded arithmetic, not a loop for each lost interval.
- `claim(n)` consumes at most the remaining **per-frame** budget.
  Unclaimed proposals remain subject to the next frame's selected backlog
  policy; claimed proposals become the caller's dispatch responsibility,
  **not proof that a graph operation or remote effect completed**. Failed
  dispatches and unknown effects must be reconciled by the owning domain
  instead of blindly replaying claims.
- An application's monotonic event/frame clock supplies **elapsed time**
  exactly once to its chosen tick owner. There is no implicit wall clock,
  timer task, frame deadline, jitter guarantee or background scheduler.
  A new application session constructs a **fresh** admission instance,
  never reuses an old session's deferred ticks, and still relies on existing
  graph/session fencing for execution.
- Centrally owned ticks may each consume one proposal and execute a
  `domain_tick` plan. Domain-owned frames instead invoke one outer
  `delegated_frame` plan operation; its native `!Send` coordinator owns
  its own independent fixed-step accumulator and claims internal ticks.
  A graph remains **one-shot**. An immutable `BoundExecutionPlan` can be
  cloned to start *separate* runs in the same session, with no implicit
  retry of cancelled/uncertain effects. A reusable automatic recurring
  runner, true wall-clock pacing and #113 run identity/stepping remain
  separate, not silently implemented by `advance_frame`.

Run
`cargo run --locked -p riesel --features async-tcp --example execution_fixed_step`.
It drives three independent one-shot tick executions across three synthetic
frame samples (35ms, 0ms, 0ms) using a 10ms tick interval, 2-tick frame
cap and one retained tick. Its explicit clock input is reproducible; it
does not claim a real-time frame loop. The existing `execution_domains`
integration verifies that central and delegated **actual** thread-confined
actor execution retain equivalent committed-tick semantics with the same
bounded policy and independent async work.

## Runnable headless workflow and session shutdown

Run `cargo run --locked -p riesel --features async-tcp --example execution_graph`. The example executes two deliberately overlapping async branches, an explicit `start` signal and a completion join, using a single real `Application::run` session. It awaits the entire graph on the caller-owned Tokio executor **before** `Application::run` tears down the session. It neither performs a second provider-resolution pass nor creates a second Riesel application.

The runtime's structured `AsyncTasks` interface is separate from arbitrary `tokio::spawn`. A graph detached into a caller-created task is **not** automatically registered or guaranteed joined before provider teardown. Use **either** `runner.execute().await` within the session entrypoint **or** `context.async_tasks().spawn_graph("name", runner)` inside an active async scope. The latter uses the existing bounded named application-task owner, not a second scheduler. A runner from another application session is rejected before task admission via an **in-process, clone-based session identity check**; this is not remote authentication or proof of executable provenance. Normal scope shutdown requests a cooperative stop, aborts and **joins** all graph-owned Tokio operation futures before their provider instances are released. A completed unsuccessful graph propagates `GraphExecutionFailure` (including its local `GraphRunReport`) as a named `RuntimeError::TaskFailed` and revokes the application session; planned shutdown itself is not a task failure. It neither automatically retries nor rolls back previously sent effects.

A graph interrupted during a structured stop cannot claim that dispatched effects did not occur. Async callback futures are dropped and joined, but operations that start independent native threads or external mutations must arrange their own teardown, fencing and reconciliation. Noncooperative synchronous code on a Tokio worker can still block shutdown. Authoritative committed effects and live reports remain part of [#113](https://github.com/aschenrot/riesel/issues/113).

## Real managed-process crash and uncertainty fixture

The opt-in [managed graph integration fixture](../../../crates/riesel_app/tests/managed_graph_lifecycle.rs) runs with `cargo test --locked -p riesel_app --features async-tcp,test-fixtures --test managed_graph_lifecycle`. Its crash-service executable lives under `tests/fixtures/` and is **not built by ordinary application/SDK builds**. The fixture uses **production managed-service spawn, ephemeral TCP readiness, a real OS process exit, session monitoring, and the actual graph runner**. Its small private command protocol (`COMMIT`, `CRASH`) is fixture-only, not Riesel RPC or an example of authenticated provider identity. Both the directly awaited graph and the registered `AsyncTasks::spawn_graph` path are exercised.

The test first obtains a provider-confirmed acknowledgement, then dispatches another command whose child intentionally exits without replying. The already-confirmed operation remains completed, the in-flight graph operation is conservatively outcome unknown, success-only dependents do not run, stale signals are rejected, and the actual managed-service exit remains the primary failure. The structured variant verifies graph callback futures are dropped/drained before the owner scope finishes without creating a secondary graph-task error. A fresh managed application session is distinct and does not *implicitly* reexecute an old runner. **This does not prove cross-process exactly-once effects**, generic transport authorization, or that an opted-in application restart will refrain from repeating its *own* entrypoint logic.

## Fixed-tick ownership acceptance fixture

The feature-gated [two-policy domain fixture](../../../crates/riesel/tests/execution_domains.rs) compares the same two committed simulation ticks (four proposed, catch-up budget two) under **Riesel-admitted outer ticks** and a **domain-owned nested frame**. Both run alongside an overlapping two-branch async workflow and final join. The native `Rc<Cell<...>>` world never leaves its own actor thread. Per-tick nodes appear only in the central-admission projection; the delegated graph exposes one frame, not fictitious internal ECS-system steps.

The fixture also rejects stale domain permits and explicit owner mismatches before mutating state. A domain-owned frame cancelled after its first committed tick yields `OperationOutcome::Cancelled`, blocks its dependent final node and preserves the first tick in the domain's actual world. **This is a tested adapter/fixture invariant, not a published operation-authority metadata contract or a general-purpose fixed-rate clock policy.** Riesel currently cannot inspect arbitrary user code and guarantee every operation has declared the correct domain owner. This fixture demonstrates actual domain-world commits, not inferred graph success. Bounded attempt-scoped confirmed-prefix accounting is implemented under [#113](https://github.com/aschenrot/riesel/issues/113); cooperative controls and authoritative external reconciliation remain outstanding.
