# Riesel — Product Definition

## Purpose

Riesel is a Rust-first, schema-driven application platform. Applications declare the capabilities they need; components provide implementations; Riesel validates, composes, and runs them.

The product objective is to remove repeated infrastructure wiring without forcing application code into one UI, process, or execution model. Applications can combine generated definitions with ordinary Rust behavior.

This document specifies intended product outcomes, **not** the current implementation status. See the [implementation roadmap](implementation-roadmap.md) for verified delivery.

## Problem and users

Application developers repeatedly rebuild model validation, capability communication, service lifecycle, state management, and integration with tooling. This complicates reuse and introduces differences in behavior between local and independent execution.

Riesel primarily serves Rust developers building applications, tools, and headless services. Real-time applications and games are potential specialized uses of the same foundation; they do not dictate the core runtime architecture.

## Product model

| Concept | Responsibility |
| --- | --- |
| **Application** | Logical software unit with requirements, behavior, and a lifecycle; not necessarily a single process |
| **Definition** | Inspectable, non-executable description of models, requirements, and configuration |
| **Capability** | Versioned contract that consumers depend on, independent of implementation |
| **Component** | Implementation of one or more capabilities, potentially sharing state |
| **Provider** | A registered implementation selected for a requirement |
| **Instance** | Actual running provider execution with a distinct identity and lifecycle |

A component is **not inherently an operating-system process**. Local and independently executed providers can implement the same contract, subject to compatible semantics. Deployment and execution policy determine where components run.

The application uses capabilities; components implement capabilities; Riesel resolves and runs them.

## Accepted product requirements

### Composition and developer experience

- Developers can define application requirements, models, and capability contracts using supported Rust authoring APIs; portable, inspectable definitions are derived without a second manually synchronized schema.
- A capability requirement can be resolved to a local, explicitly managed, or configured external implementation.
- Normal application authoring does not require handwritten transport clients, dispatch loops, or manually assigned ephemeral infrastructure IDs.
- Ambiguous requirements, invalid definitions, and incompatible provider selection fail with actionable diagnostics rather than implicit fallback.
- Ordinary custom Rust behavior remains possible without requiring a particular ECS, GUI, or transport implementation.

### Execution and lifecycle

- Application logic can execute headlessly, with no graphical window.
- Application, component, provider-instance, and process lifecycles are distinguished explicitly.
- Startup ordering, shutdown, cancellation, and recovery respect declared dependencies and stated ownership boundaries.
- Recovery never implies that ambiguous external side effects were not executed; individual commands are not silently replayed.
- Independent native window hosts use separate operating-system processes, with lifecycles independent from application logic. This is an **accepted target requirement**, not a claim of current delivery.

### Executable plans and operational inspection (partially delivered)

- Developers can compose and validate inspectable, versioned **execution plans** of registered Rust/capability operations and run them **headlessly**. This operation graph is distinct from capability/provider selection and the startup dependency DAG.
- Riesel provides reusable general graph execution for the operations it owns, with **one declared admission/control authority per execution boundary**. Specialized domain runtimes may retain their own internal tick, frame and system scheduling.
- Developers can inspect the exact plan revision and authoritative run state, correlate session/run/attempt events, and cooperatively pause, advance or resume **supported** safe points without arbitrary thread suspension.
- Cancellation, failure and **uncertain outcomes after dispatch** preserve confirmed committed effects; lost events or replies never imply rollback, nonexecution or permission to retry.
- **Optional graphical inspection** is an accepted product goal over the same headless semantics, not a required GUI or separate execution owner.
- **Editable executable visual composition** is an accepted **long-term direction** for an explicitly specified subset of registered operations. Its source of truth, graph edits, typed data edges, version/layout semantics and executable round trip must be specified before implementing an editor. An unrestricted visual Rust builder is not an accepted guarantee.

