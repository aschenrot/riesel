# Riesel — Implementation Roadmap

Status reviewed: 2026-11-19
Working branch: `feature/schema_based_app`

This tracks **tested implementation status**, not all long-term product requirements.
See [Product Definition](riesel.md) for the intended product scope.

## Validation gates

For code milestones, Linux CI runs automatically; Windows validation is
available as a manual workflow dispatch and is required before releases:

```sh
cargo validate --check
cargo smoke
```

`cargo validate --check` checks committed formatting, all workspace targets,
Clippy (warnings denied), tests, and documentation. `cargo smoke` executes
the actual Todo and Storage binaries, including runtime-owned Storage processes,
then verifies a failed second managed service launch cleans up the first one,
and that the Storage binary cooperatively exits on a bounded private stdin
shutdown request. The suite also starts the independent Counter component
locally, as a runtime-managed RPC provider, and with a deterministic real
process crash followed by safe application-session restart using generated
contract and client bindings.
A separate smoke check runs the `--inspect-deployment`
CLI with a nonexistent service executable and validates the generated plan
without invoking that executable. Workspace tests also exercise a real child exiting during
application execution, cancellation delivery, bounded retry exhaustion,
preservation of concurrent business errors, task failure and panic
propagation, task joins, crash detection during task joins, normal shutdown
without false crash reports, and healthy-sibling cleanup.

## Milestone status

| Stage | State | Evidence |
| --- | --- | --- |
| 0 — Rust foundations | Initial implementation | Cargo workspace, model and capability macros, Storage contract |
| 1 — Declarative requirements | Implemented | Versioned requirements; Rust-generated application definitions |
| 2 — Provider resolution | Implemented for exact versions | Registration, explicit selection, missing/ambiguous-provider errors |
| 3 — Capability binding | Implemented reference slice | Unified component definition for local and managed execution; typed generated RPC; verified TCP |
| 4 — Application runtime | Cooperative lifecycle and bounded whole-session recovery implemented; production hardening remains | Managed startup, DAG ordering, rollback, shutdown, supervision, task joins, restart policies/backoff, failure-injection smoke |
| 5 — Developer experience | Generic RPC, deployment inspection and second provider integration implemented; still partial | Rust SDK, models, generated Storage/Counter RPC, shared RPC host, local/managed Counter provider, preflight and JSON startup plans |
| 6 — Production reliability | Not implemented | Persistent Storage, secure transports, authorization, recovery, observability |

Completed stages mean *working within their currently specified scope*, not
production-grade infrastructure.

## Accepted high-level API — staged execution plan

The accepted authoring direction is documented in
[High-level API architecture v3](api-authoring-v3.md), with the
[async execution contract](async-execution-contract.md) defining the next
runtime implementation gates. This is a **target API**;
examples in that document do not imply macros or async handles already exist.

| Step | Scope | Status | Validation / exit gate |
| --- | --- | --- | --- |
| 5.1 | Identity and injection semantics, API contract, compatibility and lifecycle rules | Complete on Linux | Contract recorded; deterministic private ID and stable external Counter/Storage IDs verified |
| 5.2 | Type-driven dependency extraction, aliases, record-model bindings | Complete on Linux | Function handlers register/extract typed dependencies; Counter (local/managed), Todo, alias/error and schema-equivalence tests pass |
| 5.3 | Generated local/managed component adapters | Complete on Linux | `#[component]` generates registration; Counter/Observer share state locally and managed; custom factories, private IDs and missing-impl compile-fail test validated |
| 5.4 | Async transport, generated handles, cancellation and error propagation | Complete — Linux validation and managed-host lifecycle verified | Generated native async local/managed bindings, real nonblocking startup, bounded async and blocking tasks, native crash/recovery, provider-owned blocking scope with cooperative shutdown and joins; Windows manual before release |
| 5.5 | Deterministic provider catalog and deployment configuration | Complete on Linux — final CI verified | Linked/local defaults, explicit managed and loopback external placements, selection-only manifests, exact provider identity metadata, side-effect-free inspection, lifecycle/recovery and CLI conflict tests; Windows manual before release |
| 5.6 | `#[riesel::app]` and ergonomic model/Records injection | Complete on Linux — macro, Counter, Todo and catalog integration verified | Minimal native-async Counter apps run linked, managed, aliased and catalog-selected; typed Todo Records and builder-equivalent schemas/plans verified; negative startup/CLI and macro validation tests pass |
| 5.7 | Optional modules and lifecycle handlers | In progress — module registration slice verified on Linux | Typed reusable modules register into one application schema; module and provider conflicts, capability dependency order, and macro composition tested; lifecycle handlers, cancellation and bounded shutdown still required |
| 5.8 | DX documentation and regression suite | Not started | Compile-fail diagnostics, inspection, Linux validate and smoke; Windows manual before release |

