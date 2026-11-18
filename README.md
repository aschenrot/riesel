# Riesel

Riesel is an experimental Rust-first, schema-driven application runtime.
Applications declare versioned capabilities and bind them to local, external,
or runtime-managed providers. The runtime handles provider validation,
generated typed RPC, managed child startup, and dependency-ordered teardown
with optional bounded cooperative shutdown. Both Storage and Counter use
the same generic RPC host; neither component implements its own wire protocol
or process lifecycle loop.

**Status:** reference applications work and runtime-owned children are
supervised with cooperative cancellation. Production persistence,
authenticated remote transport, automatic service recovery, and UI tooling
are not implemented. Do not use the in-memory Storage reference as durable
storage.

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
managed TCP Storage and multi-service failure cleanup on Linux and Windows.

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
not provider discovery or automatic recovery.

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

## Observing and cancelling managed applications

A single runtime-owned monitor watches child process exits. An unexpected
exit of any required **managed** service (including exit code 0) cancels the
running application's shared `CancellationToken`. Both
`RunningApp::cancellation_token()` and `Context::cancellation_token()`
provide access to the same signal. Workers can use
`wait_cancelled_timeout(Duration)` instead of busy waiting, and application
code can call `cancel()` explicitly. `RunningApp::check_processes()` exposes
the recorded service failure.

For long-running application code:

```rust
let cancellation = ctx.cancellation_token();
while !cancellation.wait_cancelled_timeout(std::time::Duration::from_millis(100)) {
    // Perform one bounded unit of application work.
}
```

The existing synchronous `run` entrypoint reports an `AppRunError::Supervision`
when a required managed child fails, **once the entrypoint returns**. Rust
closures cannot be preempted safely: application code must check its
cancellation token and return cooperatively. The runtime joins monitoring
before shutting down children and records their final exit state.

There is currently **no automatic restart**, background monitoring of
externally owned TCP providers, or cancellation of arbitrary blocking I/O.
The default policy for required managed providers is to cancel and fail the
application rather than silently reuse a dead client.

## Documentation

- [Implementation roadmap](docs/product/riesel/implementation-roadmap.md)
- [Product definition](docs/product/riesel/riesel.md)
- [Todo example](apps/examples/todo/README.md)

Riesel is under active development and intentionally permits breaking API
changes during its initial architecture phase.
