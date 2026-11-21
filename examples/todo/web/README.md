# Todo browser interface

Application-specific SvelteKit frontend for [Todo R2](../../../docs/product/todo/todo.md). It calls the accepted [local HTTP contract](../../../docs/product/todo/http-local-browser-contract.md) and does not implement its own task persistence or domain validation.

## Development

Requires Node.js 22.12 or later and the delivered Rust Todo HTTP backend. Builds are static; runtime operation uses Rust, not Node.

1. Start the Rust API with an explicit persistent SQLite path. Its port is assigned by the OS and printed on startup.
2. In this directory run npm ci.
3. Set TODO_API_ORIGIN in .env to the printed http://127.0.0.1:PORT (see .env.example).
4. Run npm run dev and open the displayed 127.0.0.1 Vite address.

The local Vite proxy forwards /api but translates Origin only for requests genuinely originating from that same local Vite page. It preserves Fetch Metadata so cross-site browser requests remain denied. This exists **only for local development**; the production build must be served from the same authority as the API. No fixed backend port, permissive CORS, remote access, browser authentication, or separate production Node server is assumed.

## Validation and output

Run `npm run check`, `npm test`, and `npm run build`. For full Rust/browser integration, build the Rust backend with `cargo build --locked -p todo --features http --bin todo_http`, install Chromium once with `npx playwright install --with-deps chromium`, then run `npm run test:e2e`. The runner starts a real backend with an OS-assigned port, provides an isolated SQLite database and shuts the process down after the browser tests. The static app is generated into build/ via adapter-static; this directory is gitignored. Start the Rust host with `--assets ./examples/todo/web/build` after building and use its printed `127.0.0.1:PORT` address to test the actual single-origin application. No production Node process is required.

All mutations include the last observed task revision as a decimal string (not a JavaScript number), preserving the entire Rust u64 range. A stale write is surfaced as a conflict, not retried. Lists request at most 32 tasks per page using the backend's opaque cursor. No PWA installation, offline writes, subscriptions, or synchronization is included.

**Delivery boundary:** Same-origin static serving and Chromium integration tests against a real long-lived Rust/SQLite host are included. This is local single-user use only; remote access, user authentication, production power-loss certification and offline synchronization remain excluded.

## Interface and accessibility

Todo R2 uses a compact responsive task list, direct, keyboard/touch-accessible edit and delete controls, inline create/edit validation and a reusable top-positioned success toast. The toast is a polite live-region announcement; it automatically dismisses after a short period, pauses on hover or focus, and can be dismissed manually. Revision conflicts and connectivity errors remain persistent, actionable in the task panel, and never trigger automatic command retries. The current Delete action is permanent and retains explicit confirmation; recoverable Trash and Undo for deletion are not delivered (see #119). Confirmed title and completion changes can be reversed with an immediate, revision-checked Undo action (#124).

The upper-left circular-arrow control checks for task changes on demand, spins during the GET request (except when reduced motion is preferred), and displays the browser-local time of the last successful list-page fetch as **Last checked**. Failed refreshes leave that timestamp unchanged and retain existing rows while showing an actionable error. The label deliberately does **not** claim continuous or bidirectional synchronization; manual refresh fetches the first page, and Load more fetches further pages. Editing a task cannot silently trigger a refresh that discards an unsaved draft.

The All/Open/Done filters and visible counts operate on **loaded cursor pages only**. Loading additional pages may change those totals; they are not a global task count. Todo does not claim creation-order sorting, real-time synchronization or an authorized remote UI.

Browser regressions include responsive overflow, keyboard actions, toast feedback and the existing mutation/revision-conflict scenarios. Visual and screen-reader checks still require a human review of the rendered result.

## System appearance (first delivery, #105)

Todo's browser interface follows the operating-system/browser `prefers-color-scheme` preference, with semantic light/dark colors and native `color-scheme` support. Light/dark switches take effect without reload. System is currently the **only** appearance policy: Todo does not yet expose a manual Light/Dark override or claim persistence of such a choice. That separate requirement is specified in [#106](https://github.com/aschenrot/riesel/issues/106) because `--port 0` may give the browser a different origin after a restart. Do not use per-origin `localStorage` as proof of durable cross-port settings. The completed-task section and status-aware paging remain separate research in [#107](https://github.com/aschenrot/riesel/issues/107).

## Direct controls and loaded-task batch actions (#118)

Task titles and pencil icons open inline editing. The trash icon opens an explicit confirmation for **permanent** deletion until the separate Trash specification is implemented. Completion remains a dedicated 44px checkbox target, never the whole row.

The **Select tasks** control activates a distinct selection mode. Selection checkboxes replace completion checkboxes; Shift-click selects a range within the currently displayed/loaded tasks, Ctrl/Cmd-click on a title can enter selection, Escape cancels, and Ctrl/Cmd+A selects only visible, loaded tasks outside editable text fields. A filter change or successful refresh clears selection. A `Select all loaded` control does not select unseen cursor pages.

Batch **Mark done / Reopen** uses sequential, revision-aware existing API PATCH requests. Each mutation may succeed independently; there is **no transaction** across selected tasks, no automatic retry after an uncertain result and no global `all matching tasks` command. A partial failure is explicitly reported and failed tasks remain selected. Reconcile another client's revision conflicts with a deliberate refresh before attempting a new operation. Batch Delete/Trash and durable Undo history remain deferred pending #119. Confirmed batch completion changes expose an immediate Undo of successful items; partial failures remain visible and are never silently retried.

## Immediate revision-safe Undo (#124)

After a successful title edit, completion/reopen or batch completion change, the success toast offers **Undo** for approximately seven seconds; hover/focus pauses dismissal. Undo is a one-action, in-memory opportunity—not a durable history. The inverse PATCH uses the server-returned revision of the successful operation: newer edits by another client cause an explicit conflict instead of a silent overwrite. An unsuccessful or uncertain write does not grant Undo.

Batch Undo compensates only individually confirmed successes and may itself partially fail. Such failures are reported and require deliberate refresh/reconciliation; a batch is not an atomic transaction. If a batch partly failed, its successful subset can be undone explicitly from the persistent batch result. Existing DELETE is **permanent**, has no Undo, and remains confirmed until the separate Trash lifecycle (#119) is implemented.
