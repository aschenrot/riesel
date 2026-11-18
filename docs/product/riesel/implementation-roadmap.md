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
| 5 — Developer experience | Improved, not complete | Rust SDK builder, model derive, common CLI flags, generated schema export, smoke tests |
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

## Remaining engineering work, in priority order

1. **Model and capability ergonomics:** generate higher-level typed API
   adapters and compatibility checks for independently developed providers;
   add richer model fields and an explicit model evolution policy.
2. **Lifecycle robustness:** service dependency graph, startup ordering,
   graceful and reverse-order shutdown, exit monitoring, cancellation,
   structured errors and multi-service rollback tests.
3. **Developer tooling:** inspect providers and bindings without launching
   services; grow the CLI coherently; generate RPC client/proxy and dispatcher
   code from authoritative capability traits rather than maintaining
   duplicate operation definitions.
4. **Production operation:** persistent Storage with migrations; secure,
   authenticated transports; authorization and secrets; resource limits,
   observability, repeatable deployment and recovery policies.

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