Durable boundaries and delivery gates are defined in the [accepted execution-plan contract](execution-plan-contract.md), based on [specification #83](https://github.com/aschenrot/riesel/issues/83). A bounded session-owned headless runner, live observation, confirmed-effect prefixes and cooperative **outer admission** controls are implemented in the reference SDK; see the [runner's verified scope](execution-runner.md). The optional graphical inspector, editing contract, external authorization and deeper domain-owner integration remain separate and are **not** delivered by these local APIs.

### Optional domain-owned nested execution observation (accepted direction; implementation pending)

- A developer **may opt in** to an application/domain-owned, **read-only** observation of nested work executed inside a delegated operation, such as an ECS frame/tick or a non-game processing pipeline. An application without such an adapter retains the complete existing **outer-only** inspection and execution workflow. Neither Riesel nor the application must use an ECS to participate.
- Riesel shall offer reusable, minimal **correlation, snapshot and loss semantics** associating a domain-authoritative nested view with an exact outer application session, plan/revision, graph run and operation/attempt, and the domain's own owner-instance generation. Domain-owned work identities, state, revision/high-water marks and published capabilities originate with that owner, **not** from synthesized outer graph nodes.
- Nested observation may use **versioned domain-specific capability views and typed payloads**, with optional specialized inspector presentation. The platform-wide part must provide discoverable support/absence, bounded retention, detectable event loss and authoritative resynchronization without requiring one universal inner ECS/system/stage model. A domain's internally meaningful state and scheduling semantics remain under its control.
- Nested owner snapshots cannot imply a global event order with the outer graph, change already reported outer terminal or attempt-confirmed effect classifications, establish durable replay, or make ambiguous mutations safe to retry. A domain owner replacement invalidates the old domain attachment independently of the outer session generation.
- **No nested control is accepted in this increment.** Internal pause/step/resume, system breakpoints and request admission remain exclusively domain-owned and require separate future specification if exposed through Riesel. This accepted product outcome does not authorize a public ABI, cross-process exporter, privileged UI reader, new provider resolver, ECS dependency or automatic child-run scheduler.

**Decision:** [#146 owner acceptance (22 November 2026)](https://github.com/aschenrot/riesel/issues/146#issuecomment-7073454321). **Evidence:** [research PR #148](https://github.com/aschenrot/riesel/pull/148), using real Bevy ECS, pinned RunenECS and a non-ECS coordinator; research-only fixtures are not approved public contracts. The [execution-plan contract](execution-plan-contract.md) specifies the durable authority/identity boundaries. The first implementation is a separate **trusted local read-only** foundation; external inspection requires accepted #60/#75 security and interface semantics.

### Contracts, identity, and interfaces

- Applications depend on versioned capability contracts rather than specific providers.
- Private identifiers can be derived by the framework; independently published contracts retain a deliberate stable external identity.
- CLI and programmatic interfaces should expose comparable supported platform operations with consistent error semantics.
- New capabilities and specialized execution integrations can be added without replacing the foundational application model.

### Reusable typed Query semantics (accepted direction; implementation pending)

- Riesel shall support **optional, reusable, transport-neutral typed Query semantics** for selective, correctly typed, bounded retrieval of capability-owned application data. Applications and providers may adopt the shared semantic foundation where its behavior is meaningful; specialized domains are not obliged to implement one universal Query interface or executor.
- Shared semantics shall make supported typed predicates, model/schema binding, result bounds, ordering and continuation, explicit incomplete/consistency outcomes, unsupported-feature errors and resource budgets understandable and verifiable. A query request must not silently change the declared type, ownership, authorization or consistency rules of its domain.
- **The authoritative data owner remains in control of physical query execution and indexes.** Linked Storage is the first concrete research consumer; ECS, spatial, graph, search, streaming and analytical domains retain specialized execution and data ownership.
- The reusable semantic direction is **accepted as a product outcome**, not as a public Query contract, language, trait, SQL engine, deployment policy or release claim. The interface for registered records is **not yet selected**: compare a separately versioned read-designated facet, a unified/attenuated Storage surface, a composite typed authoring handle and viable alternatives against the same model/read/write tasks before accepting a technical architecture.
- Any first read-only application interface must support typed model registration without granting write methods; applications requiring read-your-writes must declare and verify the **logical data authority and supported consistency** rather than assuming two capability aliases or a lifecycle dependency share state. A read-designated method surface alone does not authorize external readers. Secure cross-process access and physical index maintenance require separately accepted guarantees.

**Decision:** [#138, owner acceptance on 2026-11-22](https://github.com/aschenrot/riesel/issues/138#issuecomment-7073259869). **Research:** [PR #140](https://github.com/aschenrot/riesel/pull/140). The product decision does not approve implementation; record-interface packaging, model-only authoring, permissions and query-owner binding remain specification work.

## Product interfaces and delivery boundaries

| Interface | Intended role | Current boundary |
| --- | --- | --- |
| **Rust SDK** | Primary application and component authoring API | Experimental public `riesel` facade |
| **Application CLI** | Deployment selection, inspection, and application-specific commands | App-scoped inspection and runtime arguments exist; a full platform CLI is not implied |
| **Riesel Manager** | Registration, discovery, process supervision, and coordination | Existing `riesel_manager` crate is a process-ownership primitive, not a completed user-facing manager |
| **Manager UI** | Inspect and manage applications and infrastructure | Planned, not delivered |
| **Native window hosts** | Independent interfaces attached to application logic | Planned, not delivered |

The in-memory reference Storage provider is ephemeral; a linked SQLite provider is available for explicit single-host local persistence. Loopback TCP is not authenticated. Passing development tests does **not** establish production-wide durability, authenticated remote networking or distributed recovery.

### Durable Record Storage evolution

- Linked local SQLite Storage shall support an **explicit, offline, authorized** transformation of Rust-derived application models, with pre-mutation verified backup, preserved UUIDs, checked revision fencing, exact source/target validation, and bounded recovery.
- Ordinary application startup remains fail-closed on incompatible stored models; automatic/online upgrades and remote/managed parity are not accepted.
- The [accepted model migration architecture](sqlite-offline-migration-architecture.md) defines the controlled single-host ownership, legacy process rollout and verification boundary. Provider ownership, offline backup and an internal typed migration engine have partial delivered evidence; **the approved operator preview/consent workflow is not delivered**, so the end-to-end product outcome remains incomplete. See the [roadmap](implementation-roadmap.md) and implementation [#50](https://github.com/aschenrot/riesel/issues/50)/[#51](https://github.com/aschenrot/riesel/issues/51).

## Planned infrastructure extensions

- Deliver the accepted bounded SQLite model-evolution workflow and separately plan any broader Storage support.
- Authentication, authorization, and protected remote transport.
- Operational visibility, configurable resource budgets, and reliable deployment management.
- Standalone manager workflows and independently executed native interfaces.
- Investigate coherent **cross-project developer tooling** and **shared application identity/readiness/interface semantics** without assuming one daemon, supervisor, public exporter or new configuration source ([developer tooling #68](https://github.com/aschenrot/riesel/issues/68), [operational boundary #75](https://github.com/aschenrot/riesel/issues/75), [Manager #59](https://github.com/aschenrot/riesel/issues/59), [native host #60](https://github.com/aschenrot/riesel/issues/60)). The architecture and expanded product scope remain subject to explicit acceptance.

These outcomes require separate designs and acceptance criteria; they must not be treated as already implemented.

## Exploratory product directions

Game/ECS integrations, automatic multiplayer, subscription or messaging services, a custom UI framework, and an **unrestricted general-purpose visual application builder** remain exploratory specialized directions. These are not core-runtime prerequisites and do not automatically require new crates. This does **not** supersede the accepted optional execution inspector or the bounded long-term executable-graph editing direction above.

## Product-level success criteria

1. The same declared capability can be consumed through supported local and managed providers with equivalent contract behavior, verified by integration tests.
2. Applications can run headlessly and expose inspectable definitions with deterministic dependency and provider validation.
3. Ordinary example applications use typed authoring interfaces without repeating manual RPC or process-wiring code.
4. Session shutdown and recovery have documented, testable cancellation, identity, and side-effect boundaries.
5. Platform interfaces provide consistent operations where both interfaces are supported.
6. Future application types can extend the platform through contracts or specialized integrations without altering its core composition model.

A requirement is accepted when its specific implementation gate passes; broad product success criteria are not proof that every planned feature ships today.
