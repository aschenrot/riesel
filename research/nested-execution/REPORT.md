# Riesel #146 — domain-owned nested execution: comparative research

**Status:** Research/specification evidence only. Not an accepted product extension or runtime/SDK interface.  
**Baseline:** `main` `29c90f5285f99022badac4af50aa3e57f719d3cc` (22 November 2026).  
**Scope:** [#146](https://github.com/aschenrot/riesel/issues/146); distinct from completed [#112](https://github.com/aschenrot/riesel/issues/112) / [#113](https://github.com/aschenrot/riesel/issues/113) and independent inspector [#114](https://github.com/aschenrot/riesel/issues/114).  
**Verified baseline:** [#83 research PR #84](https://github.com/aschenrot/riesel/pull/84), especially `research/execution-scheduler/REPORT.md` §§14–15, including real pinned RunenECS. Research structs in either workspace are **not** public API proposals approved by the owner.

## 1. Original outcome and scope — decide whether this extension is a product

The accepted Riesel developer outcome is headless executable plans, authoritative live state, owner-confirmed effects and cooperative control for **advertised supported boundaries**. A domain coordinator may own an internal tick, frame or system schedule. An optional future graphical inspector must never manufacture internal execution events merely because an outer Riesel frame operation exists.

The #146 question is *additional*: should a Riesel application developer be able to opt into **shared nested observation**, and possibly owner-mediated safe-point requests, for delegated domain schedulers with very different internal semantics?

The useful growth horizon includes (a) engine/ECS internal frame, tick and system operations with thread-affine/non-`Send` worlds; (b) non-game nested workflow stages, for example a sequential data-import or media-processing pipeline; (c) future independent inspection and versioned capability exports. Neither ECS nor a graphical client should be required for applications that do not opt in.

Two independently useful outcomes must not be conflated:

- **Nested observation:** a trusted reader can associate domain-authoritative snapshots with the correct outer graph run, operation and attempt, detect lost notifications, and see true partial/uncertain effects. This is useful even if inner control is unsupported.
- **Nested control:** a trusted caller may *request* one domain-advertised safe point through the **domain owner**; Riesel may route the request but cannot independently grant an inner permit or suspend native code.

An outer-only inspector already works at the actual supported granularity. Therefore #146 should not retroactively recast completed #113 outer controls as incomplete or mandatory for ordinary Rust authors.

## 2. Audited baseline and negative evidence

| Integrated owner | Existing evidence | Strict limit |
| --- | --- | --- |
| Outer immutable graph plan/binding and admission | `crates/riesel_app/src/execution_plan.rs`, `execution_runner.rs`; #110/#112 | Declared domain labels are trusted metadata, not enforcement on opaque Rust closures |
| Authoritative run identity and current state | `execution_observation.rs`, #137 | One graph operation per delegated frame, not an inner system/tick tree |
| Owner-confirmed positive prefixes | `GraphEffectRecorder::confirm_prefix`, #141 | Scoped to the **running outer attempt**. Cloned reporters reject late reports; an unknown reply is **not** confirmed |
| Safe outer step | `GraphRunController`, #144 | `RieselDomainTicks` rejected for a `DelegatedFrame`. Already-running nested work is never preempted |
| Thread-confined domain simulation | `crates/riesel/tests/execution_domains.rs`, #128/#135/#136/#144 | One actor owns `Rc<Cell<_>>`; its real committed world can be finer-grained than its outer graph terminal report |
| Real RunenECS differential | #84 `REPORT.md` §§14–15; #146 adds the **same pinned RunenECS source** invoked from a real `RunningApp::execution_graph` delegated frame ([Linux CI evidence](https://github.com/aschenrot/riesel/actions/runs/44976980523)) | Actual ECS ownership remains on a dedicated `!Send` invoker thread; these are research-only read-only projections, **not an integrated public nested observation contract** |
| External interface attachment | #60, #75, #114 | Open research/authorization gate: a graph generation, same UID, TCP loopback or locally known port is not an authenticated principal |

A decisive negative case: a domain-owned frame may finish two actual mutations after the outer callback loses its response. Its **authoritative domain** may know the two commits, while the outer graph must retain `OutcomeUnknown` with **no inferred confirmed-prefix increase**. A future reconciliation facility must establish which authority can attest past effects after the attempt recorder is inactive; it cannot simply resynthesize a missing event or retry the command. A mutable domain object leaking to the inspector also violates affinity/ownership even when a graph metadata type is `Send`.

## 3. Equal-criteria architecture alternatives (including capability composition)

| Alternative | Reuse and fidelity | Control and ownership | Cost / reversibility | Main gap |
| --- | --- | --- | --- | --- |
| **A — Outer only** | Current complete outer graph; domain tools stay separate | Safest minimal ownership; no nested control claim | Lowest immediate cost, cheap rollback; many independent debugger integrations later | A general inspector cannot show true nested progress without product-specific logic |
| **B — Optional nested *read-only* domain projection** | One correlated snapshot/event vocabulary for ECS and non-ECS; no Riesel child scheduling | Internal state and event order originate with owner; no generic control channel | Moderate schema, bounds and snapshot-version work; easy to adopt incrementally | Cannot offer shared inner step UX; owner-specific debugging still needed |
| **C — Optional typed observation + owner-routed control** | Extends B with explicitly advertised safe points/capabilities; strongest future inspector semantics | Domain owner performs *actual* admission and validates generation/epoch/target | Higher adapter/version/security cost; staged adoption if B contract is stable | Authorization, unknown control acknowledgement and variable system-level safety require real owner adapters |
| **D — Model inner work as registered child plans/runs** | Reuses graph validation/execution and existing visual semantics where inner work is genuinely registered | Needs explicit parent/child plan identity, shared/split authority and coordinated lifecycle | Larger migration and execution fanout risk; potentially appropriate for schedulable workflow subgraphs | Treating native ECS inner systems as separately Riesel-admitted operations would duplicate domain authority |
| **E — Riesel centrally schedules all inner work** | One inspector/scheduler vocabulary possible for specially built or Riesel-native domains | Requires domain executors/affinity and deliberate transfer of ownership; must not pretend a generic `Send` runner can move a `!Send` world | Largest runtime migration and semantic coupling, but shared infrastructure may pay off for domains designed for it | Not justified for existing independent ECS owners by the actual mixed-workload outcome; cannot infer it impossible for future domain-specific integrations |
| **F — Reuse versioned domain-specific capability contracts + inspector plugins** | Leverages Riesel's existing typed capability vocabulary for each domain's distinct snapshot and safe-point meaning, with an optional minimal parent-correlation envelope | Domain/app deliberately exposes a capability; GUI plugin interprets it; no universal inner scheduler or forced normalized payload | Less universal schema migration; repeated UI adapters and discovery/version contracts, and optional plugins may become fragmented | A generic inspector cannot render arbitrary unknown domain-specific state without a supported common envelope and explicit presentation/compatibility rules |

**Same criteria applied:** original multi-domain functionality, plan/run/attempt identity, truthful inner snapshots, partial/unknown mutation semantics, safe-point ownership and unavailable-step errors, affinity, bounded event loss, latency/overhead, ergonomic opt-in, crash/restart behavior, trust profile, native/browser future interface, migration and backwards compatibility.

F is a serious comparator because Riesel already has generated, versioned capability contracts and one provider-selection mechanism ([provider catalog](../../docs/product/riesel/provider-catalog.md)); a domain-specific *application-facing* inspector capability might avoid freezing a universal domain schema. **However**, the catalog is not an independent app export/authorization layer. [#75's operational investigation](../../docs/product/riesel/application-operational-boundary-investigation.md) and [#60's native-host investigation](../../docs/product/riesel/native-window-host-investigation.md) explicitly leave approved app-facing publication, trust and external version negotiation open. F cannot bypass that gate. B and F could usefully combine: a minimal shared owner/parent/snapshot/loss envelope with typed, versioned domain-specific payloads and optional presentation plugins.

No architectural choice is inferred from source line counts or one microbenchmark. In particular, C is **not** selected because it is “more generic,” and A is **not** selected because it is already shipped. D/E are credible for a new domain whose scheduling is genuinely Riesel-owned, not a drop-in claim over an unrelated ECS internal scheduler.

**External semantic comparators checked on 2026-11-22:** [Bevy ECS 0.20.1 `Stepping`](https://docs.rs/bevy_ecs/0.20.1/bevy_ecs/schedule/struct.Stepping.html) advertises feature-gated schedule/system stepping, breakpoints and cursors **under Bevy's own scheduler**, which makes domain-owner-routed C plausible for an ECS integration; it is not proof that Riesel can independently step an opaque domain's systems. [OpenTelemetry trace SDK](https://opentelemetry.io/docs/specs/otel/trace/sdk/) supports contextual spans but permits sampling and dropped events, reinforcing that trace correlation cannot replace owner-authoritative state. [Tokio task cancellation](https://docs.rs/tokio/1.54.1/tokio/task/index.html#cancellation) is cooperative and `abort` does not synchronously prove cancellation or undo non-async/remote effects. These are API documentation checks, not Riesel runtime measurements.

## 4. Explicit authority and identity matrix

| Datum/action | Issuer and lifetime | Required claim / failure mode |
| --- | --- | --- |
| Application session generation | Existing Riesel runtime, new per application session | Local correlation only; old session's controls and handles reject. Not a remote credential |
| Logical outer plan ID and immutable revision | Validated application authoring/registry | Graph run is bound to exactly one plan revision |
| Graph run sequence and outer operation attempt | Existing Riesel runner/publication | Cannot alias a different session/plan/attempt |
| Delegated domain **instance generation** | Actual domain owner | Invalidate old inner requests and snapshot attachments after domain restart, rebind or migration |
| Inner tick/stage/system identities | **Domain owner**, optionally projected through a versioned descriptor | Never guessed from graph node names, counters or SDK-created UI labels; availability may change by domain |
| Inner snapshot revision, high-water mark and trace sequence | Domain owner | One consistent authority for each domain instance. Trace loss leads to resnapshot, not invented replay |
| Inner pause/step/resume | Domain owner after validating a **trusted** request and its exact domain epoch/capability | Reject stale, unsupported, duplicate, wrong-owner and post-terminal attempts. Riesel's outer controller cannot independently grant these |
| Outer operation terminal and confirmed prefixes | Existing graph observer / attempt-scoped callback recorder | Never conflate `Completed` with verified external mutation or alter old confirmed effects after recorder expiry without a distinct accepted reconciliation rule |
| Independent UI/Manager principal | Still unresolved #75/#60 | Read vs control capabilities separated; a session ID, PID, OS UID or plan ID is not authentication |

An optional future nested projection could identify a parent by the **full** outer identity and attempt plus a domain-issued instance epoch and a domain-issued inner key. It should explicitly advertise whether each inner boundary is *observational only* or supports a particular safe point. Parent and nested sequences are independent; there is **no global total order** implied.

The demonstration uses this correlation vocabulary only inside a trusted research fixture. It does not accept a public serialization ABI, actor protocol, trust profile, version negotiation or mutation ledger.

## 5. Reproducible isolated experiment

`research/nested-execution/` is an independent Rust workspace using the real production `riesel` SDK, Tokio 1.54.1 and a **research-only, exact pinned** `runen-ecs` Git revision `5d6af86bfa7c58f1ad26ec73635590a72e89ef37` (the same revision verified in #84). A separate `Cargo.lock` retains the resolved research dependency graph. `--locked` must fail rather than silently resolve different indirect packages. No extra dependency enters the normal Riesel workspace. No new provider, Riesel application daemon, ECS adapter, scheduling crate, public SDK API, window host, or production trace format is introduced.

Run on Rust 1.100.0:

```sh
cargo validate --check
cargo smoke
cargo fmt --manifest-path research/nested-execution/Cargo.toml -- --check
cargo clippy --locked --manifest-path research/nested-execution/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path research/nested-execution/Cargo.toml -- --nocapture
```

The #146 draft branch's CI executes the final three checks **only** on that research PR. Both normal workspace validation/smoke and isolated research checks are mandatory to call a revision verified; their execution order in CI does not weaken either gate. This deliberate isolation follows the #83 precedent. The isolated lockfile is committed, including the exact pinned RunenECS Git revision and resolved transitive crates; use `--locked` to avoid dependency drift.

Experiment uses a **real** `RunningApp::execution_graph` run and `DelegatedFrame` plan; a bounded synchronous command channel to a dedicated native owner thread, real Tokio callbacks/response loss and the existing effect recorder; a separate domain-owned copied read-only state and four-event lossy ring. The baseline fixture owns `Rc<Cell<i32>>`; the added real-RunenECS variant constructs and runs an actual `World`, `Runtime`, `Schedule`, mutating position system, `TickCount` and `Rc<Cell<_>>` resource **inside the same native owner thread**. It never transfers the `!Send` world or a borrowed resource to a graph Tokio task. A non-ECS pipeline uses the same parent/projection vocabulary with different stages and effects. No new ECS system scheduler is created.

| Probe | Verifiable assertions | Exclusions |
| --- | --- | --- |
| Modeled delegated frame, one committed tick then cancellation | Independent async graph node advances; wrong domain control and stale owner permit rejected; domain snapshot sees first commit before outer terminal; outer returns `Cancelled`, dependent `Blocked`, positive prefix 1; no synthetic graph tick node | This baseline world is a small `Rc<Cell>` actor; additional actual RunenECS cases exercise the same boundary |
| **Actual pinned RunenECS** frame after one tick then cancellation | The true ECS schedule updates an owned Position, count and invoker-thread-only resource; an unsupported operation kind is rejected; the domain snapshot confirms exactly one tick, outer frame remains `Cancelled` with prefix 1 and dependent `Blocked` | This does not prove generic RunenECS system-level pause/step or multi-threaded parallel performance |
| **Actual pinned RunenECS** two ticks with lost reply | The owner world commits two schedule invocations while the callback gets no reply; outer graph stays `OutcomeUnknown` and records **no guessed prefix**, while the domain snapshot retains the true world and independent sequence | Owner snapshot after outer terminal is not a new retroactive effect confirmation or a durable reconciliation protocol |
| Lost reply after two committed ticks | Owner snapshot reports two; graph returns `OutcomeUnknown`, no confirmed prefix in outer report, no success-only dependent and no retry | This does not provide an accepted later reconciliation path or independent remote identity attestation |
| Non-ECS 12-stage processing | Same parent correlation/snapshot representation; independent branch completes; ring retains only four of 12 events; `truncated=true`, authoritative snapshot still proves 12 stages | A modeled batch coordinator, not a production media/data pipeline throughput benchmark |
| Multiple delegated frames of one actual run | Two real sequential `DelegatedFrame` graph operations use the same domain actor, distinct parent operation IDs and per-attempt committed prefixes. Re-dispatching the same already-admitted parent attempt is rejected before mutation | The research actor's duplicate set is in-memory only, **not durable exactly-once** or external authorization |
| Stale application/session run | Two real Riesel sessions produce different issued generations; owner rejects a frame stamped with the other run before mutating its world | A local in-process consistency test, **not** hostile same-UID caller authentication |
| **Domain-owner replacement without app-session restart** | A replacement native owner issues a new internal epoch; an old permit is rejected **without an inner commit**, even though its outer plan/run/session identity is unchanged; a current-epoch cancellation releases the operation | Does **not** establish recoverable in-flight attempts or safe replay after owner restart. A locally held duplicate set is lost when the old owner disappears; production needs separate reconciliation/fencing |

**Exact integration evidence (22 November 2026):** [Actions 44976980523](https://github.com/aschenrot/riesel/actions/runs/44976980523) on tested source revision `6f8bdcf92cc9902f7d4408b586aca124b3c32452` passed Rust 1.100.0 research rustfmt, `cargo clippy --locked --all-targets -- -D warnings`, **8/8 locked research tests** (including both real pinned RunenECS cases, two-frame owner correlation and owner-epoch replacement), normal `cargo validate --check` and `cargo smoke`. Windows was skipped. The research remains an **isolated, intentionally unmerged specification artifact**, not a verified shipping API or the correctness of arbitrary external domain implementations. Any later source/report revision requires its own exact-head CI verification before calling that later revision tested.

## 6. Trade-offs, critical bias audit and evolution

**Short-term bias:** Staying at A prevents a shared graphical inspector from making useful *true* inner observations when the domain already has those snapshots. Asking each engine and pipeline to independently invent session correlation, event loss, effect taxonomy and stale-boundary handling risks incompatible tooling and repetitive migration. Explicit opt-in B can address that without letting Riesel schedule the owner’s work.

**Speculative abstraction bias:** “Nested graph” does not automatically mean arbitrary typed dataflow, recursive `ExecutionPlan`, fully serializable domain executor, cross-process exporter or generic `StepSystem`. C can be materially dangerous when it presents controls before confirming owner capability and active safe points. A common vocabulary is not itself sufficient proof that different systems have the same safe-point semantics; reject unsupported capabilities rather than giving every node a pause button.

**Durability and extension design:**
1. Keep the existing `GraphRunObserver` and `GraphRunController` contract intact as the **outer** authoritative view. An optional nested attachment should be a separately versioned, owner-provided **read-only** source with an explicit parent address and its own domain instance epoch.
2. Reuse the source-of-truth pattern, **not necessarily the same mutex, event sequence or run allocator**: domain owner publishes snapshot and a bounded loss-aware event stream; the graph/inspector can compose a view but does not own the domain state.
3. If inner control is later justified, add an optional, typed *capability advertisement* and owner-routed control adapter only for verified safe points. A viewer may display unavailable/observation-only inner work. Do not automatically translate an outer `step` into an inner tick.
4. Decide external attachment/authorization in #75/#60, versioned nested schema and error taxonomy, **separately from** local research. Reject stale grants after restart, late results and ambiguous commands; no automatic retry.
5. Distinguish outer graph effect prefixes, domain-authoritative internal committed state and a separate future reconciliation protocol after an outer attempt ends. Never derive internal effects from trace completeness, graph `Completed`, or generated child labels.
6. Preserve a migration path to D where a *genuinely Riesel-authored nested plan* uses the existing runner, while native/domain-owned internals stay in B/C. No forcing every app through one ECS or scheduler.

**Unmeasured:** domain snapshot construction cost/lock contention under realistic ECS sizes, native GUI rendering overhead, remote session attach/detach/resync, Windows/macOS native thread behavior, effect-ledger durability, cryptographically authenticated owner claims and bounded resource upper limits beyond this fixture. The research actor takes a short lock after each inner step; a production adapter could instead publish at owner-selected boundaries or sample/copy immutable snapshots. Do not extrapolate event-per-system overhead or real-time scheduling guarantees from this probe. The event ring cap is intentionally illustrative (four), not a recommended public default. Run targeted benchmarks or protocol research **only if** those uncertainties change the acceptance of B or C.

## 7. Research-only recommendation for #146 owner disposition

**Provisional scope recommendation: ACCEPT optional nested *observation* through a small shared owner/parent correlation and loss-aware snapshot envelope (B), with **domain-specific versioned payloads and optional inspector plugins** evaluated as a first-class compatible route (F). Keep nested *control* as an optional, owner-routed later extension (C) with its own explicitly supported safe points. Do not expand the core outer graph scheduler (E) or reinterpret domain-owned internals as Riesel child plans (D) by default.** Preserve A as the supported fallback for domains that do not opt in.

This is a **recommendation, not a maintainer decision or implementation authorization**. The test-only model is not an API choice; no new product definition or contract edits are made by this research PR. An adoption decision still needs the accountable maintainer’s explicit accept/revise/defer/reject, after reviewing the actual test evidence, alternative trade-offs and the stated limitations.

If B/F is accepted, the first bounded implementation should be a *read-only, owner-published nested snapshot/projection contract* with a stable correlation envelope, explicit capability/unsupported metadata and bounded resync, verified with both real RunenECS and one non-game adapter. Decide whether domain-specific payloads use existing typed capability contracts, an opt-in typed library, or narrowly versioned inspector plugins **before** standardizing serialization. Its output must remain consistent with existing #113 outer reports on cancelled/unknown dispatch and must not grant cross-process control. C, authenticated export and a generic child-run orchestration policy must not be smuggled into that first slice.

## 8. Open questions for acceptance

- Is optional nested observation a shared Riesel outcome needed by multiple products, or should per-domain tooling suffice for #114’s first graphical inspector? P3 can already render genuine outer state without nested ECS events.
- Who authenticates a domain owner’s claims once an independent process publishes nested state, and what is the expected revocation/attestation contract with #75? In-process signed/declared parent identity and owner epochs are *local correlation and cooperative checks*, not cryptographic principal verification or persistent replay barriers.
- Does B need one typed common read-only projection, or should Riesel transport only opaque owner-provided data and let registered inspector plugins render it? Which evolves better across domain schema versions and optional layout/data fields?
- How much nested history should be retained by the owner, and what exact read consistency guarantees are required across the graph snapshot and domain snapshot **without** a global clock or transaction?
- Which internal safe points, if any, may be called through a shared C controller? The actor’s native thread/system schedule must stay authoritative.
- What is the product-approved disposition for *post-attempt domain-confirmed effects* after an outer graph operation returned `OutcomeUnknown`? There is currently no accepted graph-report mutation/reconciliation authority.

The workflow requires an explicit disposition on these material product/contract questions before a production implementation Issue and PR. Green isolated research CI is **necessary evidence, not automatic architecture acceptance**.
