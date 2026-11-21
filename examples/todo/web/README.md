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

Todo R2 uses a compact responsive task list, a keyboard/touch-accessible actions menu, inline create/edit validation and a reusable top-positioned success toast. The toast is a polite live-region announcement; it automatically dismisses after a short period, pauses on hover or focus, and can be dismissed manually. Revision conflicts and connectivity errors remain persistent, actionable in the task panel, and never trigger automatic command retries. Remove requires explicit confirmation; Undo is not offered.

The All/Open/Done filters and visible counts operate on **loaded cursor pages only**. Loading additional pages may change those totals; they are not a global task count. Todo does not claim creation-order sorting, real-time synchronization or an authorized remote UI.

Browser regressions include responsive overflow, keyboard actions, toast feedback and the existing mutation/revision-conflict scenarios. Visual and screen-reader checks still require a human review of the rendered result.
