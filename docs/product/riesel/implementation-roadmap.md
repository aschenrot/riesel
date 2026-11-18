# Riesel — Implementation Roadmap

Status reviewed: 2026-11-18
Working branch: `feature/schema_based_app`

This tracks **tested implementation status**, not all long-term product requirements.
See [Product Definition](riesel.md) for the intended product scope.

## Validation gates

For all code milestones on Linux and Windows:

```sh
cargo validate --check
cargo smoke
```

`cargo validate --check` checks committed formatting, all workspace targets,
Clippy (warnings denied), tests, and documentation. `cargo smoke` executes
the actual Todo and Storage binaries, including runtime-owned Storage processes.

## Milestone status

| Stage | State | Evidence |
| --- | --- | --- |
| 0 — Rust foundations | Initial implementation | Cargo workspace, model and capability macros, Storage contract |
| 1 — Declarative requirements | Implemented | Versioned requirements; Rust-generated application definitions |
| 2 — Provider resolution | Implemented for exact versions | Registration, explicit selection, missing/ambiguous-provider errors |
| 3 — Capability binding | Initial implementation | In-process factories, typed Rust record handles, verified remote TCP |
| 4 — Application runtime | End-to-end slice implemented | Application start/run, managed child startup, dynamic local endpoints, cleanup |
| 5 — Developer experience | Generic RPC integration implemented; still partial | Rust SDK, model derive, generated Storage and Counter RPC, generic typed connectors, schema export |
| 6 — Production reliability | Not implemented | Persistent Storage, secure transports, authorization, recovery, observability |

Completed stages mean *working within their currently specified scope*, not
production-grade infrastructure.

## Recommended Rust authoring workflow

`apps/examples/todo/src/task.rs` defines a Rust `TaskData` model, annotated
with `#[derive(RieselModel)]` and `#[riesel(model = "task")]`.
There is no hand-maintained `apps/examples/todo/app.json`.

`Application::new("Todo").records::<TaskData>()?.development()?`
registers the model and versioned Storage requirement and selects the
ephemeral in-process MemoryStore. `Context::records::<TaskData>()` gives a
typed CRUD handle. `--print-definition` exports the canonical generated
schema; `--deployment FILE` allows explicit external or managed providers.

`Application::rpc::<SomeContract>("alias")` declares a generated RPC
capability and automatically registers a shared transport connector.
`Context::capability::<SomeContract>("alias")` returns the typed service.
Implementations opt in using `#[capability(..., client_trait)]` and define an
explicit conversion from `RpcError` into their public error type. Custom
providers can still be registered explicitly. A general provider catalog and
automatic deployment discovery are not implemented.

`development()` is an **explicit opt-in** to ephemeral in-memory storage,
not a production default or persistence guarantee.

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

Deployment configuration remains optional for local development but is
necessary when choosing external or managed process execution.

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

The independent `apps/examples/counter_rpc` example exercises commands,
queries, borrowed string inputs, domain errors, incorrect arguments, wrong
capability versions, and real TCP request/response round-trips on Linux and
Windows. No hand-maintained request enum or per-operation dispatcher is
needed for that example.

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

1. **Lifecycle robustness (next):** service dependencies and startup
   ordering; graceful reverse-order shutdown; child exit monitoring,
   cancellation, failure policy, and multi-service rollback tests.
2. **Developer tooling and models:** inspect provider selection before
   launching services; improve model field types and schema evolution; test
   generated adapter compatibility with a second runtime-integrated provider.
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
`crates/riesel_storage` is the API contract; `components/storage` is an
implementation. The optional development SDK convenience may include the
reference implementation but does not change that contract boundary.

## Deferred until justified

GraphQL, SQL query capabilities, a custom UI framework, game-specific systems,
distributed orchestration, and the visual application builder are not
prerequisites of the current application runtime.
