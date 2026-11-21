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


## 10. Gate D — live-control proof and limitations (verified)

**Verified revision:** `71c131a4daf2235abfdc04fbfb18e6b6506f170b`, [Linux Actions 44874816519](https://github.com/aschenrot/riesel/actions/runs/44874816519): `cargo validate --check`, `cargo smoke`, and **17/17 isolated research tests** passed. These Gate D results supersede earlier claims that concurrent in-process observer/control and cooperative domain tick stepping have not been tested; they do **not** establish production readiness.

- `src/live_control.rs` tests a separate coordinator publishing watch snapshots while the outer `Run::advance(&mut self)` remains borrowed; independent handles can issue single-boundary admission, pause-on-next-boundary and resume; a generation mismatch cannot mutate coordinator state.
- Long, uncooperative regions continue until they reach a declared safe point. Cancelling a parked operation prevents its unstarted effect, but the generic graph runner currently classifies its returned cancellation error as `Failed`, exposing an **unresolved cancellation/error contract**. A live snapshot is about the **research coordinator**; it does not make every operation's internal state directly readable.
- `src/differential.rs` tests real domain-owned RunenECS per-tick permits with an explicit thread owner, stale-generation rejection, partial-frame cancellation and lossy nonblocking diagnostic observations. Already completed ECS ticks are retained, never rolled back by cancelling the remaining frame. `!Send` resources remain exclusively owned by the ECS invoker thread.
- Neither the watch channel nor integer test generation IDs authorize an external client. #75 owns authentication and public runtime-generation semantics; no external debugger/control protocol has been implemented.

## 11. Gate E — Bevy ECS comparison and independently evolvable RunenECS (research)

**Scope:** `bevy_ecs =0.20.1` (MIT OR Apache-2.0, declared Rust 1.95.0) is the standalone library, **not** the full Bevy engine, renderer, window host or Riesel application lifecycle. It is pinned as a test-only dependency alongside exact-pinned GPL-3.0-only RunenECS. Riesel remains AGPL-3.0-only. The RunenECS maintainer also controls the Dornglut repositories, so RunenECS may evolve in response to verified requirements; it is **not** a fixed third-party API or automatically Riesel's mandatory runtime.

**Controlled parity probe:** `src/bevy_comparator.rs` builds the same three-system serial-reference workload in **both** ECS engines: deferred spawn, ordered movement, invoker-thread-only non-`Send` state update, identical initial state and input, and frame tick proposals `0/1/4` bounded to two per frame. Both use intentionally serial executors; sorted ECS snapshots and update counts are the oracle, not iteration order. Successful parity **does not** establish equal unordered scheduling semantics, deterministic Bevy parallel execution, throughput or memory cost.

**Distinct inspector experiment:** Bevy `bevy_debug_stepping` exposes `Stepping::begin_frame`, `step_frame`, `continue_frame` and `set_breakpoint` against a configured schedule. The fixture tests actual effects under a headless `bevy_ecs::Schedule`, rather than inferring them solely from documentation. A `Schedule` initialized for one Bevy `WorldId` cannot be reused on a new `World`; the first comparator CI run ([44875459337](https://github.com/aschenrot/riesel/actions/runs/44875459337)) demonstrated this intentional ownership assertion. The fixture was revised to construct a new frame schedule for the new world. This is an **adapter lifecycle consideration**, not evidence that stepping is broken.

**RunenECS's existing alternative:** `Runtime::inspect_schedule` gives diagnostic system/ordering/access/mobility facts; `run_schedule_with_deferred_publication_frontier` and its parallel variant can invoke a callback **after semantic deferred-publication cuts** on the schedule-invoking thread. Those callbacks are not a built-in per-system breakpoint/continue mechanism and may not occur at every system. A debugger could expose coarse authoritative safe points without adding full system stepping; system-level stepping should be considered separately for its impact on deferred visibility, serial-reference correctness, partial-frame state, parallel worker fences and normal/diagnostic execution equivalence.

| Question | Bevy ECS v0.20.1 | RunenECS pinned research revision | Evidence limit |
| --- | --- | --- | --- |
| Fixed-tick serial ECS reference | Exact same fixture scenario | Exact same fixture scenario | Test-only functional parity; no release benchmark |
| Explicit non-`Send` state | `World::insert_non_send`, `NonSend<T>` on invoking thread | `Resource` with `Rc<Cell<T>>`, explicit `on_invoker_thread()` | Does not prove OS/GPU affinity portability |
| Execution graphs | `Schedule::graph()` and systems metadata | `inspect_schedule` with ordering/access/mobility diagnostics | Neither is a versioned cross-runtime executable IR |
| Domain debugger | Feature-gated per-system stepping/breakpoints | Publication-frontier callbacks; separate per-system stepping not established | Different safe-point granularity; no arbitrary Rust preemption |
| Parallel scheduling | Multi-threaded support available under explicit feature; serial executor selected for parity | Serial oracle and deterministic parallel realization documented | **No matched parallel benchmark or differential test yet** |
| Evolution and licensing | External permissive dependency; upstream API/MSRV evolution | Owner-controlled independent GPL-3.0-only project | Must still assess compatibility, contributor rights and long-term maintenance |

**Bias controls and next evidence:** Do not declare Bevy superior merely because a debugger API already exists, or RunenECS superior merely because its serial oracle is explicit or its maintainer also owns Riesel. Measure whether per-system stepping is actually a necessary *accepted* product outcome, and whether implementing the required safe points upstream in RunenECS is worth the design/verification cost. A future fair performance comparator must pin complete dependency resolution, CPU/OS/compiler/features, workload/entity counts, warmups and repeated release-build samples; separate serial and parallel execution and state whether observability is on. Short debug-build CI timings are **not** performance evidence. Revisit the plan/visual round-trip, borrow/affinity contracts, session failure/control and security requirements independently of ECS choice.

**Delivery decision remains:** Keep #83 open and PR #84 draft/research-only; no change to accepted Riesel or RunenECS product contracts, public APIs, implementation Issues or roadmap. Favor independent optional ECS integrations if matched outcomes show a coherent common control/observation model. The Bevy comparator CI outcome must be recorded against its **exact tested revision**, not inferred from the first-run partial success.


## 12. Gate F — serial/parallel equivalence, release sampling and direct-query attribution (research)

**Authoritative tested inputs:** Riesel test-only research branch, standalone `bevy_ecs =0.20.1`, exact RunenECS `5d6af86bfa7c58f1ad26ec73635590a72e89ef37`, Rust `1.100.0`, Linux x86_64. RunenECS current `main` was checked and matched this pinned revision during the experiment. This does not make RunenECS a production Riesel dependency.

**Evidence sources:**
- [`82cb1690c83229cfbbee75ea1a743a058da12739`, successful CI 44876653339](https://github.com/aschenrot/riesel/actions/runs/44876653339) — serial/parallel correctness and paired end-to-end release sampler.
- [`9ef9948c446988ebbcf7960c5008b8d6e66fdedb`, successful CI 44877157317](https://github.com/aschenrot/riesel/actions/runs/44877157317) — **21/21** research tests, including direct-World read/mutate parity, and expanded release sampler; workspace validation and smoke gates also passed. The exact revisions are separately retrievable via the linked run; do not pool their absolute medians as if measured simultaneously.

### Methodology and semantic oracle

`src/ecs_parallel.rs` constructs independent, matched four-component Worlds and registers four-system workloads: empty scheduling, four compatible reads, four independent component writers, and four conflicting writers of one component. Both engines execute the same arithmetic over identical row counts. Conflicting writes deliberately **commute**, since neither engine's generic unordered schedule guarantees a chosen user-code order. The test checks actual authoritative component sums after repeated execution for 0, 64, and 512 entities under both serial and four-worker modes, comparing every engine/mode to the same expected result. This establishes **bounded functional parity**, not identical ordering, parallel scaling or arbitrary deterministic user callbacks.

The standalone `benches/ecs_scaling.rs` harness uses the optimized Cargo bench profile, precreates Worlds/plans outside timing, warms each instance six times, takes seven paired warm-process samples, alternates engine order, runs a fixed number of schedule invocations per sample and rechecks world-state sums after timing. It emits `BENCH_CONTEXT`, `BENCH_RESULT` and `BENCH_DIRECT` log records with medians/min/max and a ratio, not a threshold/gate. The runner reported **two available CPUs**, while Bevy's global ComputeTaskPool was configured for **four worker threads** and RunenECS admission capacity was **four**. Both engines ran with the standalone Bevy ECS `bevy_debug_stepping` feature compiled but not enabled in these workloads. This is a hosted, oversubscribed execution-policy comparison.

In the second run, `run_direct` probes invoke **public World query construction and iteration without scheduling** over the same four-component Worlds. A read computes a checksum; a mutation increments all C0 components, and an untimed authoritative sum verifies the outcome. This **still includes query construction, traversal and change bookkeeping**, and is not an isolated storage or cache-miss microbenchmark. Unlike the schedule workload, these direct probes touch only one of the four component columns.

### Results — measured samples, not comparative guarantees

For clarity, values below are **median elapsed microseconds per invocation**. All quoted observations are from one exact host/job per table; the first and second jobs are separate ephemeral hosted runs.

| Workload, 10,000 entities unless stated | Bevy ECS, CI 44876653339 | RunenECS, same job | Runen / Bevy |
| --- | ---: | ---: | ---: |
| Empty serial schedule | 0.097 | 0.225 | 2.32× |
| Empty parallel schedule, 4 workers | 22.565 | 154.979 | 6.87× |
| Four read-only query systems, serial | 7.451 | 156.377 | 20.99× |
| Four independent writes, serial | 49.836 | 1,494.247 | 29.98× |
| Four independent writes, parallel | 86.854 | 2,189.821 | 25.21× |
| Four conflicting writes, parallel | 139.505 | 1,828.840 | 13.11× |

| Workload, 10,000 entities unless stated | Bevy ECS, CI 44877157317 | RunenECS, same job | Runen / Bevy |
| --- | ---: | ---: | ---: |
| Four read-only query systems, serial | 5.110 | 88.883 | 17.39× |
| Four independent writes, serial | 20.313 | 823.535 | 40.54× |
| Four independent writes, parallel | 41.215 | 1,496.353 | 36.31× |
| **Direct single-column read, no schedule** | **1.586** | **22.312** | **14.07×** |
| **Direct single-column write, no schedule** | **7.158** | **217.348** | **30.37×** |

The different absolute values between separate hosted runs are material: the median Bevy four-writer serial case changed **49.836 → 20.313 µs** and RunenECS **1,494.247 → 823.535 µs**, despite comparable fixture definitions and identical compiler/version/nominal host class. **Do not turn this into a stable speedup, regression threshold, product choice, or claim that Bevy is inherently 30–40× faster**. Within each job the paired differences are consistent for this bounded workload, so the research question remains important.

### Critical interpretation, ownership and next gates

1. **Neither engine is being accused of incorrect scheduling.** Every tested engine/mode delivered the expected final ECS component values. Bevy is ahead *for the measured simple traversal/write workload*, but no attempt was made to compare full game features, ECS relationship semantics, exceptional failure behavior, arbitrary side effects, memory, compile time or real-world frame pacing.
2. **The serial gap is not chiefly caused by RunenECS worker creation.** A substantial read/write difference remains in **direct public World query paths without scheduling**, and grows with entity count. The next owner to profile is ordinary query state construction, dense row traversal and mutation/change-observation cost. It is not yet proven which internal step dominates.
3. **Parallel worker lifetime is separately non-equivalent.** The pinned RunenECS source physically launches scoped threads for planned worker cohorts; Bevy's CPU executor uses a reusable global pool. Changing that *physical policy* is possible under RunenECS ADR 0003, but it is not an architecture acceptance or a promise of better end-to-end results. The two-CPU/four-worker oversubscription also makes this run unsuitable for general multicore scaling conclusions.
4. **Avoid duplicating the existing RunenECS investigation.** RunenECS already delivered substantial evidence-driven query and mutation optimizations through [#114](https://github.com/dornglut/runen-ecs/issues/114), [#116](https://github.com/dornglut/runen-ecs/issues/116), [#118](https://github.com/dornglut/runen-ecs/issues/118), [#124](https://github.com/dornglut/runen-ecs/issues/124), [#126](https://github.com/dornglut/runen-ecs/issues/126), [#133](https://github.com/dornglut/runen-ecs/issues/133), [#147](https://github.com/dornglut/runen-ecs/issues/147), and [#157](https://github.com/dornglut/runen-ecs/issues/157). These have closed, with independent correctness and host-specific performance evidence; they are **not** proof the observed cost is fixed everywhere. Before proposing another RunenECS optimization, reproduce and attribute the current workload inside its own maintained Criterion benchmark suite, on the same pinned candidate/base host and under its safety/semantics gates. Do not open redundant RunenECS implementation Issues from this Riesel spike.
5. **Further evidence required:** a verified >=4 physical-core host with a controlled nonoversubscribed worker configuration; separate matched schedule/query-construction and cached-query iteration paths; meaningful 100k/1m workloads plus representative simulation; CPU profile/memory/caches; stable lockfiles/compiler/feature parity; transparent setup/serialization/capability adapter overhead; failure/deferred publication and observed debug-control costs. The existing isolated Riesel research workspace still lacks a checked-in Cargo.lock and does not guarantee transitive dependency reproducibility across future clean builds.
6. **Product ownership remains unchanged.** Riesel's execution-plan and safe-point contracts must be evaluated independently of either ECS's internal query throughput. A shared outer execution/inspection model can support *both*; no universal scheduler, per-system debugger, RunenECS redesign, framework replacement, or mandatory ECS dependency follows from these CI numbers.

**Gate F disposition: evidence collected; architecture selection still DEFERRED.** The simple-workload performance gap is credible enough to prioritize a targeted independent RunenECS profiling question, but insufficient to rewrite its memory/observability contracts. Riesel #83 stays open, PR #84 stays draft and research-only. Headless composition, authoritative local control, visual inspection, and executable visual authoring remain **separate product decisions**.


## 13. Gate G — versioned execution document and honest visual round-trip (research)

**Code under test:** `src/plan_document.rs`, with a bounded modification to the existing isolated `Node` / `Plan` / `Event` research model so that logical IDs and dependencies can be **owned strings**. Production Riesel APIs, `AppSchema`, `DeploymentPlan`, provider catalog and application lifecycle are **untouched**. The format below is an illustrative `Document` with **format_version=1**, not an accepted public wire format, SDK type, CLI format or cross-version migration promise.

**First verified head:** [`973145a4baa0d3f34b0a9ed02eabc32ec8943f59`](https://github.com/aschenrot/riesel/commit/973145a4baa0d3f34b0a9ed02eabc32ec8943f59) passed [Linux Actions 44884548029](https://github.com/aschenrot/riesel/actions/runs/44884548029): workspace validation, smoke, **27/27 research tests**, and the existing exploratory release sampling step. Its **six** new plan-document probes passed. A later follow-up adds a negative trace/plan-revision identity test; its exact-revision CI must be checked separately before recording that test as verified.

### Representation and trust boundary

The prototype separates four concerns:

1. **Authorable semantic document:** a plan ID, document format version, revision, logical node IDs, explicit node dependencies, declared signal names, symbolic `{ operation_key, version }` bindings, and optional typed data-edge declarations. Strict JSON parsing rejects unknown execution fields and unsupported format versions. The data graph does not serialize Rust closure bodies, type-erased pointer values, runtime capability handles, OS addresses or provider selections.
2. **Registration/binding:** an explicit, test-only **Rust-owned operation registry** maps symbolic keys and versions to callable `Operation` closures, trusted access declarations and typed port descriptors. Parsing a document **does not** authorize arbitrary code execution. Unknown keys or unsupported operation versions fail before invocation; a missing registration is never silently replaced with another provider. This registry is **not** a new Riesel provider resolver or public authorization mechanism.
3. **Validated execution:** a bound document creates the **same generic research runner's** `Plan` / `Run` used by ordinary Rust-authored nodes. No parallel execution logic is duplicated in a visual-document interpreter. Dependencies, triggers and registered resource conflicts continue through the existing generic runner. A plan's callback operations are never invoked merely to parse, inspect or bind it.
4. **Presentation metadata:** a declared `view` map preserves node placement and labels across the JSON round-trip. Unknown node placements fail. Such metadata has no executable authority. Its inclusion in the same `revision` is an intentionally visible design weakness: a view-only edit changes serialized bytes and collides with the same-revision ledger. A separately versioned layout sidecar may be preferable if visual inspection/authoring is accepted.

The test-only `RevisionLedger` compares **canonical serialized document bytes** for repeated `(plan_id,revision)` pairs within one in-process ledger. Idempotent reads succeed; changing a document at the same revision is rejected. **This does not prove a durable, signed, process-independent content identity, global revision allocator, schema migration, or remote authorization.** Different app processes can publish divergent documents if an authoritative publisher/installation identity and immutability rule are not defined (coordinate with #75).

### Verified positive and negative behavior

| Research question | Exact probe and result | Boundary not proven |
| --- | --- | --- |
| Can an authored Rust metadata value survive JSON without executing operations? | Programmatically authored v1 document encodes/decodes unchanged; registry callbacks are not called by inspection or binding | No `#[app]` macro generation, binary provenance or passive inspection of arbitrary app startup |
| Can a visual edit become an executable revision? | A v2 revision changes layout, adds a logical node pointing to an **already registered** Rust operation, encodes/decodes, binds and completes through the existing research `Run`. Previously bound v1 remains immutable | No arbitrary new Rust computation, persisted publication, code-generation feedback or safe hot-swap of a running plan |
| Are format and runtime bindings fail-closed? | Future document version, missing operation, mismatched operation version, duplicate node, dependency cycle, unknown signal, and unknown view node all reject before execution | Published compatibility/migration policy and operator authorization are absent |
| Can a graph express typed data edges safely? | Declared input/output **string-labeled** type equality, unknown ports and duplicate targets are validated; execution with any data edge explicitly returns `DataFlowExecutionUnsupported` | No actual typed value transfer, ownership/lifetime/affinity proof, output snapshots or cross-process serialization |
| Does an edit silently erase unknown executable semantics? | `deny_unknown_fields` rejects future execution fields rather than dropping them on round-trip | This strict approach can reject forward-compatible editors; explicit read-only/migration modes need a deliberate version policy |
| Do trace events identify the plan that produced them? | A further negative test constructs different document revisions with the same session/run/node/attempt/event keys; the existing research `Event` contains **no plan ID/revision** | Authoritative snapshot/event correlation and run-ID uniqueness across plan edits/sessions need a new contract; session-generation integers are not credentials |

### Equal-scope authoring alternatives

| Model | Merits | Risk against the *full* proposed product |
| --- | --- | --- |
| **Rust-only operation closures + live introspection** | Most expressive; inherits Rust types, borrows and domain ownership | A displayed graph is not automatically editable/serializable. Round-trip needs an explicitly supported declarative subset or code-generation policy |
| **Symbolic versioned document + explicit Rust binding registry (tested)** | Declarative headless authoring, offline metadata parsing, stable logical IDs, validation, controlled editor changes, one existing generic runner | Registry/format version maintenance; code/graph source-of-truth reconciliation; dataflow, stable published IDs, unknown adapters and affinity must be designed carefully |
| **Fully serializable arbitrary executable plan / universal bytecode** | Strong portability aspirations, one artifact | Cannot faithfully serialize arbitrary Rust closures, native borrows and foreign resources. A separate constrained DSL/VM and its security model would be a *different accepted product* |
| **UI graph editing that writes arbitrary Rust source** | Familiar code-first workflows if a constrained generator owns a code region | Not a faithful reverse compiler for arbitrary handwritten Rust/control flow; generated/handwritten ownership and merge conflicts need explicit rules |

**Interpretation:** This probe establishes the feasibility of one **registered-operation declarative subset**, not the compatibility of every existing Riesel application with external executable graph editing. It is sufficient to reject the assumption that all visual composition requires serializing Rust closures; it is **insufficient to accept visual authoring, its exact v1 JSON layout, or a new universal scheduler**.

### Remaining decision-critical engineering contracts

- **Plan identity and provenance:** distinguish logical plan ID, immutable semantic revision/fingerprint, author/publisher and installed artifact identity, current application session, execution run, node ID and per-attempt number. A test-only runner event with `session/run/node/attempt` but **no plan revision** can be ambiguous under ID reuse. Prefer a verifiable plan association on authoritative snapshots/events before promising cross-process inspection. A plan ID/version is not an authenticated peer identity.
- **Type and authority:** named port type strings and registry keys are illustrative; production type compatibility must derive from versioned Rust/SDK-defined contracts, and only trusted application ownership can register executable bindings and access/affinity facts. JSON view metadata must not weaken these facts. No second capability/provider resolver or separate application task owner.
- **Semantic revision vs editor revision:** choose whether visual layout is a separate, non-executable document and define how edit conflicts, stale source code, new code registrations and version migrations behave. If code is source-of-truth, an editor must not claim to modify uneditable handwritten Rust; if the manifest is source-of-truth, code bindings must be compiled/published against its accepted version.
- **Executable dataflow and native execution:** define typed ports/value lifetime, output materialization, transferability, resource conflicts and `!Send`/GPU/main-thread affinity, or explicitly restrict the product to control-flow relationships with domain-owned state. A local `Send + Sync` closure runner cannot prove these.
- **Live updates, control and failure:** a running plan revision must be immutable; changing graph topology while operations run requires a separately accepted stop/rebind/restart/reconcile contract. Existing #83 cancellation classification gap, external unknown mutation outcomes, and #75 authorization/readiness still apply. No arbitrary thread preemption or automatic replay is proven.
- **Static inspection purity:** this test calls Rust code to construct a trusted registry and performs metadata validation **without invoking registered operations**. Invoking an untrusted application executable to obtain that registry can execute arbitrary setup code; this is the separate inspection provenance hazard already demonstrated under #75.
- **Nested schedule policy:** A serializable outer graph does not settle who admits fixed simulation ticks, ECS internal systems or native event loop work. C and H remain viable; a common explicit plan representation is useful to both when each node has one authoritative scheduling owner. Do not force a specialized ECS scheduler to pretend its internal schedule is a directly editable outer graph.

**Gate G recommendation (NOT ACCEPTED):** A **versioned semantic plan description plus explicit registry-backed execution** is a credible long-term direction *if headless executable composition is accepted*, but needs a clean SDK-produced definition and a source-of-truth/version/publisher contract. Keep the view/composer and typed dataflow promises separate; implement a trustworthy editor only after the supported editable subset and value/identity semantics are accepted. Continue the C-versus-H ownership comparison using real mixed-domain admission and safe-point requirements rather than treating successful JSON round-trip as scheduler selection. Remain within #83 research authority: no production API, new provider resolver, implementation Issue, roadmap change or merge.


## 14. Gate H — competing tick admission and the shared mixed-domain execution boundary

**Scope and exact artifact:** `src/differential/admission_policy.rs` extends the **existing** actual, exact-pinned RunenECS differential test module; it does not create a second generic scheduler, reimplement ECS system scheduling, or alter production Riesel/RunenECS. The only variable is **who grants each domain simulation tick** and how the outer graph observes it. All three tests use one fixed input, real RunenECS on a dedicated owner thread holding a `!Send` resource, independent async branches with a barrier/join, one existing generic research graph runner, one simulated frame proposing four ticks with a catch-up limit of **two**, and bounded cooperative control.

### The three discriminating tests

| Dimension | Central outer admission (C) | Domain-owned tick admission (H) | Decision implication |
| --- | --- | --- | --- |
| **Tick identity and observability** | Each allowed tick is an explicit Riesel outer node; two completed tick events appear in the outer graph | Outer graph owns one `domain_frame` node; domain emits authoritative per-tick snapshots through its own observer | One inspection vocabulary is desirable, but nested tick and outer operation must remain distinguishable; an explicit adapter is needed for H's deep observations |
| **Same simulation/workflow result** | Fixed input, same two tick completions, the same asynchronous branch/join and final completion | Same authoritative ECS snapshots and completed asynchronous join | Neither policy wins on tested semantics alone; provider/ECS ownership remains domain-local |
| **Other async work while a tick is paused** | **Yes, if** the graph uses explicit per-tick cooperative admission permits while unrelated nodes remain eligible | **Yes,** the domain pauses tick permits while outer async nodes remain eligible | A global debugger budget of one graph node blocks unrelated operations by policy; it is not a fundamental limitation of centralized scheduling. Avoid using that biased comparison to choose H |
| **Cancellation after one committed tick** | Earlier node-level cancellation tests show a completed tick cannot be undone, and pending work may be cancelled; no production effect-outcome mapping accepted | The domain returns `ticks=[tick1], cancelled=true`, but this test's intentionally naïve adapter returns `Ok(())`, making outer `domain_frame` **Completed** | A *shared*, explicit partial/committed-prefix/cancelled outcome contract is required before adopting an adapter. Mapping the same state to plain `Failed` is not sufficient either |
| **Generation and control** | Current prototype `Tick` message has no generation field; the separate outer research live-control probe tests generation fencing | `SteppedFrame` permits reject stale generations on the real domain owner thread | Neither C nor H may infer authorization from a generation number; runtime-issued generation, trusted controller and revocation require #75-compatible contracts |
| **Time and affinity** | Outer caller admits two ticks for one synthetic frame proposal; RunenECS systems remain on their own invoker thread | The domain frame itself grants two ticks under identical proposal/cap | This proves *bounded logical tick admission*, not real wall-clock frame pacing, GPU/main-thread pinning or deadline scheduling |

**Independent observability is not yet unified.** The test records native `Run` events, domain tick snapshots and one async join counter; no production shared observer/snapshot schema reconciles these sources. A mutable-borrowed `Run::advance` still needs the separately tested live-control coordinator; the domain's `try_send` diagnostic stream may drop messages without losing authoritative tick results. The plan revision must be correlated with both streams (Gate G), and operation boundaries must not be confused with domain tick or system boundaries.

**Safety and bias limits:** The same asynchronous IO workload and ECS semantics are exercised under both policies, but there is **no cross-policy throughput/latency benchmark**, no real timer jitter or frame deadline, no adversarial external client, no strong OS-thread/main-thread proof and no remote durable execution. Both variants use the research-level `Send` graph plus one domain-owned thread; they do not demonstrate a universal `!Send` executor. C's cooperative gate uses a Tokio semaphore **for the fixture**, not as a suggested public scheduler mechanism. H's cancellation-to-success mapping is an explicitly demonstrated *defect in this test adapter*, not an inherent requirement of H.

**Architectural conclusion (recommendation, NOT accepted):** A **single validated semantic plan, one graph runner for Riesel-owned operations, and explicit ownership of each admission/safe-point boundary** is supported by tests. A specialized ECS/domain coordinator can retain its own nested tick/system authority, exposing supported deeper observations and controls through a deliberate adapter. C and H can share much of this implementation; they are policies about *who owns specific boundaries*, not necessarily mutually exclusive executables. Selecting a centrally admitted tick or domain-admitted frame should depend on required semantics, not on the label C/H or an unevidenced performance preference.

**Decision gate:** Headless composition (P1), authoritative cooperative inspection/control (P2), optional visual inspection (P3), and editable visual authoring (P4) still require explicit independent maintainer disposition. If P1/P2 are accepted, the missing **contract** to specify first is outcome/commit-prefix classification, plan/session/run correlation, which coordinator owns each safe point, and the native-affinity/authorization exclusions. Do not create an automatic second provider resolver/Manager, merge the research as production infrastructure, or silently turn unsupported typed dataflow into executable work. Treat further microbenchmarks as optional evidence only when they can change the actual decision.


## 15. Gate I — explicit terminal effects, plan/run correlation and nested control fencing (research)

**Scope:** `src/differential/outcome_contract.rs` tests a typed **boundary outcome**, session-local plan/run association and admission-owner/stepping granularity against the existing actual pinned RunenECS owner. This file is a research sketch, not a published Riesel API, accepted outcome enum, new plan registry, a second session owner or an additional scheduler. The executable fixture deliberately leaves `Run`'s existing `Result<(), &'static str>` invocation interface alone so that its limitations remain visible rather than being hidden by an unreviewed public change.

**CI status:** the first two attempts of [run 44887406345](https://github.com/aschenrot/riesel/actions/runs/44887406345) at code revision `70ecbde42ccdc4c236c7ab9626e226d1f7d63c46` failed **before runner execution, with no job steps or runner name**; the workflow run contains one annotation but the connected GitHub interface does not expose its content. This is **NOT a Rust test failure**, and it is also **NOT passing CI evidence**. The new tests must be verified on a later exact revision before this gate can be called executed/verified. Gates C–H retain their previously recorded exact-head green CI evidence.

### Distinguish terminal state from effects

A proposed boundary-level terminal reason cannot be flattened into `Ok` versus `Err`:

| Meaning | Terminal | Persisted evidence | Safe interpretation |
| --- | --- | --- | --- |
| All admitted work returned successfully | **Completed** | Authoritative owner confirms the entire admitted frame/tick result | Dependent success-only graph operations may advance |
| Cancellation at a supported cooperative safe point | **Cancelled** | Earlier ticks may have committed and remain visible | Do **not** roll back prior effects, claim entire success, or automatically replay the remainder |
| Explicit domain/operation failure | **Failed** with reason | May contain a confirmed prefix; error does not imply zero side effects | Preserve failure and committed-prefix separately; propagate dependency semantics |
| Lost response/transport uncertainty after dispatch | **Outcome unknown** | Only effects re-established by an independently authoritative snapshot can be counted confirmed | Do **not** assert not-executed, ordinary cancellation, a definite failure or exactly-once retry |

The bounded fixture creates actual RunenECS `SteppedFrame` results via **two legal domain permits** (completed) and **one permit followed by domain cancellation** (partially committed); it records completion from the authoritative reply rather than relying on a lossy diagnostic channel. A candidate mapping rejects an allegedly successful but under-completed frame, preserves the number of committed ticks and identifies the remaining unconfirmed prefix. A lost-reply constructor deliberately returns `OutcomeUnknown` without fabricating effect evidence. These semantics need a stronger production effect taxonomy if applied to arbitrary external/network operations or non-rollbackable databases.

**Observed preexisting issue:** current generic `Run` turns *any* operation-returned `Err` into `Failed`, while a partial domain cancel incorrectly turned into outer `Completed` when the adapter returned `Ok` (Gate H). The candidate `BoundaryOutcome` maps true cancellation to `Cancelled`, but its `OutcomeUnknown` intentionally cannot be represented by the existing research `Status`. This is evidence for a richer **authoritative terminal/outcome contract**, *not* an accepted expansion of `Status` or an assertion that an application-level sidecar alone would be sufficient.

### Correlation, stale-session fencing and nested authority

- A test-only **session-owned `RunLedger`** registers exactly one immutable logical-plan ID/revision for each `(session_generation,run_id)`. Registering the same run twice—**even after editing the plan**—fails. Legacy trace events with session/run/node/attempt/sequence can be associated with the pre-registered plan; unknown runs and old session-generation events fail rather than being assigned a guessed plan revision.
- A restarted session can legitimately reuse the numeric run ID without aliasing the old execution because its generation is part of the address. This does not establish a stable externally attested plan fingerprint, monotonic cross-process run allocator or durable trace log. If the ledger is lost, the old trace alone is still ambiguous; **self-describing plan association in the authoritative operation state/event contract is preferable**.
- A research **boundary authority** distinguishes outer-operation, domain-tick and ECS-system stepping. Under C, Riesel owns outer operations and explicit outer tick admission; under H, the outer node belongs to Riesel and nested ticks to the domain. Wrong-owner and stale-generation requests reject. No generic ECS-system debugger is silently promised. This check is *local routing logic*, not protection against a hostile external process.
- The actual controlled RunenECS thread retains all native `!Send` state, and these tests do not add a second ECS system scheduler. A service exposing controls externally needs **separate caller/server authorization and runtime-issued session generations** from #75. The existing same-UID/loopback observations under #75 prohibit treating a matching generation, a filesystem owner bit or a local socket as a trusted principal.

### Architecture and source-of-truth recommendation — NOT accepted

For an accepted P1/P2, favor one **authoritative execution state model** combining immutable plan association, session/run/attempt identity, boundary ownership and a typed final result with separately recorded *confirmed effects*. A lossy event stream is only a projection; consumers must be able to resnapshot authoritative state. Keep generic graph-node control distinct from domain-provided tick/system safe points. The same plan and boundaries should work with C-style outer admission or H-style domain admission; the choice is per declared operation boundary and must never create two independent dispatchers for the same work.

Do **not** adopt the sample `RunLedger` implementation, expose its integers as external credentials, or treat this fixture's `BoundaryOutcome` naming as a public API. The research cannot substitute for a trusted application-published registration/plan source, durable run history, a versioned transport contract, robust effect-idempotency semantics, real main-thread/GPU borrowing, fully typed dataflow or fault-injected multi-process recovery.

**Decision boundary:** Gates C–H already support a shared execution plan, reusable generic graph runner and explicit nested authority. This final targeted evidence addresses the concrete **misclassification/identity hazard**; completing it should move #83 into **explicit P1–P4 and mechanism acceptance review**, rather than expanding into indefinite platform experiments or silently entering implementation. #75/#59/#60/#68 remain their own accepted-scope product research. No production/roadmap/merge action follows from passing a research fixture.
