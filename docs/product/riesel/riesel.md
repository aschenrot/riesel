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

### Contracts, identity, and interfaces

- Applications depend on versioned capability contracts rather than specific providers.
- Private identifiers can be derived by the framework; independently published contracts retain a deliberate stable external identity.
- CLI and programmatic interfaces should expose comparable supported platform operations with consistent error semantics.
- New capabilities and specialized execution integrations can be added without replacing the foundational application model.

## Product interfaces and delivery boundaries

| Interface | Intended role | Current boundary |
| --- | --- | --- |
| **Rust SDK** | Primary application and component authoring API | Experimental public `riesel` facade |
| **Application CLI** | Deployment selection, inspection, and application-specific commands | App-scoped inspection and runtime arguments exist; a full platform CLI is not implied |
| **Riesel Manager** | Registration, discovery, process supervision, and coordination | Existing `riesel_manager` crate is a process-ownership primitive, not a completed user-facing manager |
| **Manager UI** | Inspect and manage applications and infrastructure | Planned, not delivered |
| **Native window hosts** | Independent interfaces attached to application logic | Planned, not delivered |

The current reference Storage provider is ephemeral, and loopback TCP is not authenticated. Passing development tests does **not** establish production persistence, authenticated remote networking, or distributed recovery.

## Planned infrastructure extensions

- Durable Storage, schema evolution, and migration.
- Authentication, authorization, and protected remote transport.
- Operational visibility, configurable resource budgets, and reliable deployment management.
- Standalone manager workflows and independently executed native interfaces.

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

A requirement is accepted when its specific implementation gate passes; broad product success criteria are not proof that every planned feature ships today.
