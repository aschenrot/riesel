# Todo — Product Definition

## Purpose and role

Todo is a task-management application built with Riesel and a reference case for testing the platform's model, Storage, and application-runtime APIs.

Three delivery modes remain distinct:

- **R0 reference:** a one-shot in-memory `todo` example for framework regression checks.
- **R1 headless manager:** persistent `todo_cli` commands using explicitly configured linked SQLite.
- **R2 local browser application:** long-lived `todo_http` Rust backend, persistent SQLite, and optional SvelteKit static assets served on the same local origin.

The R2 interface is a Todo product integration, not a generic Riesel web framework or permission for remote deployment.

## User problem

Users need a small, dependable way to record and complete tasks. Developers need a realistic example that exercises Riesel without requiring handwritten process or RPC infrastructure for each operation.

Task data must survive restarts for R1/R2 usable-product acceptance; the reference R0 example intentionally remains ephemeral.

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

**Limitations:** R0 loses data on restart. R1 and R2 use opt-in linked SQLite and remain single-host local deployments; neither is a general production-durability certification.

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

### R2 — Independent graphical interface (delivered for single-user local use)

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

**Implementation:** SvelteKit builds static browser assets. The `todo_http` Rust executable hosts the same-origin UI and JSON API with explicit linked SQLite Storage, using the existing Todo domain and Riesel session-owned blocking work. The host is loopback-only and independent of any browser connection. The frontend uses revision strings to preserve the complete Rust `u64` domain, with no silent conflict retries. This is an application-specific delivery, not a generic Riesel HTTP framework.

**Deferred:** PWA installation, offline editing/synchronization, push notifications, real-time subscriptions, desktop packaging, and remote-user deployment.

