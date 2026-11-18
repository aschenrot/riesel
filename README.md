# Riesel

Riesel is an experimental Rust-first, schema-driven application runtime.
Applications declare versioned capabilities and bind them to local, external,
or runtime-managed providers. The runtime handles provider validation,
generated typed RPC, managed child startup, and dependency-ordered teardown
with optional bounded cooperative shutdown. Both Storage and Counter use
the same generic RPC host; neither component implements its own wire protocol
or process lifecycle loop.

**Status:** reference applications work; managed children have continuous
supervision, application-owned tasks have cooperative cancellation and join
semantics, and opt-in bounded **whole-application-session recovery** is
implemented. Per-provider live rebinding, production persistence,
authenticated remote transport and UI tooling are not implemented.
The reference Storage and Counter implementations are in-memory.

## Run and validate

The workspace is pinned to Rust 1.100.0. From the repository root:

```sh
cargo run -p todo --bin todo
cargo run -p todo --bin todo_client -- --print-definition
cargo validate --check
cargo smoke
```

The Todo example derives its model directly from Rust and opts in to an
ephemeral local MemoryStore for development. The smoke suite exercises
managed TCP Storage, multi-service cleanup and real Counter crash recovery.
The framework's development Storage feature is disabled by default.
The Todo example explicitly opts into `dev-memory-storage`, and validation
also tests `riesel` independently with `--no-default-features`.
Linux CI runs on every push and pull request; run the optional Windows job
manually using Actions > Validate > Run workflow (enable Windows) before
releases.

## Capabilities, components and runtime infrastructure

- `crates/riesel_storage` and `crates/riesel_counter` define the versioned
  Storage and Counter **capability contracts**.
- `components/storage` and `components/counter` implement those capabilities,
  providing local factories and executable RPC hosts.
- `crates/riesel_protocol` provides the generated RPC transport and shared
  managed TCP host; `crates/riesel_app` and `crates/riesel_manager` handle
  application execution, supervision and child process ownership.

The developer supplies a component implementation, not a handwritten server
loop for each capability. See [Counter example](apps/examples/counter_rpc/README.md)
for local and managed execution using the same contract.

## Component registration and refactoring

Riesel's runtime, SDK, macro generators, protocol, model schema and smoke
tests now have separate internal modules for distinct responsibilities.
Connectors are indexed by **capability ID plus version**; one local provider
instance can implement several versioned capabilities using shared state.

For a single local capability, applications can avoid handwritten
`LocalProviderFactory`, contract definitions and `ConnectedProvider` wiring:

```rust
Application::new("Counter")
    .rpc::<CounterContract>("counter")?
    .provide::<CounterContract, _>("riesel.counter.memory", || {
        std::sync::Arc::new(MemoryCounter::default())
            as std::sync::Arc<dyn Counter + Send + Sync>
    })?
    .run(|ctx| {
        ctx.capability::<CounterContract>("counter")?.add(1)?;
        Ok(())
    })
```

The code assumes the relevant `riesel`, `riesel_counter` and
`riesel_counter_component` imports. Lower-level components use
`local_provider::<C, F>` or `LocalComponentFactory::new(...).exposes::<C>()`
for multiple bindings. Generated capability traits automatically forward
methods through `Arc<T>`. Storage uses `serve_component`, Counter
uses `serve_router` for multiple contracts, and both use
`run_component_from_env` rather than duplicating process CLI and service
startup code.

The loopback TCP server serves requests through **four worker threads** with
a maximum **32 pending connections**. It has bounded JSON framing and socket
timeouts; busy connections may be rejected. Application-owned TCP transport
sessions reuse at most four idle sockets (750 ms maximum idle age); standalone
`TcpRpcTransport::new` clients still use one connection per call. Failed
requests are never replayed automatically because a command might already
have executed. These are reference defaults, not configurable production
resource policies.

`RpcClientBinding` now accepts `Arc<dyn RpcTransport>`; typed services can
bind to an in-memory or future transport without depending on TCP. Hosted
contracts are validated at registration and cached for request handling.

`RieselModel` supports `bool`, `String`, `Uuid`, `u64`, `i64`, `f64`,
JSON values, `Option<T>` and `Vec<T>`, with recursive value validation.
Model fields and RPC signatures now share the `TypeRef` description; model
schema JSON uses tagged field values such as `{"kind":"string"}` rather than
the previous bare `"string"`. Existing generated schemas must be regenerated.
Unsupported Serde renaming and field/variant modifiers are rejected by
the derive macros to avoid inconsistent generated contract metadata.
Compile-fail documentation tests verify those restrictions, invalid async
capability signatures and the required session guarding of custom
`RpcClientBinding` implementations.

### Shared component instances

Local and managed components can both expose several capability interfaces
backed by the same state. `Counter` exposes a write interface
(`CounterContract`) and a read-only interface (`CounterObserverContract`)
from one shared `MemoryCounter`.

For managed deployment, use the same explicit `instance` name and identical
launch settings for each requirement:

