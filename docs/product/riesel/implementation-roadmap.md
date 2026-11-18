# Riesel — Implementation Roadmap

Status reviewed: 2026-11-18  
Working branch: `feature/schema_based_app`

This file tracks **implementation status**, not the long-term product requirements.
See [Product Definition](riesel.md) for product scope and principles.

## Validation gates

All code milestones must pass on Linux and Windows:

```sh
cargo validate --check
cargo smoke
```

`cargo validate --check` checks formatting without modifying files, builds
all workspace targets, runs Clippy with warnings denied, executes unit tests,
and generates documentation. `cargo smoke` runs a real Todo-to-Storage TCP
integration, with the Storage process owned and cleaned up by Riesel.

## Current milestones

| Stage | State | Evidence |
| --- | --- | --- |
| 0 — Rust foundations | Implemented | Cargo workspace, schema validation, macros, Storage contract and Todo |
| 1 — Declarative requirements | Implemented | Application schema names versioned required capabilities |
| 2 — Provider resolution | Implemented for exact versions | Registry, deterministic match, explicit selection, clear errors |
| 3 — Capability binding | Implemented for present scope | Typed in-process binding; verified remote TCP binding |
| 4 — Application runtime | Initial vertical slice implemented | `App::start`, `App::run`, `ConnectorRegistry`, `RunningApp`, managed child lifecycle |
| 5 — Developer experience | Partial | Workspace validation, GitHub Actions, `cargo smoke`; inspection CLI and RPC generation pending |
| 6 — Production reliability | Not implemented | Persistent Storage, identity and authorization, secure transports, recovery and deployment guarantees |

"Implemented" here means **within the stated scope** and covered by tests,
not a promise of production readiness.

## Current executable slice

1. `apps/examples/todo/app.json` declares `riesel.storage.records@1`.
2. A deployment configuration selects an external TCP endpoint or trusted
   `managed_tcp` executable.
3. The SDK's Storage connector discovers the service and exposes a typed
   `RecordStore` instance.
4. The generic runtime validates contract shape, resolves the provider, and
   makes the instance available to Todo by requirement name.
5. `App::run` executes Todo's entrypoint and stops any managed child processes.
   Startup failures and entrypoint failures also trigger cleanup.
6. `cargo smoke` verifies the managed execution path on loopback.

## Phase 4 remaining

- Define dependencies between managed services and initialize them in a
  validated order, with reverse-order teardown.
- Establish graceful shutdown before forced termination.
- Add a documented health/supervision policy. Current health inspection is
  on demand; there are no automatic restarts.
- Improve lifecycle and error diagnostics, including startup timeouts and
  partial-failure rollback tests for multiple services.
- Keep process management and functional capability resolution distinct.

## Phase 5 priorities

- Add CLI commands to inspect application definitions, provider selection
  and contracts without starting a process.
- Generate protocol proxies and dispatch adapters from the authoritative Rust
  capability definitions, replacing handwritten Storage transport boilerplate.
- Add compatibility/contract tests for independent provider implementations.
- Preserve fast local development and the flat `crates/` / `components/` layout.

## Phase 6 minimum gates

- Persistent Storage provider and migrations.
- Authentication, authorization, encrypted remote communication and
  management of secrets.
- Connection resilience, resource limits, failure recovery, observability,
  deployment hardening and reproducible builds.
- Documented compatibility guarantees across provider and protocol versions.

The present Storage server is single-process, in-memory, and uses
**unauthenticated, unencrypted TCP**. Managed mode is deliberately restricted
to loopback. Provider advertisements prove structural compatibility, **not
identity or trust**. Only use `managed_tcp` with trusted deployment files.

## Deliberately deferred

GraphQL, SQL query capabilities, a custom UI framework, game-specific systems,
distributed orchestration and the visual application builder are not
prerequisites of this initial platform runtime.
