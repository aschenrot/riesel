# Riesel

Riesel is an experimental Rust-first application runtime. Applications declare versioned **capabilities**; **components** implement them; Riesel validates, composes, and executes the selected providers. The application author does not need to write a separate RPC client, service loop, or process manager for each capability.

**Status:** development/reference framework, not a production-ready application platform. For current implementation gates and the accepted target API, see the [roadmap](docs/product/riesel/implementation-roadmap.md) and [authoring contract](docs/product/riesel/api-authoring-v3.md).

## Quick start

The workspace uses Rust 1.100.0. From the repository root:

```sh
cargo run -p riesel_hello_example
cargo run -p todo --bin todo
cargo run -p todo --bin todo -- --print-definition

cargo validate --check
cargo smoke
```

`cargo validate --check` checks formatting, workspace builds, Clippy, tests and documentation. `cargo smoke` runs executable integration scenarios for Todo, Storage, Counter, managed processes and lifecycle behavior. Linux CI is required during development; Windows validation is manual and required before release.

The Todo command runs a **demonstration using explicitly selected ephemeral MemoryStore**. It does not save tasks across restarts.

## Core model

| Concept | Meaning |
| --- | --- |
| **Application** | Declares behavior, capability requirements and lifecycle |
| **Capability** | Versioned contract consumed independently of implementation |
| **Component** | Implements one or several capability contracts, possibly sharing state |
| **Provider** | A selected implementation, running locally or through a configured endpoint |
| **Instance** | A specific execution of a provider, with separate identity and lifetime |

A component is not automatically a separate process. One component can expose multiple contracts through one shared state instance.

### Provider execution

- **Linked/local:** a statically registered component runs in process, without TCP.
- **Managed:** Riesel starts and supervises a configured child executable, using a loopback endpoint and a readiness protocol.
- **External:** a separately owned provider exposes a declared contract over explicitly configured loopback TCP.

The provider catalog supports explicit named managed/external placements and selection-only manifests. Unambiguous linked providers can be selected by default; remote and managed placements require explicit selection. Provider metadata validation is **not remote authentication**.

See [provider catalog and selection](docs/product/riesel/provider-catalog.md) and [Counter example](examples/counter/app/README.md).

## Application authoring

Use the `riesel` SDK to derive application definitions from Rust models, capability contracts, and typed handler dependencies. The schema is inspectable without maintaining a duplicate handwritten application JSON file.

- `#[capability]` defines generated contracts and typed local/remote bindings.
- `#[component(provides = [...])]` registers implementations and generates provider adapters.
- `Application::handler` and `Application::async_handler` derive dependencies from function parameters where supported.
- The `records` feature provides typed Storage records; `dev-memory-storage` explicitly opts into an in-memory reference provider.
- `async-tcp` adds true nonblocking Tokio transport and async execution support without making Tokio mandatory for the minimal SDK.

The higher-level `#[riesel::app]` entrypoint and related ergonomic APIs are tracked in the [authoring contract](docs/product/riesel/api-authoring-v3.md). Verify their latest delivery status in the roadmap rather than assuming every target syntax example is released.

### Inspection and configuration

Applications expose runtime options where supported:

```sh
cargo run -p todo --bin todo -- --print-definition
cargo run -p todo --bin todo -- --inspect-deployment path/to/deployment.json
```

Applications that register a provider catalog can additionally use `--select-providers FILE` and `--inspect-catalog`. For application-specific CLI arguments, use the SDK's explicit argument-forwarding entrypoint.

Inspection performs local validation and prepares a plan without starting providers. It does not prove external availability or authenticate a remote process.

## Lifecycle and reliability boundaries

Managed child processes are supervised. Application-owned tasks use cooperative cancellation and are joined during teardown. Opt-in bounded recovery starts a **new whole-application session**; it does not transparently rebind an existing capability handle or retain MemoryStore data.

Generated session-bound handles reject calls after their session expires. Requests are not implicitly retried because a failed or timed-out command might already have executed. Blocking Rust code that does not cooperate with cancellation cannot safely be forcibly interrupted in process.

Reference RPC over TCP is **unencrypted, unauthenticated and loopback-only**. Production persistence, protected remote transport, authorization, broader operations tooling and independent GUI hosts remain future work.

See the [async execution contract](docs/product/riesel/async-execution-contract.md) for detailed execution semantics.

## Repository layout

| Directory | Purpose |
| --- | --- |
| `crates/` | Framework runtime, SDK, schema, macros, protocol and process primitives |
| `packages/storage/` | Reusable Storage contract and ephemeral memory implementation |
| `examples/hello/` | Minimal Rust-first application |
| `examples/counter/` | Independent capability, shared-state component and local/managed tests |
| `examples/todo/` | Typed model and task-operations reference application |
| `tools/xtask/` | Validation and executable smoke-test tasks |
| `docs/product/` | Product definitions, accepted architecture contracts and active roadmap |

## Documentation

- [Riesel product definition](docs/product/riesel/riesel.md)
- [Implementation roadmap](docs/product/riesel/implementation-roadmap.md)
- [High-level application authoring contract](docs/product/riesel/api-authoring-v3.md)
- [Async execution contract](docs/product/riesel/async-execution-contract.md)
- [Provider catalog and selection](docs/product/riesel/provider-catalog.md)
- [Todo reference](examples/todo/README.md)
- [Counter reference](examples/counter/app/README.md)

Riesel is in an early architecture phase; intentional breaking changes are permitted. Published capability contract identities and error/lifecycle semantics must nevertheless be changed deliberately and verified by tests.
