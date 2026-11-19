# Todo browser interface

Application-specific SvelteKit frontend for [Todo R2](../../../docs/product/todo/todo.md). It calls the accepted [local HTTP contract](../../../docs/product/todo/http-local-browser-contract.md) and does not implement its own task persistence or domain validation.

## Development

Requires Node.js 22.12 or later and the Rust Todo HTTP backend from Issue #17.

1. Start the Rust API with an explicit persistent SQLite path. Its port is assigned by the OS and printed on startup.
2. In this directory run npm ci.
3. Set TODO_API_ORIGIN in .env to the printed http://127.0.0.1:PORT (see .env.example).
4. Run npm run dev and open the displayed 127.0.0.1 Vite address.

The local Vite proxy forwards /api but translates Origin only for requests genuinely originating from that same local Vite page. It preserves Fetch Metadata so cross-site browser requests remain denied. This exists **only for local development**; the production build must be served from the same authority as the API. No fixed backend port, permissive CORS, remote access, browser authentication, or separate production Node server is assumed.

## Validation and output

Run npm run check, npm test, and npm run build. The static app is generated into build/ via adapter-static; this directory is gitignored. Start the Rust host with `--assets ./examples/todo/web/build` after building and use its printed `127.0.0.1:PORT` address to test the actual single-origin application. No production Node process is required.

All mutations include the last observed task revision as a decimal string (not a JavaScript number), preserving the entire Rust u64 range. A stale write is surfaced as a conflict, not retried. Lists request at most 32 tasks per page using the backend's opaque cursor. No PWA installation, offline writes, subscriptions, or synchronization is included.

**Delivery status:** This frontend is a separate workstream. The static asset serving integration and browser end-to-end tests depend on the accepted and merged Todo HTTP backend implementation.
