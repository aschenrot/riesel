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
| 5 — Developer experience | RPC generation proven; still partial | Rust SDK, model derive, generated typed RPC client and dispatcher, generic bounded TCP transport, schema export |
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

`Application::require::<SomeContract>("alias")` supports other generated
capability contracts. Their provider factories and client adapters still need
to be registered explicitly until a provider catalog is available.

`development()` is an **explicit opt-in** to ephemeral in-memory storage,
not a production default or persistence guarantee.

## Runtime-managed service modes

- `local`: in-process implementation, no socket or process required.
- `tcp`: explicit remote address, owned by an external manager.
- `managed_tcp`: process started by Riesel on a configured loopback port
  (retained for compatibility).
- `managed_tcp_dynamic`: service binds `127.0.0.1:0` and sends a bounded
  `RIESEL_READY` message through child stdout. Riesel then verifies the
  advertised capability contract. Concurrent instances do not need distinct
  configured ports.

The smoke suite covers explicitly configured managed TCP and two concurrent
dynamic managed instances. The local factory is covered by unit tests.
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

**Compatibility boundary:** The original Storage TCP protocol and `TcpStore`
remain intact. Migrating Storage to the generic generated protocol, preserving
or explicitly versioning the old wire API, and integrating generic client
adapters into Riesel's provider registry are **not complete**. Generated
client methods return `Result<Output, CallError<DomainError>>`; they do not
silently implement the original trait, which lacks transport-error semantics.
The first generator supports synchronous, non-generic methods with owned
serializable inputs or borrowed `&str` and serde-compatible result types.

## Remaining engineering work, in priority order

1. **Storage migration and generic provider integration:** replace
   handwritten Storage protocol/client/dispatch wiring behind a compatible,
   tested migration strategy; register generated adapters in the runtime.
   Preserve explicit transport-error semantics and verify contract shapes.
2. **Lifecycle robustness:** dependency ordering, graceful and reverse-order
   shutdown, exit monitoring, cancellation, structured errors and multi-service
   rollback tests.
3. **Developer tooling and models:** inspect provider bindings without
   launching services, improve type coverage, add schema evolution rules,
   and add stable generated adapter compatibility tests.
4. **Production operation:** persistent Storage, migrations, secure and
   authenticated transports, authorization and secrets, resource limits,
   observability, repeatable deployments and recovery policies.

## Architectural and security boundaries

The present Storage server is **in-memory**, **single-process** and communicates
using **unencrypted and unauthenticated TCP**. A pipe-based startup announcement
and contract-shape check do **not** authenticate a network service.

Managed TCP defaults and dynamic endpoints are restricted to loopback.
Only execute trusted deployment files and binaries. External TCP endpoints
remain configurable but are not secured by the current implementation.

Do not expose the current TCP Storage server to untrusted networks.
Components remain separate from contracts in the **flat** repository structure:
`crates/riesel_storage` is the API contract; `components/storage` is an
implementation. The optional development SDK convenience may include the
reference implementation but does not change that contract boundary.

## Deferred until justified

GraphQL, SQL query capabilities, a custom UI framework, game-specific systems,
distributed orchestration, and the visual application builder are not
prerequisites of the current application runtime.
