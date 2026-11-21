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
- Independent Cargo workspaces shall have an **optional, reusable cross-project developer workflow** for build, inspection, foreground launch and stop; headless apps remain usable through ordinary Cargo without Manager or a UI toolchain. The command shape and SDK participation contract remain subject to [#68](https://github.com/aschenrot/riesel/issues/68).

### Execution and lifecycle

- Application logic can execute headlessly, with no graphical window.
- Application, component, provider-instance, and process lifecycles are distinguished explicitly.
- Startup ordering, shutdown, cancellation, and recovery respect declared dependencies and stated ownership boundaries.
- Recovery never implies that ambiguous external side effects were not executed; individual commands are not silently replayed.
- Independent native window hosts use separate operating-system processes, with lifecycles independent from application logic. They attach to deliberately published app-facing interfaces; closing a GUI does not imply stopping the backend. The exporter/toolkit mechanism remains subject to [#60](https://github.com/aschenrot/riesel/issues/60). This is an **accepted target requirement**, not a claim of current delivery.
- The Manager shall support explicitly registered local applications with inspectable status and authorized lifecycle control through consistent CLI/UI operations, with **one actual process owner per application**. Hybrid ownership is the target architecture candidate; its first OS adapter and crash/reconciliation guarantees require [#59](https://github.com/aschenrot/riesel/issues/59) acceptance.
- The first completed **installed Manager release** shall support **Linux, Windows and macOS** with equivalent accepted lifecycle, recovery and privileged-control security semantics. Only Linux runs routine development CI; documented manual Windows and macOS platform verification are mandatory **pre-release** gates. Linux implementation may precede the other adapters but is not itself the complete three-platform Manager release ([#59](https://github.com/aschenrot/riesel/issues/59)). This does not impose new routine non-Linux CI on unrelated Riesel components.
- Installed Manager registrations shall explicitly select a **user-session application** or **machine background service** execution scope. Both belong to the first Manager release on Linux, Windows and macOS; they share management semantics but use scope-appropriate independently owned processes and protected, authorized OS control. Machine services can run without interactive login, whereas user-session logout behavior and boot/login scheduling depend on the accepted platform mechanism. Manager privileges are not granted merely by the caller sharing an OS user account ([#59](https://github.com/aschenrot/riesel/issues/59)). This is a required product outcome, not current delivery or selection of a specific OS service technology.

### Contracts, identity, and interfaces

- Applications depend on versioned capability contracts rather than specific providers.
- Private identifiers can be derived by the framework; independently published contracts retain a deliberate stable external identity.
- CLI and programmatic interfaces should expose comparable supported platform operations with consistent error semantics.
- New capabilities and specialized execution integrations can be added without replacing the foundational application model.
- CLI, Manager and independent GUI integrations shall share coherent application/interface identity, evidence-based readiness, session-generation, authorization and failure semantics, without requiring a central daemon or replacing the runtime's provider resolver. The concrete external contracts and trust guarantees remain subject to [#75](https://github.com/aschenrot/riesel/issues/75).

## Product interfaces and delivery boundaries

| Interface | Intended role | Current boundary |
| --- | --- | --- |
| **Rust SDK** | Primary application and component authoring API | Experimental public `riesel` facade |
| **Application CLI** | Deployment selection, inspection, and application-specific commands | App-scoped inspection and runtime arguments exist; the accepted cross-project workflow is not delivered |
| **Riesel Manager** | Registration, discovery, process supervision, and coordination | Existing `riesel_manager` crate is a process-ownership primitive, not a completed user-facing manager |
| **Manager UI** | Inspect and manage applications and infrastructure | Planned, not delivered |
| **Native window hosts** | Independent interfaces attached to application logic | Planned, not delivered |

The in-memory reference Storage provider is ephemeral; a linked SQLite provider is available for explicit single-host local persistence. Loopback TCP is not authenticated. Passing development tests does **not** establish production-wide durability, authenticated remote networking or distributed recovery.

### Durable Record Storage evolution

- Linked local SQLite Storage shall support an **explicit, offline, authorized** transformation of Rust-derived application models, with pre-mutation verified backup, preserved UUIDs, checked revision fencing, exact source/target validation, and bounded recovery.
- Ordinary application startup remains fail-closed on incompatible stored models; automatic/online upgrades and remote/managed parity are not accepted.
- The [accepted model migration architecture](sqlite-offline-migration-architecture.md) defines the controlled single-host ownership, legacy process rollout and verification boundary. Within the explicitly bounded cooperative single-host Linux scope, the typed migration engine and the operator preview/confirmation/execute/restore workflow have passed their accepted implementation gates ([#50](https://github.com/aschenrot/riesel/issues/50), [#51](https://github.com/aschenrot/riesel/issues/51)). Broader reliability, uncooperative legacy writers, online evolution and cross-platform release guarantees remain separate. See the [roadmap](implementation-roadmap.md) and implementation [#50](https://github.com/aschenrot/riesel/issues/50)/[#51](https://github.com/aschenrot/riesel/issues/51).

## Planned infrastructure extensions

- Deliver the accepted bounded SQLite model-evolution workflow and separately plan any broader Storage support.
- Authentication, authorization, and protected remote transport.
- Operational visibility, configurable resource budgets, and reliable deployment management.
- Standalone manager workflows and independently executed native interfaces.
- Complete the technical specification gates for the **accepted operational product directions** ([decision boundary](operational-products-direction.md)): shared operational semantics [#75](https://github.com/aschenrot/riesel/issues/75), cross-project tooling [#68](https://github.com/aschenrot/riesel/issues/68), Manager [#59](https://github.com/aschenrot/riesel/issues/59) and native GUI [#60](https://github.com/aschenrot/riesel/issues/60). The product owner accepted these directions; review of this documentation change and acceptance of concrete APIs, trust/ownership guarantees, implementation milestones and releases remain separate decisions.

These outcomes require separate designs and acceptance criteria; they must not be treated as already implemented.

## Exploratory product directions

Game/ECS integrations, automatic multiplayer, subscription or messaging services, a custom UI framework, and a visual application builder may be developed as specialized packages or products if concrete requirements justify them. They are **not** prerequisites for the core runtime and do not automatically require new crates.

## Product-level success criteria

1. The same declared capability can be consumed through supported local and managed providers with equivalent contract behavior, verified by integration tests.
2. Applications can run headlessly and expose inspectable definitions with deterministic dependency and provider validation.
3. Ordinary example applications use typed authoring interfaces without repeating manual RPC or process-wiring code.
4. Session shutdown and recovery have documented, testable cancellation, identity, and side-effect boundaries.
5. Platform interfaces provide consistent operations where both interfaces are supported.
6. Future application types can extend the platform through contracts or specialized integrations without altering its core composition model.

An accepted product requirement is an intended outcome, **not** an implementation claim. Delivery is verified only when its implementation acceptance gate passes; broad product success criteria do not prove that every planned feature ships today.
