# Todo — Product Definition

## Purpose and role

Todo is a task-management application built with Riesel and a reference case for testing the platform's model, Storage, and application-runtime APIs.

Two outcomes must remain distinct:

- **Reference example (R0):** demonstrate typed task operations through Riesel's ephemeral development Storage and SDK.
- **Usable headless task manager (R1):** create, inspect, modify and complete tasks across CLI invocations using explicitly configured linked SQLite.

The in-memory `todo` executable remains a one-shot regression example. The separate `todo_cli` executable is the persistent headless interface; the browser interface remains R2.

## User problem

Users need a small, dependable way to record and complete tasks. Developers need a realistic example that exercises Riesel without requiring handwritten process or RPC infrastructure for each operation.

Task data must survive restarts **before** Todo can claim the planned usable-product milestone.

## Domain model

A task has a title and completion state. Its record identity and revision are managed by Riesel's record abstraction; callers should not maintain duplicate infrastructure fields.

- **Title:** required, non-empty text.
- **Completed:** boolean, initially false.
- **Record identity:** framework-generated and stable within the configured storage lifecycle.
- **Revision:** used to detect conflicting updates or deletes.

Additional domain fields should be introduced only for concrete user requirements.

## R0 reference baseline

The `examples/todo` implementation presently demonstrates:

- Rust-derived `TaskData` schema and typed `Records<TaskData>` access.
- Creating, retrieving, and listing tasks.
- Marking tasks complete or incomplete.
- Deleting tasks using revision-aware Storage operations.
- Automatic record identifiers and revision-controlled updates.
- Headless execution using explicit in-memory development Storage.
- Tests for validation, missing tasks, paging, and revision conflicts.

**Limitations:** This R0 example loses data when its memory provider is restarted. R1 uses a separate linked SQLite executable, not the reference provider. Neither mode currently includes the planned graphical interface.

## Delivery milestones

### R0 — Platform reference (delivered)

**Goal:** validate Riesel's developer-facing authoring and Storage contract.

Acceptance:
- Typed task operations and model validation pass relevant unit/integration tests.
- SDK-based application execution requires no hand-maintained app schema or fixed provider port.
- Documentation clearly states the ephemeral state and trust boundaries.

### R1 — Usable headless task manager (delivered in `todo_cli`)

**Goal:** deliver task operations with durable state and usable interfaces.

Acceptance:
- Users can create, retrieve, list, complete/uncomplete, edit, and delete tasks through supported commands.
- Task identity is automatic and stable; conflicting modifications are detected rather than silently overwritten.
- Tasks persist across application and provider restarts, verified through restart tests.
- Supported CLI and programmatic operations exhibit the same task semantics.
- Common failure cases have defined errors and regression tests.

R1 uses the explicitly configured linked SQLite provider, not MemoryStore. Cross-process CLI tests cover persistence and revision conflicts; the R0 demo remains independent. This is a **single-host, local-disk delivery** and does not certify host power-loss safety, a managed durable provider or a remotely accessible service.

### R2 — Independent graphical interface (planned)

**Goal:** let users manage tasks through a browser-based graphical interface while Todo's Rust application logic remains authoritative.

Acceptance:
- Users can view, create, edit, complete/uncomplete, and delete tasks through the interface using the same domain rules as supported headless clients.
- The UI does not own authoritative task state or reimplement Rust business validation. Other supported clients observe consistent task state on refresh; real-time subscriptions are not required.
- Mutations based on stale record revisions are rejected and presented as conflicts, not silently applied to newer task state.
- The browser client can open, close, and reconnect without implicitly terminating the long-lived headless backend.
- Validation, revision conflicts, and connectivity failures are understandable; the interface supports responsive layouts and keyboard interaction.
- Browser access has a defined, tested security boundary; a development-only local interface does not imply permission to expose an unauthenticated public server.
- Essential task workflows are covered by browser/API integration tests.
- R1's persistent Storage and restart acceptance criteria must pass before R2 can be marked a usable product. A clearly identified ephemeral development prototype may precede R1.

**Initial implementation:** SvelteKit as a browser frontend, a small Todo-specific Rust HTTP adapter, and static frontend assets served from the same origin for the initial local deployment. This is a delivery choice, not a requirement that Riesel adopt SvelteKit or a generic web server abstraction.

**Deferred:** PWA installation, offline editing/synchronization, push notifications, real-time subscriptions, desktop packaging, and remote-user deployment.

Local-only browser API and lifecycle contract: [accepted HTTP integration boundary](http-local-browser-contract.md). Implementation still requires security and lifecycle tests.

Delivery work: [Todo browser API and backend lifecycle](https://github.com/aschenrot/riesel/issues/17) and [Todo SvelteKit browser interface](https://github.com/aschenrot/riesel/issues/18).

## Scope boundaries

Todo is a product-level acceptance example for reusable Riesel infrastructure. Implement generic functionality in Riesel or a reusable capability package when multiple applications need it; keep task-specific behavior within Todo.

Do not add synchronization, collaborative editing, reminders, or advanced task organization to the committed scope without a concrete requirement and separate acceptance criteria.