**Verified progress (2026-11-18):** `#[capability]` now permits ID-free
private contracts with deterministic declaration-path identities and default
version 1, while previously published explicit IDs remain unchanged. SDK
`ApplicationParam` provides one shared declaration/extraction contract for
owned `Capability<C>` handles and `Records<T>` handles. Requirement aliases
derive from stable contract identities rather than Rust variable names; tests
cover case-sensitive private IDs, collision-resistant alias escaping, local
binding, typed Records and stale-session rejection. CI run
[44411860685](https://github.com/aschenrot/riesel/actions/runs/44411860685)
passed `cargo validate --check` and `cargo smoke` on Linux. **This is not yet
function-signature injection or the high-level app macro.**

**Stage 5.2 verified (2026-11-18):** `Application::handler` and
`handler_with_aliases` now accept ordinary synchronous Rust functions. They
register typed requirements from parameter types, validate alias collisions
before startup, and extract fresh owned values for each session. Zero- through
four-parameter handlers are supported. Two distinct `Records<T>` models can
share one Storage alias; repeated identical bindings are rejected unless
given separate explicit aliases. Counter and Todo have been migrated to typed
handlers; the existing managed Counter and Storage smoke scenarios now execute
these handlers without explicit `Context` lookups. CI run
[44414421066](https://github.com/aschenrot/riesel/actions/runs/44414421066)
passed Linux validation and executable smoke tests.

**Stage 5.3 verified (2026-11-18):** Generated capability adapters now
provide the typed local trait binding and managed RPC dispatcher. The
`#[riesel::component(provides = [...])]` attribute builds the existing
`ComponentDefinition` and supports a private auto-derived ID, an explicit
published ID, and optional non-`Default` factories. `Application::component::<C>()`
registers a linked implementation; Counter now uses this path without
manual dispatcher creation or trait-object conversion. Counter and Observer
retain one shared state in local and managed executions. Tests cover
invalid component declarations, custom factories, generated identities and
a missing capability implementation compile-fail case. CI run
[44416458416](https://github.com/aschenrot/riesel/actions/runs/44416458416)
passed Linux validation and executable smoke tests.

**Stage 5.4 client slice verified (2026-11-19):** The protocol now exposes
`AsyncRpcTransport` and `AsyncRpcClientBinding`; the capability generator
emits a typed `<Trait>AsyncClient` alongside existing synchronous bindings.
The optional `async-tcp` protocol feature implements real nonblocking Tokio
TCP using the same bounded wire protocol and published contract identities.
Its session-bound calls reject expired sessions, distinguish domain from
transport failures, support cancellation by dropping futures, validate
remote contracts and advertised provider metadata, cap response frames, and
bound concurrent outbound calls to 32 per transport. No command is
automatically retried. The `Cargo.lock` was generated by Cargo and committed.
CI run [44420615980](https://github.com/aschenrot/riesel/actions/runs/44420615980)
passed Linux workspace validation (including async tests) and existing
executable smoke tests.

**Stage 5.4 native contract and host slice verified (2026-11-19):**
`#[capability]` now supports native `async fn` trait declarations via a
`Send` future contract and generated object-safe async service adapter,
`<Trait>LocalClient` and `<Trait>AsyncRpcDispatcher`. Mixed synchronous and
asynchronous methods in one contract are rejected with a diagnostic.
An optional `async-tcp` Tokio server exposes bounded, loopback-only
`handle_async_tcp_connection` and `serve_async_tcp_rpc_host` primitives
with one-request framing, admission limited to 32 connections and cancellation
of owned request tasks at shutdown. Contract tests execute a generated native
async Counter client against the generated async dispatcher over **real
nonblocking TCP**, including domain errors and borrowed arguments.
CI run [44423972358](https://github.com/aschenrot/riesel/actions/runs/44423972358)
passed Linux validation and managed-process smoke. Existing Counter/Storage
wire IDs and synchronous APIs remain unchanged.

**Stage 5.4 typed async application slice verified (2026-11-19):**
`SessionToken::cancelled()` now wakes pending Tokio RPC calls immediately,
including requests stalled on the outbound concurrency limiter. The existing
managed-service monitor revokes that token on child failure, while normal
shutdown revokes it before joining tasks. Rust SDK `AsyncCapability<C>` and
`Application::async_rpc::<C>()` reuse the existing requirement, provider,
deployment and session resolver, with explicit transport-mode collision
checks and rejection of incompatible local bindings in preflight.
`Application::async_handler(...)` and
`async_handler_with_aliases(...)` now run typed `async move` Rust functions
in a Tokio executor **owned per application session**, without caller-supplied
`block_on`. Pending handler futures are cancelled upon session expiry and
freshly invoked after opt-in whole-session recovery.
Counter tests verify typed remote async calls through real TCP, structured
errors, provider metadata and stale-handle rejection. Deterministic tests
cover session races, cancellation of pending I/O and application entrypoints.
CI run [44429849161](https://github.com/aschenrot/riesel/actions/runs/44429849161)
passed Linux workspace validation and managed-process smoke tests.

**Stage 5.4 async router and managed-host protocol verified (2026-11-19):**
`AsyncRpcRouter` now multiplexes native async capability dispatchers through
one provider endpoint with exact contract selection, route preflight and
duplicate/unadvertised-capability rejection. The async host validates its
routes before admission. `run_async_tcp_rpc_host` adds the existing
`RIESEL_READY` readiness frame and private `RIESEL_SHUTDOWN` control signal
without duplicating the process manager. A bounded subprocess regression
starts the host with an OS-assigned loopback port, verifies two interfaces
sharing one provider-owned state instance, and checks cooperative exit.
CI run [44433596866](https://github.com/aschenrot/riesel/actions/runs/44433596866)
passed Linux workspace validation and executable smoke tests.

**Stage 5.4 generated native async component parity verified (2026-11-19):**
`#[component(provides = [...])]` now generates typed local and hosted
bindings for native async capability traits from the same component declaration.
The native async `<Trait>Handle` is injected through `AsyncCapability<C>`
in both modes: linked providers invoke the underlying async implementation
directly (without JSON serialization), and managed providers use nonblocking
Tokio TCP. Both receive the same application-session invalidation semantics.
`ComponentDefinition::serve` chooses the native async host for async-only
components and shares one instance across multiple provided capabilities.
Runtime preflight rejects mismatched sync/async local providers before
constructing state; startup checks factory binding modes again.
A real managed Counter/Observer process and its linked counterpart exercise
the same typed handler, shared mutable state, domain error and stale-handle
rejection. Mixed sync/async single-endpoint hosting is rejected before the
state factory runs. CI run
[44439704442](https://github.com/aschenrot/riesel/actions/runs/44439704442)
passed Linux workspace validation and executable smoke checks.

**Stage 5.4 structured async tasks and native managed recovery verified (2026-11-19):**
`AsyncTasks` is a cloneable, bounded (64 active tasks) session-owned
spawner injected as a typed async handler parameter, without a synthetic
capability requirement. Tasks run in the application's Tokio executor,
preserve unique names during admission, cancel and join before providers
are torn down, and propagate errors/panics through the existing
`AppRunError::TaskFailure` path. Finished task names/budget slots are
recycled. Stale clones cannot spawn after scope exit. The synchronous
OS-thread task group remains supported, with unchanged minimal SDK behavior.

Native async Counter integration verifies locally and with a real
managed component that background work shares provider handles and
pending futures are dropped on session exit. A fault-injection managed
async child crashes after readiness; its failed session cancels pending
work, expires old typed handles, and recovers through the existing
**opt-in whole-application** restart policy. Recovery now correctly treats
supervisor-induced `RpcError::SessionExpired` as eligible cancellation
while still rejecting arbitrary business failures; commands are never
automatically retried. CI run
[44443292891](https://github.com/aschenrot/riesel/actions/runs/44443292891)
passed workspace validation and executable smoke checks.

**Stage 5.4 asynchronous startup and structured blocking work verified
(2026-11-19):** Native async connectors inspect the exact published
contract and provider metadata through nonblocking Tokio TCP rather
than synchronous `TcpRpcTransport`. The new `App::start_async` shares one
`StartupState` implementation with synchronous `App::start` for dependency
ordering, local instance ownership, managed instance reuse, contract checks,
failure rollback and session/monitor construction. Managed process startup
and bounded readiness waiting use an explicit Tokio blocking worker.
`PreparedAsyncApplication::run` and `run_from_env` reuse the existing
resolver, strict CLI and opt-in recovery while driving this async startup
on a runtime-owned executor. Legacy sync-only connectors are rejected
**before** any provider state or managed child is created; a connector
must explicitly advertise and implement nonblocking startup. An async
startup regression verifies yielding while metadata arrives slowly.

Application-owned blocking work now has an explicit
`AsyncTasks::spawn_blocking(name, |cancellation| ...)` API with a separate
limit of eight active OS workers, bounded admission, cooperative cancellation,
joining before provider teardown and named failure/panic propagation.
Local and managed native async Counter tests cover the boundary. This
does **not** automatically convert arbitrary blocking calls inside native
async component implementations into managed work. CI run
[44487397907](https://github.com/aschenrot/riesel/actions/runs/44487397907)
passed `cargo validate --check` and `cargo smoke` on Linux.

**Stage 5.4 provider-owned blocking work verified (2026-11-19):**
`ProviderBlocking` is an opt-in resource injected through
`#[component(factory_with_blocking = create_component)]`. Existing
zero-argument constructors are unchanged. The resource follows the
same component state in local and managed modes and admits at most
eight named, synchronous OS-thread jobs per component instance.
`ProviderCancellation` provides explicit cooperative shutdown; work
is rejected after the session closes. Dropping or timing out an RPC
future does **not** detach its already-running blocking job: the
provider owner retains and joins every worker before state teardown.
Panic diagnostics and bounded-admission failures remain observable.
Concurrent shutdown callers wait for the same completed joins.
Synchronous capability bindings are rejected early if configured with
a provider blocking factory.

Real Counter integration covers identical local and managed native async
operations without blocking the Tokio reactor. A separate managed-host
fixture starts an actual blocking RPC worker, interrupts the request
through private-stdin shutdown and verifies the worker finished before
the child process exited. CI run
[44493550783](https://github.com/aschenrot/riesel/actions/runs/44493550783)
passed `cargo validate --check` and `cargo smoke` on Linux.

**Stage 5.4 completion boundary:** All planned Stage 5.4 acceptance
requirements are implemented and Linux-verified. Blocking Rust code that
ignores cooperative cancellation is **not** forcibly preemptible:
local shutdown must wait for it, and the managed parent retains its
configured process-kill fallback. Production-wide budgets, isolation and
scheduling belong to Stage 6+. Windows verification remains manual
before release. Stage 5.5 (deterministic provider catalogs) is next;
the final `#[riesel::app] async fn` macro belongs to Stage 5.6.
See [async execution contract](async-execution-contract.md).

**Stage 5.5 first slice verified (2026-11-19):** `ProviderCatalog`
introduces explicit, named managed and external loopback placements that
compile into the existing `DeploymentConfig` and `DeploymentPlan` rather
than introducing a second provider resolver. Statically linked local
components remain the only implicit selection when unambiguous. Managed
and remote catalog placements require explicit selection; unknown choices,
missing capabilities, ambiguous local providers, invalid published provider
definitions, and unauthenticated non-loopback TCP are rejected. Catalog
placements pin the expected `ProviderDefinition`; the runtime rejects a
mismatched live provider before use. The Rust-first SDK exposes
`.catalog(...)`, `.select_provider(requirement, choice)`, and
`.catalog_plan()`. Real Counter integration covers linked state, one
manager-owned process shared by Counter and Observer, operator-owned TCP
providers, identity mismatch and ambiguity. This verified slice was
delivered in larger coherent GitHub commits using atomic multi-file trees.
CI run [44500272772](https://github.com/aschenrot/riesel/actions/runs/44500272772)
passed `cargo validate --check` and `cargo smoke` on Linux at branch
commit `bb41adfc`.

**Stage 5.5 completed and verified (2026-11-19):** Strict
`ProviderSelectionConfig` manifests select only placement names authorized by
the Rust `ProviderCatalog`, never arbitrary executables, addresses or ports.
`Application::provider_selections` and CLI `--select-providers FILE` retain
opt-in whole-session recovery. `--inspect-catalog` exposes startup order,
selected targets and exact pinned provider metadata without starting a process.
Provider metadata equality ignores capability declaration order but not
versions or published identity. CLI inspection is order-independent and rejects
conflicting or repeated configuration even for empty manifests. Real local,
managed-shared, external loopback, identity mismatch and configuration error
tests pass alongside existing lifecycle smoke. CI
[44503685582](https://github.com/aschenrot/riesel/actions/runs/44503685582)
passed `cargo validate --check` and `cargo smoke` on Linux for HEAD
`aac5f63`. Windows CI remains optional during development and required
before release.

**Security boundary:** A provider's advertised definition is metadata
validation, not cryptographic proof of peer identity. Unauthenticated TCP
deployment is restricted to loopback. Authenticated remote access,
authorization and stronger isolation remain Stage 6 work.

**Next:** Stage 5.6 will implement the accepted compact `#[riesel::app]`
function API by generating calls into the existing typed handler and provider
registry. See [provider catalog](provider-catalog.md) for the supported
selection and inspection workflow.

**Stage 5.6 completed and Linux-verified (2026-11-19):** The real
`#[riesel::app]` procedural attribute generates the same typed
`Application::async_handler`, linked `.component`, approved `.catalog`
and `.run_from_env` flow as the explicit builder. User entrypoints accept
generated native async `<Trait>Handle` values directly; a small private
`NativeHandleParam<T>` adaptation avoids Rust trait-coherence overlap with
existing `ApplicationParam` implementations. `Records<T>`, `AsyncTasks`,
`Capability<C>`, and `AsyncCapability<C>` remain available in the same
handler model, with `#[riesel(parameter)]` for aliased parameter types.
The macro supports explicit dependency aliases, optional linked components,
an optional approved catalog factory, application names, and explicit
`development` storage. It never guesses remote providers or enables
development storage implicitly.

Counter regression binaries verify linked and manager-owned execution from
one high-level source, two aliased handles sharing provider state, a
remote-only entrypoint rejecting missing deployment, exact macro-vs-builder
schema/plan equality, and selection-only catalog planning and managed
execution. The Todo binary verifies one `Records<TaskData>` declaration
derives the same model and Storage requirements as the builder. Syntax tests
reject duplicate component/alias declarations, unsupported function shapes,
missing return types and excess parameters.

Linux CI
[44510693321](https://github.com/aschenrot/riesel/actions/runs/44510693321)
passed `cargo validate --check` and `cargo smoke`. Windows remains
manual before release. The accepted minimal API is now implemented for its
defined one-shot async entrypoint scope; optional lifecycle handlers and
application modules remain Stage 5.7 work. See
[high-level app authoring](high-level-app.md).

**Stage 5.7 module composition verified (2026-11-19):**
`ApplicationModule` groups ordinary registration steps in reusable Rust
types. `Application::module::<M>()`, the typed prepared-handler
`.module::<M>()` methods and optional
`#[riesel::app(modules = [M1, M2])]` route every component and requirement
through the existing application schema/registries, without inventing a
module resolver or process. Module identity derives from its Rust type path
and duplicates are rejected; conflicting component registrations still
fail in the existing provider registry. A Counter integration registers
dependency requirements and a linked provider in deliberately different
module order, verifies one capability dependency DAG, runs the generated
high-level application and confirms duplicate-module/provider errors
before startup. CI
[44512577572](https://github.com/aschenrot/riesel/actions/runs/44512577572)
passed `cargo validate --check` and `cargo smoke` on Linux.

**Stage 5.7 remaining acceptance gates:** Ordered startup/shutdown hooks,
cooperative cancellation, preserved error provenance, recovery-session
re-entry and bounded shutdown must be designed using the existing
`RunningApp`, `TaskGroup` and `AsyncTasks` owner lifetimes. Module
registration is not its own lifecycle, scheduler or runtime. Do not
invent detached hook tasks or run blocking user code on Tokio. See
[application modules](application-modules.md).

**Sequencing:** 5.1 is a gate for 5.2–5.8. Component adapters can follow
parallel to dependency extraction after their shared contract is settled;
async support must not be faked with blocking calls. Implementation may make
aggressive breaking changes but must preserve verified lifecycle and transport
behavior through regression tests.

**Identity policy:** Private capabilities/components may derive deterministic
internal identity from their Rust declaration path. Published independently
built contracts need one stable external identity (explicitly or from trusted
package metadata) and an independently versioned contract. Renaming a Rust
path is intentionally an identity change for private declarations, not a safe
migration for published APIs. Provider instance names and requirement aliases
remain distinct from contract identities. Nothing is discovered magically:
local providers must be linked/registered, and managed binaries must be
configured or registered in a trusted catalog.

**Stage 6+ product requirements remain open:** persistent Storage and schema
migration; authenticated/authorized remote transports; resource budgets,
observability and durable recovery; manager/discovery and CLI parity; separate
native window hosts and synchronization; optional Bevy ECS/game networking;
and eventual visual composition. Completing Stage 5 does **not** imply
production-ready deployment.

## Architecture refactoring checkpoint

Before advancing the roadmap, the Rust implementation was consolidated and
verified using Linux `cargo validate --check` and `cargo smoke`:

- Connector registration uses exact capability ID/version keys. One provider
  instance can expose multiple distinct capability bindings.
- Typed `local_provider`, `LocalComponentFactory` and SDK `provide`
  remove repeated Storage/Counter local factory wiring.
- Generated client bindings carry revocable session tokens; calls from
  escaped local or remote handles fail after shutdown and restart.
  `RpcClientBinding` now requires explicit `guard_local` implementation,
  with no silently unguarded default. Compile-fail tests protect this rule.
- Runtime registry, planning, startup, handshake, running session, errors
  and tests are separate modules. SDK builder, CLI and Context; macro
  capability argument parsing, signature checking, expansion and model/type
  derives; and RPC client/wire/server/hosting follow the same responsibility
  split.
- The `#[capability]` macro generates `Arc<T>` trait forwarding.
  Derive validation rejects unsupported Serde field/variant/container
  modifiers rather than generating inaccurate contract metadata.
- The loopback RPC host uses four workers and a bounded 32-socket pending
  queue; slow and fast calls are tested concurrently. Runtime application
  workers have a cap of 64 simultaneously active threads; completed workers
  are joined and their names and capacity reused.
- `RieselModel` supports numeric, optional, list and JSON fields, including
  typed Storage round-trip tests.
- Obsolete handwritten-protocol/fixed-port tests were removed.
  `cargo ship` preserves deliberate staging instead of `git add .`.
  Linux CI runs on main and the working branch; Windows remains an
  explicit on-demand pre-release check.
- Regression gates include multiple versions of one capability in the same
  registry, multiple local bindings sharing one provider instance, rejection
  of incomplete bindings, stale-handle rejection after recovery, schema
  validation, bounded RPC concurrency, compiler-negative contract cases,
  and executable managed-process failure injection.

**Breaking change:** Counter now advertises version **2** because it
returns a typed internal failure instead of panicking on a poisoned lock.

**Boundaries:** Runtime-owned tasks remain cooperative and unbounded joins
can block indefinitely; RPC remains unauthenticated and loopback-only;
in-flight calls are not forcibly interrupted; manually written custom
connectors are responsible for their own session enforcement; provider
state remains non-durable. These issues belong to later production
hardening, not this refactoring checkpoint.

## Second refactoring pass — complete on Linux

This stage corrects cross-deployment component ownership rather than creating
additional crates or compatibility shims:

- A `RpcRouter` serves multiple capability contracts over one managed port.
  Counter and Observer now use the same in-memory state in both local and
  explicitly grouped managed deployments. Named managed instances launch
  once; conflicting definitions fail during preflight.
- `InstanceIdentity` distinguishes the actual local, managed or external
  endpoint from the provider implementation ID in resolved bindings.
- Generated `RpcClientBinding` accepts a transport-neutral
  `Arc<dyn RpcTransport>`. Validated host contracts are cached; application
  TCP sessions reuse a bounded pool of sockets without automatic command
  retries. Standalone one-shot clients remain supported.
- Models and capability contracts share one `TypeRef` type algebra,
  including `i64` and `f64`. Model JSON now uses tagged type values:
  `{"kind":"string"}`, `{"kind":"optional","item":{"kind":"u64"}}`.
  Old schema serialization is intentionally incompatible.
- Development MemoryStore is an opt-in Cargo feature of the SDK, enabled
  explicitly by Todo. Typed `records_at` registration and Context lookup
  support named Storage requirements.
- Provider definitions for Storage and Counter derive from their runtime
  registration; the same provider metadata powers local and managed hosting.
- `cargo ship` rejects unstaged or untracked inputs before validating the
  exact source staged for commit. Smoke child invocations have bounded waits,
  and minimal-feature SDK builds are checked independently.
- Application task failures preserve their concrete error sources for
  inspection instead of reducing all errors to strings.

The pass is complete on Linux, verified by `cargo validate --check` and
`cargo smoke`. Regression coverage includes explicit instance conflicts,
shared Counter state across local and real managed RPC,
transport-independent generated bindings, pooled TCP session reuse,
pipelined request framing without dropping buffered bytes, typed model
round trips, cooperative lifecycle recovery and executable smoke tests.
Windows CI remains on-demand and must be exercised before release.

**Remaining production boundaries:** A cooperative Rust task may never
return, and Rust threads cannot be forcibly terminated safely. The
reference Storage backend is ephemeral and does not independently enforce
all application-defined model schemas for arbitrary RPC callers. TCP is
still plaintext, unauthenticated and loopback-only. Per-instance rolling
restart, persistent state recovery, transport security, configurable executor
budgets, and distributed networking are future milestones.

## Application/component developer-experience refactor

The third refactoring pass reorganizes products by ownership while keeping
Cargo package names stable:

- Framework infrastructure stays under `crates/`; Counter's contract,
  component and consuming app now live together under `examples/counter/`.
  Todo and the Rust-first Hello example live under `examples/`.
- The standard Storage contract and reference MemoryStore are grouped under
  `packages/storage/` rather than appearing as core engine crates.
  Typed Records support is now behind the optional `riesel/records` feature;
  `dev-memory-storage` enables it plus a schema-aware ephemeral provider.
  Minimal SDK, Records-only, and full development feature configurations
  are tested separately.
- `ComponentDefinition` produces local and managed hosting from a single
  validated capability declaration. Counter's observer and writer share
  the same state in either execution mode; Storage uses the same abstraction.
- Todo and Counter each use one deployment-aware application entrypoint;
  the Hello example avoids hand-authored JSON. Counter's fault-injection
  executables now live under test fixtures, and its integration tests are
  split out of application logic.
- Todo uses `Record<TaskData>` instead of copying infrastructure fields into
  a duplicate domain type. `Records::update_with` avoids manual revision
  plumbing without retrying concurrent writes or replaying callbacks.
  Model-to-Storage requirement bindings are retained in exported application metadata and honored by typed record lookup.
- `run_from_env_with_args` composes infrastructure options with
  application-specific tokens; strict `run_from_env` preserves early rejection
  of unknown arguments.
- The Storage capability advances to **version 2**: bounded cursor pages,
  revision-checked delete, typed `Records<T>::list_page` and `delete`, and
  a 64 KiB reference-provider record bound. The typed `list` convenience
  method gathers pages for small datasets.
- The in-process development Storage provider is initialized with registered
  Rust model definitions and validates direct writes. Standalone managed
  MemoryStore remains schema-unaware until deployment supplies trusted model
  metadata. This is not a production authorization or persistence solution.
- Network tests use bounded lifecycle fixtures rather than fragile exact
  connection counts; page boundaries and revision conflicts are exercised
  in backend and Todo tests.

The Storage v2 contract intentionally breaks version 1 clients; no
compatibility adapter is maintained. Windows remains on-demand and is
required before a release.

## Recommended Rust authoring workflow

`examples/todo/src/task.rs` defines a Rust `TaskData` model, annotated
with `#[derive(RieselModel)]` and `#[riesel(model = "task")]`.
There is no hand-maintained `examples/todo/app.json`.

`Application::new("Todo").handler(run_demo)?.development()?`
derives the model and versioned Storage requirement from the
`Records<TaskData>` handler parameter and selects the ephemeral in-process
MemoryStore. The low-level `Context::records::<TaskData>()` remains available
for programmatic composition. `--print-definition` exports the canonical generated
schema; `--deployment FILE` allows explicit external or managed providers.

`Application::rpc::<SomeContract>("alias")` declares a generated RPC
capability and automatically registers a shared transport connector.
`Context::capability::<SomeContract>("alias")` returns the typed service.
Implementations opt in using `#[capability(..., client_trait)]` and define an
explicit conversion from `RpcError` into their public error type.
`Application::provide::<Contract, _>(id, factory)` can register a local
implementation without manually implementing the provider-factory trait.
Custom providers can still be registered explicitly. A general provider catalog and
automatic deployment discovery are not implemented.

`development()` is an **explicit opt-in** to ephemeral in-memory storage
that validates registered Rust model shapes; it is not a production default
or persistence guarantee. The generated schema records each model's
selected Storage requirement alias so `Context::records::<T>()` does not
guess among unrelated aliases.

Applications with their own CLI arguments can use
`Application::run_from_env_with_args` or `run_with_args`; Riesel reserves
deployment/inspection flags while forwarding the remaining application tokens
(and all tokens following `--`). Unknown flags remain errors under the strict
`run_from_env` variant.

## Runtime-managed service modes

- `local`: in-process implementation, no socket or process required.
- `tcp`: explicit remote address, owned by an external manager.
- `managed`: starts a trusted process that binds `127.0.0.1:0` and sends
  a bounded `RIESEL_READY` announcement through its startup pipe. Riesel
  verifies its advertised provider ID and generated capability contract.

The fixed-port `managed_tcp` mode, its pre-bind race, and the earlier
`managed_tcp_dynamic` naming have been removed. The smoke suite runs two
concurrent independently managed Storage instances with OS-assigned ports.
The local factory is covered by unit tests.
After readiness, the runtime continues draining child stdout.

Managed children that advertise `"shutdown":"stdin_line"` in their
`RIESEL_READY` frame receive a private `RIESEL_SHUTDOWN` command on piped stdin.
The process registry waits up to the deployment's `shutdown_timeout_ms`
(default 2,000 ms, maximum 120,000 ms) for an ordinary exit, then force-kills
and reaps an unresponsive child. Providers without the opt-in retain immediate
forceful cleanup. Storage implements the cooperative path, which is validated
by process tests and the executable smoke suite on Linux and Windows.
This mechanism is a local lifecycle channel, **not transport authentication**.

Applications can declare dependencies between requirement **aliases** using
`depends_on`. Riesel rejects missing, self-referential, duplicated, and cyclic
dependencies before launching providers. Startup follows a stable topological
order; independent requirements preserve declaration order. Managed children
are terminated in reverse **actual startup** order, including on partial
startup failure. The rollback smoke test confirms that a previously connected
Storage server no longer listens after a dependent launch fails. This is
reverse-order teardown with a bounded graceful opt-in. An explicitly opted-in
application can now recover by reconstructing **all** providers and typed
handles after complete session cleanup, rather than blindly restarting one
child.

### Managed service supervision

When a running application owns managed child processes, one monitor thread
checks their exit state independently of application work. The default
required-service policy is **cancel-and-fail**: the first unexpected exit
(including a clean exit code) is retained, signals a shared
`CancellationToken`, and is exposed by `RunningApp::check_processes()`.
The Rust SDK exposes the same token through `Context::cancellation_token()`.
Callers can poll `is_cancelled()`, wait using `wait_cancelled_timeout()`, or
request cooperative cancellation explicitly using `cancel()`.

During application shutdown, the runtime cancels and joins registered
application-owned tasks **while child supervision remains active**.
`RunningApp::spawn_task` and `Context::spawn_task` register named tasks;
their errors and panics cancel the shared token, with
`AppRunError::TaskFailure` preserving entrypoint and shutdown context.
The monitor's final exit-status snapshot occurs **after** task joins but
**before** intentional process termination. `RunningApp` also cleans up
on drop. This closes the gap where a provider could crash while application
tasks were still finishing.

**Limits:** entrypoints and worker threads remain synchronous and cannot be
forcibly interrupted. Non-cooperative tasks may prevent shutdown and
recovery from finishing. External TCP providers are not process-supervised,
and production-grade health metrics are not yet provided.

For example, in a Rust-authored application:

```rust
let app = Application::new("Example")
    .rpc::<RecordStoreContract>("frontend")?
    .rpc::<RecordStoreContract>("storage")?
    .depends_on("frontend", ["storage"])?;
```

The emitted schema carries `"depends_on": ["storage"]` on the `frontend`
requirement. All referenced aliases must exist in the final definition.
Dependencies express lifecycle ordering; they do not infer a component's
internal dependencies or discover providers automatically.

Deployment configuration remains optional for local development but is
necessary when choosing external or managed process execution.

### Side-effect-free provider preflight

`App::preflight(&deployment, &connectors)` validates all declared
requirements and the complete selected dependency order before starting any
components. It catches unknown local providers, missing adapters, wrong
local contract versions, missing endpoints, invalid managed timeouts and
cyclic dependencies. It does not call local factories or connect to TCP
services. `App::start` now executes the preflighted plan rather than
recomputing startup order; a late configuration error is reported before an
earlier managed process is spawned.

`Application::deployment_plan(&deployment)` exposes the same inspection to
Rust SDK applications, and `--inspect-deployment FILE` emits a machine-readable
JSON plan without executing application logic. The output includes ordered
requirement aliases, capability IDs/versions and selected local, TCP or
managed targets, including managed process arguments and explicit instance
identities.

Provider identity and remote contract claims **cannot** be established by
a no-I/O preflight. They are verified at runtime after connecting. Preflight
does not authenticate remote services or prove a managed binary is trustworthy.

### Recovery policy and safety boundary

The deployment can opt in to bounded **whole-application restart**:

```json
"recovery": {
  "mode": "restart_application",
  "max_restarts": 3,
  "initial_backoff_ms": 100,
  "max_backoff_ms": 2000
}
```

The default policy is `{"mode":"fail_fast"}`. Recovery configuration is
validated before any provider starts: 1–32 restarts, positive initial and
maximum backoff, each <=120,000 ms. The delay doubles up to the configured
maximum. When retries are exhausted, `AppRunError::RecoveryExhausted`
preserves the last supervision failure.

Recovery is attempted **only** after an unexpected required managed child
exit, when the prior entrypoint and full shutdown succeeded. Business errors,
worker task failures, startup failures, registry failures, and any shutdown
failure are not retried. Each new attempt reconstructs all application
providers and client handles from scratch, respecting dependency startup
and reverse shutdown order. Rust SDK callbacks are `FnMut` to support
repeated invocation; callers must make their work safe to replay.

Generated TCP client handles retain their original endpoint but are scoped
to a revocable application session; calls through escaped handles fail
after revocation. Individual provider restart without rebuilding clients
would still be unsafe. Per-service
transparent rebinding, selective dependent restart, persistent state
rehydration, untrusted remote transport and production observability are
**not** implemented. Memory-only providers lose state across attempts.
This bounded whole-session policy is not a generic exactly-once execution
guarantee.

## Generated capability RPC — implemented reference slice

The `#[capability]` macro now generates, from one annotated Rust trait:

- `NameContract` — versioned inspectable contract metadata
- `NameRpcClient<T: RpcTransport>` — typed RPC methods
- `NameRpcDispatcher<T: Name + Send + Sync>` — argument validation and
  implementation dispatch

`crates/riesel_protocol` provides reusable in-process `LocalRpc` and
bounded loopback-only `TcpRpcTransport`, including contract inspection and
structural shape verification. Transport failures are represented separately
from domain errors through `CallError<E>`.

The Counter capability contract (version 2) lives in
`examples/counter/contract`, with
its independent in-memory implementation in `examples/counter/component`. The
`examples/counter/app` application tests generated clients and
dispatchers, commands, queries, borrowed string inputs, domain errors,
incorrect arguments, wrong capability versions, and real TCP round-trips.
The same application exercises a typed local provider and the runtime-managed
Counter executable through `RpcConnector<CounterContract>`. Its managed host
uses `ComponentDefinition::serve` and the shared RPC router infrastructure.

Storage and Counter both register their capabilities once using
`ComponentDefinition`, which supplies local factory creation and managed
`serve` routing. Their executable entrypoints use
`run_component_from_env` for CLI parsing and lifecycle control.
Readiness, private shutdown, loopback restrictions, bounded TCP request
workers and accept loops are no longer duplicated in each component. Neither capability has a handwritten request
enum or per-operation RPC dispatcher.

**Storage RPC migration completed:** `RecordStoreRpcClient` and
`RecordStoreRpcDispatcher` are generated from the Rust trait. The macro's
opt-in `client_trait` mode generates the original capability-trait
implementation, with an explicit `From<RpcError>` conversion. Storage uses
`StoreError::Transport` to distinguish transport from domain failures.
`RpcConnector<C>` provides generic remote binding, while
`RpcHost<H>` advertises and validates provider identity. The handwritten
Storage request/response enums, `TcpStore`, manual dispatcher, and
Storage-only connector are deleted. The old wire protocol is intentionally
incompatible with the new version and has **no compatibility shim**.

The first generator supports synchronous, non-generic methods with owned
serializable arguments or borrowed `&str` and serde-compatible result types.
Generated inherent client methods still offer `CallError<E>` for callers
that need the original separated domain/transport error representation.
Additional client transports, asynchronous methods and backend-neutral
provider discovery remain future work.

## Remaining engineering work, in priority order

1. **Runtime lifecycle and recovery (implemented within cooperative whole-session scope):**
   dependency validation, reverse teardown, crash monitoring, task cancellation
   and joins, configurable bounded restart/backoff, fresh dependency-ordered
   sessions, retry safety rules, and real-process failure-injection are tested.
   Remaining for production reliability: selective dependency-subgraph restart
   with safely rebindable clients, stateful recovery contracts, health metrics,
   bounded handling of noncooperative tasks, and external-provider monitoring.
2. **Developer tooling and models:** provider preflight, dependency ordering,
   JSON inspection, a second runtime-integrated Counter provider, generic
   component registration and bounded reusable RPC hosting are implemented.
   Numeric, optional, list and JSON record fields are supported. Remaining:
   schema evolution/migrations, nested named models, additional transports
   and broader contract shapes.
3. **Production operation:** persistent Storage, migration strategy, secure
   authenticated transports, authorization and secrets, resource limits,
   observability, repeatable deployments, and recovery policies.

## Architectural and security boundaries

The present Storage server is **in-memory**, **single-process** and communicates
using **unencrypted and unauthenticated TCP**. A pipe-based startup announcement
and contract-shape check do **not** authenticate a network service.

Managed service endpoints are restricted to loopback. The built-in Storage
server and generated RPC client also enforce loopback-only plaintext TCP.
Only execute trusted deployment files and binaries. Do not treat the provider
advertisement as cryptographic authentication.
Components remain separate from contracts in the **grouped package** layout:
`packages/storage/contract` is the API contract; `packages/storage/memory` is an
implementation. The optional development SDK convenience may include the
reference implementation but does not change that contract boundary.

## Deferred until justified

GraphQL, SQL query capabilities, a custom UI framework, game-specific systems,
distributed orchestration, and the visual application builder are not
prerequisites of the current application runtime.
