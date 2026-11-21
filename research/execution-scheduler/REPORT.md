# Riesel Issue #83 — general-purpose execution architecture investigation

**Status:** research and recommendation only. **Not** an accepted product outcome, public API, runtime contract, or roadmap change.  
**Baseline:** `aschenrot/riesel/main` at [`0798a6fb8815a57bca822c2db9f220e21ec14354`](https://github.com/aschenrot/riesel/commit/0798a6fb8815a57bca822c2db9f220e21ec14354) (2026-11-21).  
**Specification:** [#83](https://github.com/aschenrot/riesel/issues/83). **Research PR:** [#84](https://github.com/aschenrot/riesel/pull/84).  
**Scope:** compare complete product outcomes, execution ownership, long-term growth, and bounded headless experiments. All type and command names below are **illustrative research vocabulary only**.

## 1. Product jobs and independent acceptance decisions

The proposed full platform outcome is that a Rust developer can **compose a valid executable plan, inspect it without executing its operations, run it headlessly, observe live progress, and intentionally advance supported execution boundaries**. Domain-specific scheduling and native Rust behavior remain usable. It must credibly serve both a fixed-tick simulation and an asynchronous, event-driven application; neither domain alone defines the platform.

Do not silently reduce this to tracing, and do not infer all Rust control flow is serializable. Treat these as **separable scope decisions**, not already accepted product requirements:

1. **Headless executable composition:** declarative *operation relationships* and reusable validated execution/admission semantics, with Rust SDK and machine-readable inspection. This is the primary proposed platform feature; decide whether the general execution kernel itself is a supported product.
2. **Live observation and safe control:** stable logical/run/attempt/session correlation, authoritative state reads and cooperative pause/advance/resume for work implementing safe points. Inspection is useful even for coordinators that Riesel does not schedule internally.
3. **Optional visual inspection:** graph/timeline/snapshot UI over the same explicitly scoped inspection semantics; neither an always-on Manager nor a GUI dependency for headless applications.
4. **Optional visual executable authoring:** editor-originated graph must round-trip to a versioned plan whose operations are registered/authorized Rust adapters. Graph shape is not serialized arbitrary Rust closure bodies. Generated nodes, code-owned plans and edited plans need an explicit source-of-truth/round-trip contract **before** a GUI designer can promise faithful execution.

The last two are distinct: graph visualization is not graph authoring. No acceptance of visual authoring can be inferred from a graphical trace.

## 2. Audited accepted baseline and concrete gaps

| Existing authoritative mechanism | Verified file / contract | What it does *not* imply |
|---|---|---|
| Versioned capability requirements and `AppSchema::startup_order()` | [schema](../../crates/riesel_schema/src/schema.rs), [authoring contract](../../docs/product/riesel/api-authoring-v3.md) | Startup dependency DAG does not describe operation, tick, timer, branch or dataflow execution |
| `DeploymentPlan` and preflight | [plan](../../crates/riesel_app/src/plan.rs), [runtime planning](../../crates/riesel_app/src/runtime/planning.rs), [catalog](../../docs/product/riesel/provider-catalog.md) | Provider resolution and process selection must not be duplicated inside an execution plan |
| Async app entrypoint, session and task scopes | [async handler](../../crates/riesel/src/application/async_handler.rs), [AsyncTasks](../../crates/riesel_app/src/async_tasks.rs), [session token](../../crates/riesel_protocol/src/session.rs) | Tokio execution and bounded session-owned tasks are not a business-operation scheduler/inspector; task name is not logical node ID |
| Sync task registry and process/managed supervision | [tasks](../../crates/riesel_app/src/tasks.rs), [RunningApp](../../crates/riesel_app/src/runtime/running.rs), [lifecycle](../../docs/product/riesel/runtime-lifecycle.md) | No forced Rust-thread pause; no automatic per-node restart, portable CPU/GPU affinity or guaranteed side-effect replay |
| High-level builder, modules, typed native handles | [SDK parameters](../../crates/riesel/src/application/parameter.rs), [modules](../../docs/product/riesel/application-modules.md) | A node is not inherently a capability, provider, module, RPC or OS process |
| Existing operational investigation | [#75](../../docs/product/riesel/application-operational-boundary-investigation.md) | Provider readiness != app-interface readiness; in-process SessionToken != authenticated external control identity |

Current reference transport is loopback plaintext with no caller authentication; completed operations and externally executed commands cannot be unwound by cancelling a future. Existing whole-session recovery may replay entrypoint side effects. A future inspector must not claim a side-effect-free inspection simply because `--print-definition` does not start providers: launching an arbitrary app binary executes its own Rust initialization (independently tested under #75).

Independent comparison: [RunenECS architecture](https://github.com/dornglut/runen-ecs/blob/main/ARCHITECTURE.md) owns ECS world state, system precedence, deferred mutation, resource access and a serial-reference/deterministic-parallel schedule. The ECS may be invoked **inside** a Riesel plan operation; Riesel may not independently schedule individual RunenECS systems while also claiming to preserve RunenECS semantics. RunenECS is a separate project, not part of Riesel; feature overlap is legitimate, competing authority over *the same state and operations* is the failure mode.

## 3. Equal-scope architecture alternatives

**C — broad Riesel-owned scheduler.** Riesel validates graph and data/control edges, admits runnable nodes, coordinates triggers/time policies and bounded parallelism, keeps authoritative states and debug controls, and calls external executors. Strong fit for common operation graphs and graphical authoring. Significant challenges: reference determinism for concurrent conflicting operations; serializing a meaningful plan without serializing arbitrary closures; `!Send` native references, GPU/main-thread affinity, zero-copy dataflow; safe distributed effects, fairness, policy coupling and interoperability with nested ECS/workflow schedulers. Not ruled out by upfront effort alone; global universality remains unproven.

**H — shared kernel with optional general graph runner and explicit domain adapters (candidate).** Shared graph validation, node identity, state/trace/control contracts and common bounded graph runner for supported general operations. A simulation/domain coordinator retains its *internal* system/tick/frame/state authority; it can be invoked as a coarse operation and offer deeper domain-specific inspection via explicit adapter. A domain may choose to use common primitives if its semantics genuinely match. This retains executable generic workflows while avoiding a second ECS system scheduler. Additional complexity is *multiple levels* of execution inspection/control, explicit ownership and adapter versioning. This is not automatically cheaper than C and cannot simply hide capability gaps in adapters.

**B — shared semantics only, specialized coordinators schedule everything.** High expressiveness for existing ECS/task engines, lowest interference with native borrows; common IDs/events could serve useful inspector. But the proposed *Riesel-executable authored graph* lacks a common execution authority. To meet that part, every application/domain implements its own graph runner or a user-facing compiler/adapter must own the missing scheduling: cumulative duplicated code and inconsistent controls. Valid **only if** executable visual composition is revised/deferred or a separate runner proves adequate.

**A — domain execution plus ordinary tracing.** Existing `tracing`, Tokio console, OpenTelemetry spans may offer immediately useful diagnostics at lowest intervention. Does not by itself produce a validated executable plan, authoritative node state, pausable work, or faithful node-level control. The minimum comparator, not a complete answer to the proposed outcome.

**External semantic comparators:** [Bevy ECS schedule](https://docs.rs/bevy_ecs/latest/bevy_ecs/schedule/) (system access and ordering domain), [Tokio JoinSet](https://docs.rs/tokio/1.54.1/tokio/task/struct.JoinSet.html) (async task mechanics, not business graph authority), [OpenTelemetry traces](https://opentelemetry.io/docs/concepts/signals/traces/) (portable observer/export signal), and [Temporal workflow semantics](https://docs.temporal.io/workflows) (durability/replay assumptions for a *different* workflow class). None establishes a mandatory provider, runtime, or chosen API.

| Common criterion | C: general scheduler | H: shared kernel/adapters | B: shared contracts | A: tracing |
|---|---|---|---|---|
| Generic executable operation DAG / branches | Native | Native for supported graph operations | Per-domain or new runner | Not supplied |
| One consistent plan + authoritative node state | Potentially strong | Strong for kernel-owned work; adapters need agreement | Depends on coordinator | Trace reconstruction only, possibly lossy |
| Native ECS deterministic serial-reference semantics | Must explicitly delegate/reconcile | Delegate internal schedules | Native domain authority | Native domain authority |
| Async workflow join, cancellation, unknown effects | One kernel can express but not guarantee transactionality | Shared runner can express, side effects remain domain-owned | Reimplemented per coordinator | Delegated to app |
| Borrowed state / `!Send` / affinity | Requires differentiated executor kinds | Explicit support or domain delegation | Direct domain control | Direct domain control |
| Cooperative step operation / tick / frame | Requires distinct safe-point contracts | Shared outer operations + domain-specific internal stepping | Per coordinator adapters | Not supplied by tracing |
| Visual executable round-trip | Plausible shared IR and registry | Plausible shared IR for kernel + explicit adapter boundaries | Requires extra execution source of truth | Not supplied |
| Upfront construction and growth cost | Highest uniformity ambition and semantic risk | Kernel plus adaptation/version costs | Small initial core but repeated integration cost | Lowest initial cost and largest functional gap |
| Reversibility | Hard if one universal scheduler becomes foundational | Moderate if plan/executor/domain interfaces remain separable | Future standard runner may require migration | Future graph model requires migration |

These are **conditional architectural properties**, not measured weighted rankings. **C explicitly allows nested specialized engines already**; therefore showing one RunenECS-like operation inside a graph does **not** distinguish C from H. The substantive policy question is whether Riesel's scheduler universally owns *outer* node eligibility, clock and control (C), or whether domain coordinators can own some admission/timing policies while reusing common graph/observer semantics and an optional graph runner (H). Both can share the same code and runtime for the simple probes; their practical distinction must be demonstrated with a conflict/ownership test, not inferred from the names. C and H both warrant first-class continued research; initial minimal effort does not select B/A.

## 4. Proposed responsibility and identity model (not accepted)

| Concept | Single authority | Lifetime and key constraints |
|---|---|---|
| Application definition / capability requirement | Riesel SDK + `AppSchema` | Declared contract/alias; independent of execution-graph node |
| Provider resolution / placement | Existing Riesel resolver/catalog | Once per session/deployment; never re-resolve inside graph |
| Provider instance / application session | Existing `RunningApp` and session token | Handles expire on teardown; recovery makes a **new session** |
| Execution plan / operation ID | Candidate plan validator and application author | Plan revision + stable logical node ID; closure implementations registered separately |
| Scheduler admission / runnable-node state | **Exactly one** coordinator per node | Kernel-owned generic nodes **or** domain-owned internal nodes, never two competing dispatchers |
| Execution adapter | Typed capability or plain Rust invocation or explicit nested runtime | Invokes but does not silently own providers, processes or domain state |
| Domain tick / ECS systems / frame composition | Explicit domain coordinator, e.g. RunenECS for ECS systems | Domain controls state/borrows/systems; an outer operation can request one tick, not split its hidden schedule |
| Async tasks / noncooperative blocking work | Existing application-session task scope | Lifetime fenced by session, cooperative cancellation and bounded joins |
| OS processes | Existing process supervisor/Manager or external OS owner | Never inferred from a graph-node ID; external service remains externally owned |
| Observer and event exporter | Bounded read-only diagnostic surface | Loss-aware and not a source of authoritative mutable state |
| Debug controller | One authenticated session-fenced controller when externally exposed | Cooperative safe-point pause/step/resume; separate permissions for inspect vs control |

**Research-only proposed vocabulary:** `ExecutionPlan { plan_revision, nodes, edges, trigger_definitions }`, `ExecutionNode { logical_id, executor_binding, access, affinity, dependencies }`, `ExecutionEvent { session_generation, run_id, node_id, attempt_id, seq, transition }`, `ExecutionSnapshot { authoritative_states, revision, cursor }`, `StepControl { pause_at_boundary, advance_operation, advance_domain_tick, resume }`. A plan is a *validated executable declaration* and inspection artifact only for registered operations; it is distinct from provider deployment plans and emitted traces. A trace is historical, potentially lossy evidence; a snapshot must be obtained from the authoritative coordinator. No single clock or globally total event sequence across remote processes is implied.

**Static validation:** graph identities, types/edge compatibility when declared, cycles, unsupported executor mode, known resource/affinity constraints and serializable metadata. **Runtime checks:** session generation, resource availability, capability binding, actor authorization, admission capacity/deadlines, emitted event limits, supported safe points, actual provider failures and partial effects. Unsupported portability must fail explicitly, not silently convert a local borrow/GPU resource into an RPC value.

**Timing/control:** a fixed-step simulation's **tick source** should be a domain policy; display frame is a separate consumer with 0/N bounded ticks. Advance-one-operation does not mean advance-one-tick, and neither means advance-one-rendered-frame. Work with no safe point can complete before pause or remain pending; no arbitrary Rust preemption. A `!Send` resource or thread-affine native loop requires a distinct placement/adapter strategy, not the Tokio `Send` path used by this probe.

## 5. Reproducible research executable and observations

The isolated package under [`research/execution-scheduler/`](./) is **not a workspace member** and does not add production SDK types. Run from Riesel repository root with the pinned Rust 1.100.0 and pre-cached locked main dependencies:

```sh
cargo validate --check
cargo smoke
cargo test --manifest-path research/execution-scheduler/Cargo.toml -- --nocapture
```

Research code uses the **same generic runner** for both domains; nodes are ordinary Rust closures, with one typed Riesel `AsyncCounterContract` call inside a true `Application::async_handler_with_aliases` session. No graph node is modeled as a provider. Tokio tasks execute on a **current-thread test executor** for most probes: simultaneous async eligibility proves concurrency/interleaving, **not** multi-core CPU execution.

| Probe | Concrete asserted outcome | Limitation |
|---|---|---|
| Plan preflight | Empty/duplicate IDs, missing/duplicate dependencies and cycles rejected before invoking operations | No versioned portable node/code registry or typed data-edge validation |
| Ordering / concurrency | Dependency runs before successors; independent branches meet an async barrier, then join; incompatible declared read/write accesses do not overlap | Resource declarations are trusted; no static borrow analysis, fairness proof, `!Send` placement or real threads |
| Correlation and failure | Node starts/completes with matching attempt IDs, failures block descendants; panicking async operations become failed-node status | No structured source/panic reporting or session-policy propagation yet |
| Cancellation and stepping | Stop-before-admission marks pending nodes cancelled; `advance(1)` admits one next node and returns at operation boundary | Running work is not forcibly stopped; step count is admission budget, not an OS debugger |
| Minimal fixed-step simulation | 0,1,4 proposed ticks with cap 2/frame -> exactly 3 ticks; deterministic input -> sim -> snapshots of (tick, position) = (1,2),(2,4),(3,6) | Minimal world, **not an integrated RunenECS schedule**, no physics/GPU, no recovery/restart proof |
| Non-game event workflow | Gated external event, two independent concurrent branches, join, side effect followed by lost acknowledgment; side effect invoked once and successor blocked | Trigger is a local one-shot bit, not a durable event queue/timer/subscription; cannot prove exactly once across restart |
| Real SDK capability | Resolved async Counter called from graph node within Riesel-owned application session; value becomes 3 | Only linked `AsyncMemoryCounter`, not managed/remote deployment or session-crash/recovery test |
| Trace and inspection | Fixed-capacity retained events and evictions; disconnected subscriber counted; authoritative snapshot retains three nodes while trace keeps only two | No auth/export, replay, sampled timestamps or cross-process ordering |
| Observational timing | Same 16-node ×80-run = 1280-operation workload with trace enabled vs disabled | Single short debug-build timing sample, no reliable percent-overhead or scale conclusion |

**First tested head `19c9599`:** Linux [Actions 44869952993](https://github.com/aschenrot/riesel/actions/runs/44869952993): `cargo validate --check` passed, `cargo smoke` passed, isolated **7/7** tests passed. Raw measurement printed `instrumentation_enabled=false: 4,754,595 ns`; `true: 4,212,814 ns` for 1280 operations; the *opposite sign* of expected overhead illustrates noise/order effects, not a performance win or zero-cost instrumentation. Both figures measure whole run scheduling/allocation on one ephemeral hosted CI environment, **not per-event production overhead**.

**Probe review correction:** parallel completion initially emitted the global *latest* attempt rather than the node's attempt; panic initially propagated through a `JoinSet::join_next().expect`. Subsequent research-only changes track node-specific attempt IDs and map panicking workers to failed-node states, with new tests; **the corrective Gate B head `73f8e16fa06182c28a29a109955750a363f2e775` passed all Linux gates and 8/8 research tests in [Actions 44870596199](https://github.com/aschenrot/riesel/actions/runs/44870596199); this does not validate later Gate C changes**. This is concrete evidence that a meaningful observer requires stable run/attempt identity and explicit worker failure containment.

## 6. Correctness, security and performance uncertainties

**Decision-critical unproven matters:**
- **Generic vs domain scheduling:** compare an *actual* RunenECS nested step and domain-owned contract adapter on the same world; prove paused tick/time semantics, deferred ECS mutations, determinism and bounded catch-up across stop/restart. A simulated world cannot validate those ECS invariants.
- **Plan authoring/round-trip:** distinguish Rust-authored plans, generated metadata, editable graph source of truth and immutable plan revision. Test versioned operation registry, typed data edges, schema migration, forged/stale plan rejection, cycles, code/plugin provenance and unsupported graph constructs.
- **Resource safety and placement:** measure `Send` vs `!Send`, invoker-thread-only operations, native graphics/window boundary, borrowed-state lifetimes, read/write access validation and accidental concurrency conflicts. Never infer memory access claims from arbitrary opaque closures.
- **Execution policies/limits:** explicit priorities, fairness, starvation, deadlines, event queues/overflow, timers, backpressure, deterministic serial reference, physical CPU parallelism, admission budget=0 semantics, operation error types and failure propagation to parent session. Test bounded queue, runaway and malicious graph input.
- **Live debug & recovery:** active-operation cooperative cancellation, breakpoint reentrancy, step operation vs tick vs frame, snapshot consistency during parallel work, nested boundaries, observer isolation, discontinuous traces, session epoch fencing and how process restarts invalidate control handles.
- **Performance:** repeated randomized A/B traces on stable hardware with release builds, node-count scaling, trace memory, resource conflict density, many-tick loop and long-lived observers. The first CI microtiming cannot decide architecture.
- **Trust and side effects:** app/process/UI authorization, exposed private state, capability scopes and secrets, stale-session control rejection; unknown in-flight external effects require explicit idempotency/deduplication/reconciliation. No automatic replay/retry.

A controller that can mutate execution is more sensitive than telemetry. The local research code neither exposes network controls nor authenticates them. [#75](https://github.com/aschenrot/riesel/issues/75) owns any reusable externally represented app/session/interface identity and authorized live export; this does not block in-process research. [#59](https://github.com/aschenrot/riesel/issues/59) owns Manager processes, [#60](https://github.com/aschenrot/riesel/issues/60) owns GUI host, [#68](https://github.com/aschenrot/riesel/issues/68) owns developer launch. None is a blanket prerequisite to choosing an internal plan/runner model.

## 7. Developer ergonomics and proposed acceptance tests

**Illustrative SDK experience (NOT implemented):** author plain Rust functions and registered capability operations; construct a versioned plan of stable logical operations and explicit dependencies; ask `inspect_plan()` before execution; run under an existing Riesel application session; request `snapshot()` and `step_operation()` at allowed boundaries. No requirement that operation implementation become a component or RPC.

**Illustrative CLI experience (NOT implemented):** inspect an existing trusted built artifact/explicitly authorized app and present plan revision, dependencies, capabilities and validation; connect to an already running, explicitly exported, authorized session to `watch`, `snapshot`, `pause`, `step`, `resume`. Invoking a binary for discovery is not passive inspection; no global Manager is assumed.

A concrete **NON-COMPILING API sketch**, deliberately leaving registration and placement unselected:

```rust,ignore
// Illustrative only: none of these types/methods exist in the Riesel SDK.
let plan = ExecutionPlan::builder("simulation-pipeline", 1)
    .node("input", input_operation)
    .node("simulation", nested_domain_tick_operation)
    .node("snapshot", snapshot_operation)
    .after("simulation", "input")
    .after("snapshot", "simulation")
    .build()?;
let prepared = session.execution().validate(plan)?; // no operation runs
let metadata = prepared.inspect_plan()?;              // version + bindings + edges
let run = prepared.start()?;                          // existing app-session owner
let before = run.snapshot().await?;                   // authoritative node state
run.pause_at_safe_point().await?;
run.advance_operation().await?;                      // NOT an arbitrary thread pause
// run.advance_domain_tick() requires an explicit domain adapter.
```

Illustrative future CLI interaction (not runnable commands and not an accepted CLI contract):

```text
riesel execution inspect --trusted-artifact ./app.plan.json
riesel execution watch   --app APP --session SESSION
riesel execution pause   --app APP --session SESSION
riesel execution step    --app APP --session SESSION --boundary operation
```

The first command assumes a trustworthy *already-produced* metadata artifact and must verify provenance/revision; it must not claim passive discovery by executing an untrusted binary. The latter commands require explicit app export, caller authorization, session fencing and a declared safe-point policy (shared with #75), and must fail clearly when unavailable. CLI naming/flags remain exploratory.

**Illustrative GUI experience (NOT implemented):** graph view with explicit ownership boundaries (Riesel outer node vs domain internal schedule), timeline showing event loss, authoritative current state and safe-point control availability. An optional composer may edit only declared/registered operations and statically validated graph edges; visual edits must yield the *same execution meaning* as SDK/CLI. It cannot synthesize executable arbitrary Rust from a rendered trace.

Acceptance tests to consider **only after product/mechanism agreement**: independent Cargo SDK user; graph serialization/round-trip and rejected stale code bindings; deterministic domain serial oracle and async branch/join; safe nested step/tick/frame; `!Send`/thread affinity; error/panic/cancellation and recovery; event loss and hostile observer; saturation/fairness/resource limits; authenticated stale-controller rejection; extensibility across 2+ unrelated concrete apps; real measured overhead and migration cost. No production Issue should assume these accepted.

## 8. Bias audit and recommendation for maintainer review

**Underinvestment bias:** counting only added scheduler code and excluding repeated workflow/visual authoring adapters makes B/A look unfairly cheap. Treat general graph authoring/stepping as a real cost over the growth horizon. Conversely, failure to implement all generality in this small probe is not reason to disqualify C.

**Over-abstraction bias:** calling every operation a provider, treating a plan as a serialized Rust closure, assuming all systems are `Send`, globalizing fixed-tick determinism, or requiring a mandatory universal daemon would prematurely freeze impossible or unnecessary semantics. Distinguish coarse cross-domain operations from domain-owned internal semantics. RunenECS is evidence of domain constraints, **not** an architectural veto on a Riesel kernel.

**Familiarity/source bias:** Bevy, Tokio, RunenECS, Temporal and existing Riesel tasks each address different scheduling classes. Neither popularity nor proximity to existing code selects a universal runtime. The tests are intentionally written around the candidate kernel and therefore prove its *feasibility*, not that it beats B, C or H. Existing scheduling and telemetry must be compared on *equivalent user outcomes*, including unimplemented graph authoring costs.

**Decision recommendation (NOT accepted):**
- **Product outcome: REVISE.** Explicitly separate (a) headless executable composition, (b) authoritative inspection and cooperative control, (c) optional visual inspection, and (d) optional editable/round-trippable visual composition. Seek maintainer acceptance of each; avoid silently turning exploratory goals into roadmap.
- **Mechanism: DEFER acceptance; C versus H is NOT discriminated by the current successful probe.** Both can own the tested generic outer graph, nest domain-specific execution, and reuse one SDK-owned application session. **C remains equally viable** if Riesel can own outer eligibility/time/control coherently without owning ECS internals; **H remains equally viable** if domain-owned policies demonstrably improve timing/borrow correctness or avoid duplicated authority while preserving common graph/observer behavior. A necessary differential probe must test identical mixed async/fixed-tick workloads with competing clocks, explicit nested stepping, ownership/cancellation, instrumentation and authoring costs under both policies. Correct typed graph serialization, native borrow/affinity, true RunenECS integration, guarantees under failure/recovery, and reliable overhead bounds are still unproven. B/A remain valid *for narrower accepted outcomes* but do not independently fulfill shared executable graph authoring.
- **Delivery: NO production implementation Issues, accepted architecture contracts, milestone/roadmap adjustments or merge authorization.** Keep #83 open for explicit maintainer scope/architecture review. Complete focused differential experiments only where they could change the C-vs-H or product acceptance decision.

**Plain summary:** Riesel can plausibly own a reusable outer execution graph and inspector without replacing a game's internal ECS scheduler. A small Rust probe works; it does not yet show whether a single general scheduler or a policy-splitting hybrid is better. Important safety, authoring and performance guarantees remain unproven.


## 9. Gate C — differential ownership with actual RunenECS (research extension)

**Versioned input, not a Riesel production dependency:** the isolated research package uses the public RunenECS repository fixed at `5d6af86bfa7c58f1ad26ec73635590a72e89ef37`; that repository's crate pins RunenGraph at `075f3c2b4122c1ae89209d9fb7cb1feac6bba0d4`. Both are externally maintained, GPL-3.0-only research inputs; Riesel's workspace declares AGPL-3.0-only. A future production dependency/redistribution strategy would need its own legal, governance and update review; none is selected by this spike. Unlike the previous entirely cached probe, first-time Cargo execution requires downloading the pinned public Git dependency. The separate research workspace does not yet commit a Cargo.lock, so exact crates.io transitive resolution across future clean environments is **not** frozen; record the tested GitHub Actions run and resolved revision before claiming a reproducible result.

### Test hypothesis, controls and decisive differences

The new `src/differential.rs` fixture uses **real** RunenECS: its `World`, `Runtime`, component queries, ordered system sets, `Commands` deferred entity spawning, serial schedule execution, and an invoker-thread-only system accessing an `Rc<Cell<u32>>` resource. The non-`Send` resource and ECS world are created, used and dropped **on a dedicated owning thread**. The Tokio research graph only exchanges owned messages and bounded oneshot replies across a capacity-one channel; it does not move borrowed ECS state, replace the ECS system scheduler or assume that executor threads are interchangeable. Explicit affinity bridging is nontrivial integration code, **not** automatic portability.

| Criterion | C: outer graph owns tick admission | H: domain policy owns frame admission |
| --- | --- | --- |
| Tick source | External frame clock proposes 0/1/4 ticks, outer policy admits at most two distinct `Run` executions per frame | External frame clock proposes 0/1/4; one domain-owned frame command decides and executes up to two ticks |
| ECS schedule | Exactly one `Runtime::run_schedule` invocation per admitted tick, on the ECS-owned thread | Identical ECS schedule, world, input and invoker-only system |
| Invariant | Before advancing `ecs_tick`, an earlier `read_input` operation may complete with no tick executed; the plan can then advance `ecs_tick` and `publish_snapshot` separately | A domain `Frame` command returns after its internal 0/N ticks; individual nested tick safe points are not externally controllable in the current adapter |
| Observation | The outer `Run` snapshot includes its kernel-owned `ecs_tick` operation and correlated events | The domain returns authoritative ECS results, but generic `Run` has no entry for these domain-owned ticks; a unified inspector would need a deliberate adapter |
| Single admission authority | Opposing domain-frame commands are rejected | Opposing outer-tick commands are rejected |
| Event-driven work | Generic event-gated branch/join can execute concurrently; additionally one single graph contains both an ECS tick and an async two-branch join | A domain-owned simulation frame can run alongside a separately owned async graph; common live control is not automatically implemented |

The fixture asserts the **same ECS-observable state** from both policies after three bounded ticks: sorted positions `[2,12]`, then `[4,12,14]`, then `[6,12,14,16]` with invoker-only resource update counts 1, 2, 3. It tests attempted conflicting dispatcher commands and a single C graph joining a real ECS tick with an asynchronous event-driven branch. These are **asserted research outcomes, not a universal correctness or performance guarantee**. The exact CI status for the latest branch revision is maintained in [draft PR #84](https://github.com/aschenrot/riesel/pull/84) and [Issue #83](https://github.com/aschenrot/riesel/issues/83), rather than inferring success from source code.

### Findings that limit architectural acceptance

1. **C and H are policy locations, not necessarily independent schedulers.** Both can use the same generic operation-plan semantics and the same ECS execution bridge. Only one coordinator is allowed to admit a given tick. The long-term candidate worth testing is a shared execution/inspection kernel with *explicit per-operation policy ownership*, not a universal second dispatcher above a domain's own scheduler. This is a revised hypothesis, **not accepted architecture**.
2. **The external clock remains application-controlled in this probe.** Kernel-owned tick *admission* does not prove a universal game clock, frame pacing, timed triggers or all-domain deadline semantics. H's per-frame tick completion does not prove nested per-system stepping; the tested adapter provides no external safe point between internal ticks.
3. **RunenECS already exposes `inspect_schedule` diagnostics** for precedence, access conflicts and mobility, plus a parallel executor callback at deferred-publication frontiers. Its diagnostic system descriptors are snapshot facts, not persistent universal operation IDs. Neither facility is a general Rust system debugger. An adapter must distinguish authoritative domain state, diagnostic plan facts and execution traces.
4. **The generic research `Run::advance(&mut self)` holds exclusive mutable access** until its execution budget reaches a boundary, so tests that read `snapshot()` afterward are **not** proof that an independent controller can read an authoritative live snapshot or request pause while a long operation is in flight. Active cancellation, concurrent inspection, operation breakpoints and session-fenced controls need an independent coordinator handle/ownership proof before any product promise.
5. **The ECS thread bridge is an example of a sound boundary, not proof of automatic affinity.** Queue exhaustion, lifetime closure, panics, cancellation in a frame batch, long-running operations, nested errors and stale-session commands still require typed semantics and testing. No deterministic parallel ECS executor comparison, graphics/GPU loop, full multi-core benchmark, remote transport, persistent plan serializer or executable visual authoring round-trip is established.
6. **Bias check:** the generic graph's visibility advantage for C is partly built into this fixture. H can recover unified inspection if it exports domain-owned state via a correct adapter; that adapter's design/cost and intra-tick semantics must be measured rather than presuming H cannot inspect. Conversely, pretending one shared timeline already coordinates H would ignore real integration work. B/A still deserve comparison for narrower accepted requirements.

**Revised Gate C recommendation — pending full evidence and maintainer review:** keep headless composition and authoritative inspection under separate explicit product decisions; evaluate a *single shared semantic model with configurable, exclusive admission ownership* as a way to reconcile C and H. Do not accept it until concurrent live snapshot/control, frame/tick safe-point rules, external resource ownership, versioned authoring/plan source of truth, failure/restart trust boundaries and meaningful overhead/adapter costs have been demonstrated. No implementation authorization follows.
