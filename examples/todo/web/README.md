# Todo browser interface

Application-specific SvelteKit frontend for [Todo R2](../../../docs/product/todo/todo.md). It calls the accepted [local HTTP contract](../../../docs/product/todo/http-local-browser-contract.md) and does not implement its own task persistence or domain validation.

**Real UI screenshots:** [Todo R2 verified visual reference](../../../docs/product/todo/visual-reference.md) — dated and versioned desktop/mobile/light/dark, selection, feedback and deletion captures from the actual Rust/SQLite browser E2E suite.

## Development

Requires Node.js 22.12 or later and the delivered Rust Todo HTTP backend. Builds are static; runtime operation uses Rust, not Node.

1. Start the Rust API with an explicit persistent SQLite path. Its port is assigned by the OS and printed on startup.
2. In this directory run npm ci.
3. Set TODO_API_ORIGIN in .env to the printed http://127.0.0.1:PORT (see .env.example).
4. Run npm run dev and open the displayed 127.0.0.1 Vite address.

The local Vite proxy forwards /api but translates Origin only for requests genuinely originating from that same local Vite page. It preserves Fetch Metadata so cross-site browser requests remain denied. This exists **only for local development**; the production build must be served from the same authority as the API. No fixed backend port, permissive CORS, remote access, browser authentication, or separate production Node server is assumed.

## Validation and output

Run `npm run check`, `npm test`, and `npm run build`. For full Rust/browser integration, build the Rust backend and the **opt-in offline test fixture** with `cargo build --locked -p todo --features http,e2e-fixture --bin todo_http --bin todo_seed_status`, install Chromium once with `npx playwright install --with-deps chromium`, then run `npm run test:e2e`. The runner starts a real backend on an OS-assigned port and isolated SQLite file, runs ordinary CRUD/status browser tests (including 33/100 sparse records), then stops that backend. It seeds a **different offline SQLite file** through the feature-gated Rust fixture before launching a second backend for the 10,000-record sparse Chromium measurement. Each host is shut down after its tests; the stress suite uses a separate test-results directory to preserve the visual-review screenshots. The fixture is not a production Todo command or public HTTP seed endpoint. The static app is generated into build/ via adapter-static; this directory is gitignored. Start the Rust host with `--assets ./examples/todo/web/build` after building and use its printed `127.0.0.1:PORT` address to test the actual single-origin application. No production Node process is required.

All mutations include the last observed task revision as a decimal string (not a JavaScript number), preserving the entire Rust u64 range. A stale write is surfaced as a conflict, not retried. A mutation whose network response is lost or unreadable is shown as **outcome unknown** (it may have committed), not a definite disconnection/refusal; the client never resends the command. The user can explicitly **Check current tasks** with a read-only refresh, and an uncertain edit draft is retained until the user explicitly chooses **Discard draft and check tasks**. The backend's confirmed pre-dispatch `unavailable` error remains distinct from a lost response. The Open-first and initially collapsed Completed sections use independent, status-scoped, read-only native Query continuations: at most 32 matches and 1,024 inspected candidates per HTTP page. A sparse scan may return **zero matches but still be partial**. The browser progresses automatically through at most ten sequential bounded reads (10,240 candidate inspections) before offering Continue checking; Stop checking aborts the active request, invalidates its one-use cursor and offers a fresh scan. Only a reported exhausted scan can support an empty-state message. Counts describe loaded/shown rows, never global totals, and the live UUID order is not chronological or a snapshot. An invalid/expired/consumed cursor requires a new read from the beginning. No automatic write retry follows a failed read. No PWA installation, offline writes, subscriptions, or synchronization is included.

**Delivery boundary:** Same-origin static serving and Chromium integration tests against a real long-lived Rust/SQLite host are included. This is local single-user use only; remote access, user authentication, production power-loss certification and offline synchronization remain excluded.

## Interface and accessibility

