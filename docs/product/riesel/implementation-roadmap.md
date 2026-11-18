# Riesel — Implementation Roadmap

Status reviewed: 2026-11-18
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
- `ComponentDefinition` produces local and managed hosting from a single
  validated capability declaration. Counter's observer and writer share
  the same state in either execution mode; Storage uses the same abstraction.
- Todo and Counter each use one deployment-aware application entrypoint;
  the Hello example avoids hand-authored JSON. Counter's fault-injection
  executables now live under test fixtures, and its integration tests are
  split out of application logic.
- Todo uses `Record<TaskData>` instead of copying infrastructure fields into
  a duplicate domain type. Model-to-Storage requirement bindings are retained
  in exported application metadata and honored by typed record lookup.
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

`Application::new("Todo").records::<TaskData>()?.development()?`
registers the model and versioned Storage requirement and selects the
ephemeral in-process MemoryStore. `Context::records::<TaskData>()` gives a
typed CRUD handle. `--print-definition` exports the canonical generated
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
`apps/examples/counter_rpc` application tests generated clients and
dispatchers, commands, queries, borrowed string inputs, domain errors,
incorrect arguments, wrong capability versions, and real TCP round-trips.
The same application exercises a typed local provider and the runtime-managed
Counter executable through `RpcConnector<CounterContract>`. Its server uses
the shared `run_tcp_rpc_host` helper.

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
Components remain separate from contracts in the **flat** repository structure:
`packages/storage/contract` is the API contract; `packages/storage/memory` is an
implementation. The optional development SDK convenience may include the
reference implementation but does not change that contract boundary.

## Deferred until justified

GraphQL, SQL query capabilities, a custom UI framework, game-specific systems,
distributed orchestration, and the visual application builder are not
prerequisites of the current application runtime.
