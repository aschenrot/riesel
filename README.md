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

## Defining lifecycle dependencies

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
retained rather than silently discarded.

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
lost. Existing client handles must not outlive a session. Rust threads and
blocking synchronous entrypoints cannot be forcibly interrupted; tasks must
cooperate. External TCP health monitoring, transparent per-service
reconnection and persistent-state recovery are not provided.

## Documentation

- [Implementation roadmap](docs/product/riesel/implementation-roadmap.md)
- [Product definition](docs/product/riesel/riesel.md)
- [Todo example](apps/examples/todo/README.md)

Riesel is under active development and intentionally permits breaking API
changes during its initial architecture phase.