Todo R2 uses a compact responsive task list, direct, keyboard/touch-accessible edit and delete controls, inline create/edit validation and a reusable top-positioned success toast. The toast is a polite live-region announcement; it automatically dismisses after a short period, pauses on hover or focus, and can be dismissed manually. Revision conflicts and connectivity errors remain persistent, actionable in the task panel, and never trigger automatic command retries. The current Delete action is permanent and retains explicit confirmation; recoverable Trash and Undo for deletion are not delivered (see #119). Confirmed title and completion changes can be reversed with an immediate, revision-checked Undo action (#124).

The upper-left circular-arrow control checks for task changes on demand, spins during the GET request (except when reduced motion is preferred), and displays the browser-local time of the last successful list-page fetch as **Last checked**. Failed refreshes leave that timestamp unchanged and retain existing rows while showing an actionable error. The label deliberately does **not** claim continuous or bidirectional synchronization; manual refresh starts a fresh bounded Open scan (and refreshes Completed only when expanded); Continue checking resumes a partial status-scoped scan. Editing a task cannot silently trigger a refresh that discards an unsaved draft.

The **Open** section is always first; the independent **Completed** section is initially collapsed and does not fetch its records until expanded. Both may be partial. The native `riesel.query.records@1` provider evaluates status in the same linked SQLite owner as mutations; the frontend does **not** filter only the first 32 unfiltered records. Counts remain loaded-only. Riesel does not claim creation-order sorting, real-time synchronization or an authorized remote UI. The real browser 10,000-task test prints requests, inspected candidates and measured elapsed time as context, **not a performance SLA**. If a future measured workload requires provider-native indexes, they require their own compatibility and offline migration gate.

Browser regressions include responsive overflow, keyboard actions, toast feedback and the existing mutation/revision-conflict scenarios. Visual and screen-reader checks still require a human review of the rendered result.

## System appearance (first delivery, #105)

Todo's browser interface follows the operating-system/browser `prefers-color-scheme` preference, with semantic light/dark colors and native `color-scheme` support. Light/dark switches take effect without reload. System is currently the **only** appearance policy: Todo does not yet expose a manual Light/Dark override or claim persistence of such a choice. That separate requirement is specified in [#106](https://github.com/aschenrot/riesel/issues/106) because `--port 0` may give the browser a different origin after a restart. Do not use per-origin `localStorage` as proof of durable cross-port settings. Open-first status paging was accepted in [#107](https://github.com/aschenrot/riesel/issues/107) and delivered through linked backend [#207](https://github.com/aschenrot/riesel/issues/207) and the browser implementation [#208](https://github.com/aschenrot/riesel/issues/208), independent of the still-open appearance-preference decision.

## Direct controls and loaded-task batch actions (#118)

Task titles and pencil icons open inline editing. The trash icon opens an explicit confirmation for **permanent** deletion until the separate Trash specification is implemented. Completion remains a dedicated 44px checkbox target, never the whole row.

The **Select tasks** control activates a distinct selection mode. Selection checkboxes replace completion checkboxes; Shift-click selects a range within the currently displayed/loaded tasks, Ctrl/Cmd-click on a title can enter selection, Escape cancels, and Ctrl/Cmd+A selects only visible, loaded tasks outside editable text fields. A section-collapse or successful refresh clears selection. A `Select all loaded` control does not select unseen cursor pages.

Batch **Mark done / Reopen** uses sequential, revision-aware existing API PATCH requests. Each mutation may succeed independently; there is **no transaction** across selected tasks, no automatic retry after an uncertain result and no global `all matching tasks` command. A partial failure is explicitly reported and failed tasks remain selected. Reconcile another client's revision conflicts with a deliberate refresh before attempting a new operation. Batch Delete/Trash and durable Undo history remain deferred pending #119. Confirmed batch completion changes expose an immediate Undo of successful items; partial failures remain visible and are never silently retried.

## Immediate revision-safe Undo (#124)

After a successful title edit, completion/reopen or batch completion change, the success toast offers **Undo** for approximately seven seconds; hover/focus pauses dismissal. Undo is a one-action, in-memory opportunity—not a durable history. The inverse PATCH uses the server-returned revision of the successful operation: newer edits by another client cause an explicit conflict instead of a silent overwrite. An unsuccessful or uncertain write does not grant Undo.

Batch Undo compensates only individually confirmed successes and may itself partially fail. Such failures are reported and require deliberate refresh/reconciliation; a batch is not an atomic transaction. If a batch partly failed, its successful subset can be undone explicitly from the persistent batch result. Existing DELETE is **permanent**, has no Undo, and remains confirmed until the separate Trash lifecycle (#119) is implemented.
