# Riesel — Implementation Roadmap

Status reviewed: 2026-11-19. Stage 5.7 reference commit: `b778658d` (Linux [validation and smoke passed](https://github.com/aschenrot/riesel/actions/runs/44530550169)). This document tracks **accepted delivery scope and tested progress**. Git commits, Pull Requests and CI retain implementation history; this roadmap is not a running change log.

## Current delivery position

| Stage | State | Acceptance boundary |
| --- | --- | --- |
| 0 — Rust foundations | Reference implementation | Workspace, Rust models, capabilities and initial Storage contract |
| 1 — Declarative requirements | Implemented | Generated inspectable application schema and versioned requirements |
| 2 — Provider resolution | Implemented for exact versions | Validated registration, selection and ambiguity/missing-provider errors |
| 3 — Capability binding | Reference implementation | Generated typed local/managed RPC and shared component instances |
| 4 — Runtime lifecycle | Implemented within cooperative scope | Dependency ordering, preflight, rollback, supervision, task joins and opt-in whole-session restart |
| 5 — Developer experience | In progress | 5.1–5.7 completed on Linux; 5.8 DX review remains |
| 6 — Production reliability | Not delivered | Persistence, secure remote transport, authorization, observability and operational hardening |

**Implemented** here means tested within a stated reference scope, not production maturity. Windows validation remains manual before releases.

### Stage 5 delivery gates

| Step | State | Tested result / remaining exit gate |
| --- | --- | --- |
| 5.1 — Contract identity | Complete | Derived private IDs; stable explicit published IDs; lifecycle and compatibility contract |
| 5.2 — Typed dependencies | Complete | Rust function parameters derive requirements and aliases; Counter/Todo and schema-equivalence tests |
| 5.3 — Generated components | Complete | Single component registration creates local and managed adapters; shared multi-capability state |
| 5.4 — Native async | Complete | True nonblocking RPC, async component parity, bounded owned work, cancellation, crash/recovery tests |
| 5.5 — Provider catalog | Complete | Explicit named placement, strict selection-only manifest, pinned metadata, side-effect-free inspection |
| 5.6 — High-level app API | Complete | `#[riesel::app]`, native async Counter, Records Todo, aliases, catalog choices and builder-equivalence tests |
| 5.7 — Modules and lifecycle | **Complete** | Optional typed start/stop hooks, DAG ordering, scoped shutdown and local/managed recovery regressions |
| 5.8 — DX and regression review | **Not started** | Review diagnostics, examples, documentation, inspection and full validation against the accepted API |

Detailed delivered authoring APIs: [High-level application](high-level-app.md), [accepted authoring contract](api-authoring-v3.md), [application modules](application-modules.md), and [provider catalog](provider-catalog.md). Async execution invariants: [execution contract](async-execution-contract.md). Runtime lifecycle and recovery invariants: [runtime lifecycle](runtime-lifecycle.md).

## Stage 5.7 — verified lifecycle delivery

Optional `ApplicationModule` registrations now include named, typed async start/stop hooks using the existing `Application` builder, capability DAG, `RunningApp` session and `AsyncTasks` owner. The runtime creates no additional scheduler or provider resolver. Invalid registrations fail before startup; normal shutdown drains owned work before eligible stop hooks and provider teardown. Capability-dependent cleanup is skipped after session revocation.

Counter integration tests verify order, partial startup cleanup, combined error sources, local/managed parity and fresh hooks after a real managed crash. Stage 5.7 acceptance evidence: [Issue #10](https://github.com/aschenrot/riesel/issues/10), [implementation PR #14](https://github.com/aschenrot/riesel/pull/14), and [Linux validation and smoke](https://github.com/aschenrot/riesel/actions/runs/44530550169). Precise semantics and limitations: [application modules](application-modules.md) and [runtime lifecycle](runtime-lifecycle.md).

## Next priority — Stage 5.8 developer experience

After lifecycle behavior stabilizes:

- Make the minimal Counter and Todo authoring paths compile and run exactly as documented.
- Verify that compile-fail diagnostics explain invalid macros, alias collisions, component requirements and unsupported execution modes.
- Confirm builder and macro output remain equivalent where both APIs support the same composition.
- Audit documentation for stale feature claims and consolidate redundant setup/implementation history.
- Cover the supported CLI deployment/catalog inspection modes and validate no provider is started during side-effect-free inspection.
- Run Linux validation and smoke. Record the separate manual Windows release gate.

**Exit gate:** a new developer can understand the public model, compile supported examples, inspect provider selection, and diagnose an invalid configuration without studying framework internals.

## Production follow-on — Stage 6 (not a release promise)

Plan and separately accept production features by user value and operational risk. Current candidates:

| Area | Required future outcomes |
| --- | --- |
| Storage | Durable backend, migration/versioning, schema enforcement and crash/restart tests |
| Networking | Authenticated/authorized remote transport, trust establishment and secrets handling |
| Runtime | Operational metrics, configurable resource budgets and safe stateful recovery contracts |
| Tools | Manager/discovery workflows, platform CLI parity, and repeatable deployment |
| Interfaces | Independent native window hosts, once their lifecycle and connection contract is accepted |

Selective provider restarts with transparent rebinding, distributed orchestration, automatic multiplayer, UI frameworks and visual composition remain **exploratory** until independently scoped and justified. Current loopback plaintext RPC and in-memory reference Storage are not production equivalents.

## Validation, risk and release rules

```sh
cargo validate --check
cargo smoke
```

Linux CI is the development quality gate for changes that affect executable behavior. A passing check is tied to **one exact commit**, not an indefinitely green branch. The release procedure must additionally run Windows manually and evaluate production-specific security, durability and operational claims; it cannot infer them from these reference tests.

Key current limits: cooperative Rust work can block shutdown; dropping an async RPC future does not undo already transmitted commands; a whole-session restart may replay entrypoint work; in-memory provider state is lost on restart; the provider handshake validates declared metadata rather than authenticating a peer. See [runtime lifecycle](runtime-lifecycle.md) and [async execution](async-execution-contract.md).

## Work management

The roadmap records outcomes and stages, not individual commit history or a second task list. Actionable work, acceptance evidence and exceptions belong in GitHub Issues/PRs under the [delivery model](../../project/delivery.md). Implementations need one coherent reviewed change and relevant tests, not one commit per checklist item.
