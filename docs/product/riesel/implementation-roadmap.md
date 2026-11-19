# Riesel — Implementation Roadmap

Status reviewed: 2026-11-19. Stage 5 reference commit: `b862e3f` (Linux [validation and smoke passed](https://github.com/aschenrot/riesel/actions/runs/44535298922)). This document tracks **accepted delivery scope and tested progress**. Git commits, Pull Requests and CI retain implementation history; this roadmap is not a running change log.

## Current delivery position

| Stage | State | Acceptance boundary |
| --- | --- | --- |
| 0 — Rust foundations | Reference implementation | Workspace, Rust models, capabilities and initial Storage contract |
| 1 — Declarative requirements | Implemented | Generated inspectable application schema and versioned requirements |
| 2 — Provider resolution | Implemented for exact versions | Validated registration, selection and ambiguity/missing-provider errors |
| 3 — Capability binding | Reference implementation | Generated typed local/managed RPC and shared component instances |
| 4 — Runtime lifecycle | Implemented within cooperative scope | Dependency ordering, preflight, rollback, supervision, task joins and opt-in whole-session restart |
| 5 — Developer experience | Implemented within reference scope | 5.1–5.8 verified on Linux; Windows remains a manual release gate |
| 6 — Production reliability | Partially delivered | Linked SQLite persistence and SDK integration; secure remote transport, authorization, observability and operational hardening remain |

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
| 5.8 — DX and regression review | **Complete** | Documented Hello/Todo/Counter entrypoints, macro diagnostics, inspection and Markdown link checks verified |

Detailed delivered authoring APIs: [High-level application](high-level-app.md), [accepted authoring contract](api-authoring-v3.md), [application modules](application-modules.md), and [provider catalog](provider-catalog.md). Async execution invariants: [execution contract](async-execution-contract.md). Runtime lifecycle and recovery invariants: [runtime lifecycle](runtime-lifecycle.md).

## Stage 5.7 — verified lifecycle delivery

Optional `ApplicationModule` registrations now include named, typed async start/stop hooks using the existing `Application` builder, capability DAG, `RunningApp` session and `AsyncTasks` owner. The runtime creates no additional scheduler or provider resolver. Invalid registrations fail before startup; normal shutdown drains owned work before eligible stop hooks and provider teardown. Capability-dependent cleanup is skipped after session revocation.

Counter integration tests verify order, partial startup cleanup, combined error sources, local/managed parity and fresh hooks after a real managed crash. Stage 5.7 acceptance evidence: [Issue #10](https://github.com/aschenrot/riesel/issues/10), [implementation PR #14](https://github.com/aschenrot/riesel/pull/14), and [Linux validation and smoke](https://github.com/aschenrot/riesel/actions/runs/44530550169). Precise semantics and limitations: [application modules](application-modules.md) and [runtime lifecycle](runtime-lifecycle.md).

## Stage 5.8 — verified developer experience

The documented Hello, schema-based Hello, and high-level Todo entrypoints now
execute in `cargo smoke`, which also inspects Todo's generated definition.
Existing Counter, macro/builder equivalence, provider catalog, deployment
inspection, and side-effect-free preflight tests remain in place. Macro tests
assert concrete diagnostics for invalid signatures and duplicate declarations,
with a representative compile-fail application example.

All relative Markdown destinations are now checked by the existing
`cargo validate --check` gate, without adding a separate CI job or dependency.
The link checker was delivered in [PR #16](https://github.com/aschenrot/riesel/pull/16);
authoring and diagnostics checks are covered by
[PR #15](https://github.com/aschenrot/riesel/pull/15).
Acceptance evidence: [Issue #11](https://github.com/aschenrot/riesel/issues/11)
and [combined Linux validation and smoke](https://github.com/aschenrot/riesel/actions/runs/44535298922).
Manual Windows verification remains required before release, not for routine
Linux milestone integration.

## Stage 6 — Production reliability (not a release promise)

The first bounded Storage slice is delivered through linked SQLite and opt-in SDK registration. The [Todo product](../todo/todo.md) now exercises it through persistent R1 headless commands and a local-only R2 SvelteKit/browser integration with a Riesel-owned HTTP session. The in-memory R0 regression example remains separate. These **product-level acceptance scenarios** do not establish complete Stage 6 reliability or general production release readiness.

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
