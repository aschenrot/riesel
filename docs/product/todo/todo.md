# Todo — Product Definition

## Purpose and role

Todo is a task-management application built with Riesel and a reference case for testing the platform's model, Storage, and application-runtime APIs.

Two outcomes must remain distinct:

- **Reference example (current):** demonstrate that typed task operations work through Riesel's development Storage and application SDK.
- **Usable task manager (planned):** let users create, inspect, modify, and complete tasks across restarts and supported interfaces.

The current example is not a persistent, user-facing task manager. Its execution path is primarily a one-shot demonstration and regression target.

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

## Current reference baseline

The `examples/todo` implementation presently demonstrates:

- Rust-derived `TaskData` schema and typed `Records<TaskData>` access.
- Creating, retrieving, and listing tasks.
- Marking tasks complete or incomplete.
- Deleting tasks using revision-aware Storage operations.
- Automatic record identifiers and revision-controlled updates.
- Headless execution using explicit in-memory development Storage.
- Tests for validation, missing tasks, paging, and revision conflicts.

**Limitations:** Data is lost when the in-memory provider is replaced or restarted. The example does not currently provide interactive task-management CLI commands, a persistent backend, or a graphical interface. It therefore does not satisfy the planned persistence and multi-interface requirements.

## Delivery milestones

### R0 — Platform reference (current scope)

**Goal:** validate Riesel's developer-facing authoring and Storage contract.

Acceptance:
- Typed task operations and model validation pass relevant unit/integration tests.
- SDK-based application execution requires no hand-maintained app schema or fixed provider port.
- Documentation clearly states the ephemeral state and trust boundaries.

### R1 — Usable headless task manager (planned)

**Goal:** deliver task operations with durable state and usable interfaces.

Acceptance:
- Users can create, retrieve, list, complete/uncomplete, edit, and delete tasks through supported commands.
- Task identity is automatic and stable; conflicting modifications are detected rather than silently overwritten.
- Tasks persist across application and provider restarts, verified through restart tests.
- Supported CLI and programmatic operations exhibit the same task semantics.
- Common failure cases have defined errors and regression tests.

This milestone depends on a production-appropriate persistent Storage implementation and should not be marked complete using MemoryStore alone.

### R2 — Independent graphical interface (planned)

**Goal:** attach a GUI to running headless Todo application logic.

Acceptance:
- The UI is independently executable and does not own the task data model.
- GUI and programmatic/CLI clients observe consistent task changes.
- Closing a window does not implicitly terminate the application unless the user requests it.

## Scope boundaries

Todo is a product-level acceptance example for reusable Riesel infrastructure. Implement generic functionality in Riesel or a reusable capability package when multiple applications need it; keep task-specific behavior within Todo.

Do not add synchronization, collaborative editing, reminders, or advanced task organization to the committed scope without a concrete requirement and separate acceptance criteria.