Local-only security, lifecycle and API rules: [accepted HTTP integration boundary](http-local-browser-contract.md). Backend [Issue #17](https://github.com/aschenrot/riesel/issues/17) was delivered in [PR #39](https://github.com/aschenrot/riesel/pull/39). Browser [Issue #18](https://github.com/aschenrot/riesel/issues/18) is delivered through [PR #40](https://github.com/aschenrot/riesel/pull/40), with Chromium end-to-end tests against the real Rust/SQLite host. See the [Todo example](../../../examples/todo/README.md) for execution and limitations. Remote authentication, public deployment and power-loss guarantees are **not** part of R2.

## Accepted Todo R2 interaction evolution (2026-11-21)

The next browser usability increment makes title editing and deletion directly discoverable without a three-dot overflow menu. Completion remains a dedicated checkbox; multi-selection uses a distinct explicit selection mode with Shift-range and Ctrl/Cmd additions for mouse/keyboard, and is also usable through touch buttons. Batch Mark done/Reopen operate only on selected **displayed, loaded tasks**, never on unseen database matches; report success, unchanged tasks, revision conflicts and partial failures honestly. Existing individual revision-aware APIs remain authoritative, with no silent retry or fake atomic batch.

**Current destructive-operation limit:** the existing DELETE API permanently deletes records, so it requires an explicit confirmation and cannot be called Trash or Undo. A separate ephemeral Undo action may reverse **confirmed** title/completion mutations (including the successful subset of a batch), fenced by the server-returned post-command revision, with no silent retry or persistent history. Recoverable Trash, full Undo, optional Archive and touch swipe shortcuts are separate specification/implementation work (#119 and #120). **Status-aware pagination is now accepted but not yet delivered** under #107. User-controlled System/Light/Dark settings across dynamic ports remain #106. None of those features may be claimed delivered by the basic interaction increment.

## Accepted Todo R2 status-aware Query experience (#107, 2026-11-22)

**Status: owner-accepted and delivered for the bounded R2 local-browser scope (22 November 2026).** The product owner accepted the [#107 native Query recommendation](https://github.com/aschenrot/riesel/issues/107#issuecomment-7075937597) after reproducible linked Memory/SQLite research [PR #190](https://github.com/aschenrot/riesel/pull/190). The linked Todo Rust domain/HTTP implementation is delivered by [#207 / PR #209](https://github.com/aschenrot/riesel/pull/209), and the Open-first/Completed browser implementation by [#208 / PR #210](https://github.com/aschenrot/riesel/pull/210). [The first native Record Query profile](../riesel/record-query-architecture.md) and [accepted usage semantics](../riesel/record-query-usage.md) remain independently versioned Riesel infrastructure, not a Todo-only fake filter.

**Interaction:** replace the current All/Open/Done tabs (which only filter already loaded UUID pages) with two real status-scoped sections. Show **Open** first; **Completed** starts collapsed, can be expanded using keyboard, mouse or touch, and has its own loading/progress/errors. Neither section displays an invented global count; "N shown" or "N loaded" always means only currently loaded rows. Do not imply chronological "recently completed" ordering: both sections use the supported live ascending UUID ordering. A section with no matches in the *inspected portion* must display **still checking**, never a definitive empty state until its Query reports `exhausted=true`. Initial viewport and keyboard focus, drafts and selection stay usable as rows move between sections. Selected items and Shift-range operations include only currently displayed, loaded rows.

**Read architecture:** both `Records<TaskData>` and the typed, read-designated `ModelRead<TaskData, RecordQueryContract>` attach to **one actual linked SQLite owner in the same Riesel application session**. The Todo domain owns its restricted `completed Eq bool` selector. The HTTP boundary exposes a backward-compatible *additive* read-only status-scoped API, not arbitrary filtering or transport permissions. Existing R0 in-memory and R1 CLI constructors and commands, Storage v2, task identity, decimal-string revision concurrency, lifecycle, persistence and loopback browser defenses stay compatible. A Query-only reader never implies a mutation grant or cross-process publication.

**Bounded and truthful discovery:** one provider call returns at most 32 matched rows and inspects at most 256 candidates; empty *partial* pages are expected. A single owned HTTP read may combine at most **four** consecutive bounded native reads (at most **1,024** examined candidates), stopping when it finds a nonempty page or exhausts the source. Never loop indefinitely in a server handler, inflate provider budgets, accidentally discard matched rows, manufacture totals or treat `inspected` as a total. The browser may automatically continue sequential bounded GET requests to seek actionable tasks, with a finite per-refresh discovery allowance designed to cover the researched 10,000-record sparse case without repeated user-required clicks under normal responses; allow a user to pause/continue beyond that allowance and show accurate progress. A cancelled/lost HTTP GET is an unavailable observation, not a complete empty result or proof the consumed one-use cursor can be replayed. There is **no** snapshot, latency SLA, or guarantee that live tasks moved behind an existing UUID cursor will be seen without refreshing from the start.

**State and mutation safety:** Open and Completed have separate cursor, partial/exhausted, failure and scan-generation state. Their opaque one-use tokens are bound to the predicate/limits and current provider instance, expire and are invalidated by restarts; an invalid/expired token must offer an explicit fresh scan, not silently claim no records. Confirmed completion/reopening moves a row between the sections and triggers safe list reconciliation. Never visually commit an uncertain mutation, silently retry a write, override a newer task revision, lose a pending edit draft, silently clear selected failures or present a partial batch as atomic success. An expired background response cannot overwrite a new refresh or section state.

**Performance and future evolution:** verify 33/100/10,000 sparse and mutation-behind-cursor cases on native Memory/SQLite and through actual Rust HTTP + Chromium, reporting end-to-end observations without passing off in-process Query timing as browser latency. If a measured sparse-use case is unacceptable, reopen a **provider-owned indexed status access path** under a separately accepted profile and SQLite offline migration/rollback; no silent index DDL, dual authority or speculative general-purpose engine is authorized by #107. Future Trash/Archive [#119](https://github.com/aschenrot/riesel/issues/119) needs active-location filtering in addition to `completed`; the current single-field Query predicate is **not** sufficient to hide trashed records. Resolve compound Query/domain authority at that separate specification gate, without baking active/trash policy into this first status selector.

**Delivery evidence:** the bounded native linked Rust/SQLite HTTP implementation [PR #209](https://github.com/aschenrot/riesel/pull/209) merged on protected `main` at `09b11adb`, and the Open-first Chromium UI [PR #210](https://github.com/aschenrot/riesel/pull/210) merged at [`8519ab0d`](https://github.com/aschenrot/riesel/commit/8519ab0da03453b9f67c224ae01433b062de4a4a). The synchronized UI candidate passed [Linux Validate/smoke](https://github.com/aschenrot/riesel/actions/runs/45001384673), [Todo Web including real Chromium and retained visual evidence](https://github.com/aschenrot/riesel/actions/runs/45001384674), and [runtime-ingress verification](https://github.com/aschenrot/riesel/actions/runs/45001384670). In a reproducible single CI-run measurement, the offline-seeded 10,000-record SQLite dataset with only the final UUID record Open required ten bounded native-status HTTP requests, examined 10,000 records and displayed one Open task in **514 ms** through Chromium; this is contextual evidence, not an imposed browser latency SLA or stable snapshot guarantee. Future Trash/Archive [#119](https://github.com/aschenrot/riesel/issues/119), compound active-status predicates and provider-native status indexing remain separate decisions.

## Scope boundaries

Todo is a product-level acceptance example for reusable Riesel infrastructure. Implement generic functionality in Riesel or a reusable capability package when multiple applications need it; keep task-specific behavior within Todo.

Do not add synchronization, collaborative editing, reminders, or advanced task organization to the committed scope without a concrete requirement and separate acceptance criteria.