```json
{
  "services": {
    "counter": {
      "transport": "managed",
      "instance": "counter_shared",
      "program": "/absolute/path/to/counter_service"
    },
    "observer": {
      "transport": "managed",
      "instance": "counter_shared",
      "program": "/absolute/path/to/counter_service"
    }
  }
}
```

The runtime launches **one** process for `counter_shared` and resolves
both capabilities over its endpoint. Omitted instance names continue to
create independent managed processes. Conflicting launch settings are
rejected before process startup. `RunningApp::resolved()` distinguishes
the provider implementation ID from concrete `InstanceIdentity`.

### Capability handle lifetimes

Each application session has a revocable token. Generated local trait proxies
and remote TCP capability handles check the token before each call. When a
session closes, even an `Arc` previously cloned by application code rejects
new calls rather than using an obsolete endpoint. Recovery creates new
session-bound handles. This is **not** transparent reconnection: manual or
custom connectors still require their own lifetime handling, and an in-flight
call cannot be forcibly cancelled by revocation.

## Defining lifecycle dependencies

Use `Application::records_at::<Model>("storage_alias")` and
`Context::records_at::<Model>("storage_alias")` to select among multiple
Storage requirements explicitly. `records::<Model>()` retains the single
Storage convenience path and rejects ambiguous storage selections.

Call `Application::rpc::<Contract>("alias")` to declare a generated RPC
capability. Once an alias is declared, use
`Application::depends_on("consumer", ["storage"])` to require `storage`
to become ready before `consumer` starts. Dependencies are validated for
missing aliases and cycles. They govern startup and reverse teardown order,
not provider discovery or selective provider replacement. A recovering
application restarts its entire dependency-ordered session.

## Inspecting deployment plans

You can validate every requirement, local provider selection, registered
connector contract, and dependency ordering **without starting any services**:

```sh
cargo run -p todo --bin todo_client -- --inspect-deployment path/to/deployment.json
```

The output is a JSON startup plan, with dependency-ordered requirement aliases
and selected local, TCP, or managed targets. The same preflight runs
automatically before `App::start` creates its first component, so invalid
late bindings do not leave earlier services running. Low-level callers can
use `App::preflight(&deployment, &connectors)`; the Rust SDK provides
`Application::deployment_plan(&deployment)`.

Preflight checks **local definitions only**. It cannot prove a remote
service exists, verify the server's advertised contract, authenticate a
provider, inspect a binary's behavior, or guarantee that a process will
become ready. Those checks still occur at actual startup.

## Runtime lifecycle and recovery

One runtime-owned monitor watches the exit state of required managed
children. An unexpected exit, including a successful exit code, cancels the
session's shared `CancellationToken`. Both
`RunningApp::cancellation_token()` and `Context::cancellation_token()`
expose it. Use `wait_cancelled_timeout(Duration)` rather than busy-waiting.

Create application-owned worker threads using `Context::spawn_task("name",
|cancellation| ...)` or `RunningApp::spawn_task`. On cancellation, every
task must finish cooperatively. Riesel joins tasks **before** dropping
capability clients or stopping processes. Task errors and panics cancel the
application and become `AppRunError::TaskFailure`; simultaneous errors are
retained rather than silently discarded. At most **64 concurrent
application-owned OS threads** may be registered; finished tasks are
joined and their slots and names can be reused. Blocking, non-cooperative
tasks can still prevent shutdown indefinitely; no thread is forcibly killed.

Default deployments use `fail_fast`. To opt into a bounded restart of the
**whole application session**, add this top-level deployment policy:

```json
{
  "services": {
    "counter": {
      "transport": "managed",
      "program": "/absolute/path/to/counter_service"
    }
  },
  "recovery": {
    "mode": "restart_application",
    "max_restarts": 3,
    "initial_backoff_ms": 100,
    "max_backoff_ms": 2000
  }
}
```

A restart cancels and joins old tasks, clears old typed handles, tears down
managed processes in reverse startup order, and creates **new** providers
and clients in dependency order. An exponential bounded backoff is applied
between attempts. On exhaustion, `AppRunError::RecoveryExhausted` retains
the final managed-service failure.

The policy retries only unexpected managed-process exits after the entrypoint
returns cooperatively, with no concurrent business or shutdown error. It
**never** silently retries application business errors, startup failures or
background task failures. The Rust SDK entrypoint is `FnMut` so it can be
called again with an entirely new `Context`.

**Important limitations:** restart reruns application work; callers must
ensure replay is safe and handle external side effects. In-memory state is
lost. Generated handles retained by application code reject new calls
after their session expires; they do not transparently rebind. Rust threads
and blocking synchronous entrypoints cannot be forcibly interrupted; tasks
must cooperate. External TCP health monitoring, transparent per-service
reconnection and persistent-state recovery are not provided.

## Documentation

- [Implementation roadmap](docs/product/riesel/implementation-roadmap.md)
- [Product definition](docs/product/riesel/riesel.md)
- [Todo example](apps/examples/todo/README.md)
- [Counter example](apps/examples/counter_rpc/README.md)

Riesel is under active development and intentionally permits breaking API
changes during its initial architecture phase.
