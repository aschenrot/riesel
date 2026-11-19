# Todo R2 — Local HTTP API and browser boundary

**Status: accepted and verified in the stated local-only scope; not production security certification.** [Specification Issue #36](https://github.com/aschenrot/riesel/issues/36) records the decision. The Todo HTTP implementation was delivered in [Issue #17](https://github.com/aschenrot/riesel/issues/17) / [PR #39](https://github.com/aschenrot/riesel/pull/39); the separate SvelteKit integration is [Issue #18](https://github.com/aschenrot/riesel/issues/18) / [PR #40](https://github.com/aschenrot/riesel/pull/40). This contract does not authorize exposing an unauthenticated server publicly.

## Outcome and existing constraints

Expose the **existing Rust `Todo` domain** to a local browser while its headless backend continues running independently of browser connections. Reuse Todo R1's explicit linked SQLite Storage and Riesel's application session.

- `Todo` already implements create/get/list/edit title/set completion/delete with revision-aware mutations. HTTP must not reimplement those rules or query SQLite directly.
- Riesel's `Application::async_handler` runs on a **current-thread Tokio executor** and owns session-scoped work. `Records<TaskData>` and `Todo` currently perform **synchronous** Store calls.
- Riesel's `AsyncTasks::spawn_blocking` is the existing bounded, session-owned blocking-work boundary. It does **not** directly return an HTTP response result; a request/response channel is needed.
- The initial release is **single-user local-browser access**. No remote users, login, TLS endpoint, accounts or public network bind are accepted.

## Research and alternatives (2026-11-19)

Evaluate candidates against these criteria: existing Tokio-session compatibility, routing/JSON ergonomics, graceful shutdown, dependency scope, Linux/Rust 1.100 compatibility and integration-test cost. Do not select by benchmarks or popularity.

| Approach | Relevant properties | Initial assessment |
| --- | --- | --- |
| **Axum 0.8** | Tokio/Hyper-based routing, JSON and body-limit extractors; existing `serve().with_graceful_shutdown()` | **Proposed first choice**; pin and validate actual dependency/toolchain versions |
| **Direct Hyper** | Control over HTTP/1 connections and graceful shutdown | Viable but requires more manual routing, extraction and connection lifecycle wiring for this small API |
| **Poem** | Tokio-based full web framework and graceful shutdown | Viable; more framework surface than needed for a few routes |
| **Actix Web** | Mature HTTP server with its own Actix runtime integration | Viable independently; separate runtime assumptions require more scrutiny inside Riesel's owned Tokio scope |

Evidence: [Axum serve](https://docs.rs/axum/latest/axum/serve/struct.Serve.html), [Axum body limits](https://docs.rs/axum/latest/axum/extract/struct.DefaultBodyLimit.html), [Hyper graceful shutdown](https://hyper.rs/guides/1/server/graceful-shutdown/), [Poem server](https://docs.rs/poem/latest/poem/struct.Server.html), [Actix runtime](https://docs.rs/actix-web/latest/actix_web/rt/).

At the initial specification decision, no integrated HTTP prototype had been validated. Subsequent pinned-toolchain, local-security, lifecycle and browser integration tests verified the selected Axum implementation. The original comparative assessment does not claim universal superiority.

## Local security and deployment contract

The threats are **cross-origin websites**, CSRF, DNS rebinding, unauthorized browser reads/writes, malformed requests and resource exhaustion. This is **not authentication against another untrusted local OS process**; such processes may forge HTTP headers.

1. **Bind only** `127.0.0.1` by default. Use an OS-assigned port unless an explicit local port is configured. Reject attempts to bind a public/LAN interface.
2. **Check `Host` on every request**, including static assets and reads: require the exact accepted numeric loopback host and bound port. Reject arbitrary DNS names, foreign `Host`, ambiguous authority and alternate ports. Reject missing Host for HTTP/1 requests.
3. **Unsafe methods** (`POST`, `PATCH`, `DELETE`) require `Origin` equal to the server's exact scheme/host/port. Reject missing, `null`, malformed or cross-origin values; if `Sec-Fetch-Site` is present, reject `cross-site` and `same-site`. Non-browser integration clients may supply the explicit accepted Origin.
4. **No cross-origin access:** do not emit permissive CORS headers or enable credentialed cross-origin requests. Do not use cookies or browser credential authentication for this local single-user slice. Serve R2 static frontend assets from the same authority as the API.
5. **Bound requests:** require JSON on writes, reject unexpected JSON fields, cap request bodies (proposed **16 KiB**), validate UUID/revisions and restrict methods; do not echo internal file paths or database errors to browser clients.
6. **No side effects on rejection:** enforce Host/Origin/method/body checks before calling domain commands. Return explicit status codes; avoid silently accepting unsupported methods or malformed JSON.

This protects against the specified browser-origin threats only under the stated assumptions. Origin/Host headers are **not** OS-user authentication. Defenses follow [OWASP CSRF guidance](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html), [OWASP DNS rebinding tests](https://wstg.owasp.org/latest/4-Web_Application_Security_Testing/11-Client-side/21-DNS_Rebinding/) and [GitHub's localhost security analysis](https://github.blog/security/application-security/localhost-dangers-cors-and-dns-rebinding/).

## Local API contract

Prefix: `/api/v1`. JSON field names use snake_case. The HTTP layer calls the existing typed `Todo` methods.

| Method/path | Request | Success |
| --- | --- | --- |
| `GET /tasks` | `after` optional UUID, `limit` bounded 1–32 | `200` `{records:[...],next_cursor:...}` |
| `GET /tasks/{id}` | UUID path | `200` record |
| `POST /tasks` | `{title:string}` | `201` record |
| `PATCH /tasks/{id}` | `{revision:string,title:string}` **or** `{revision:string,completed:bool}`, exactly one change | `200` updated record |
| `DELETE /tasks/{id}` | `{revision:string}` | `204` no body |

A record is `{id,revision,title,completed}`; `revision` is a **decimal string representing a full Rust `u64`** in both requests and responses, avoiding precision loss in JavaScript beyond `Number.MAX_SAFE_INTEGER`. The API rejects JSON numeric revisions and malformed/out-of-range decimal strings. This changes only the JSON wire representation, not optimistic concurrency semantics. List pagination uses the existing non-snapshot cursor contract. The HTTP adapter must preserve update/delete optimistic revision checks, **never** prefetch a new revision to silently retry. If pagination needs a new `Todo::list_tasks_page` helper, add it to the Rust domain rather than bypassing `Todo` in HTTP handlers.

All errors use `{error:{code,message}}` with stable short codes. Proposed mapping: `400` malformed input, `403` browser origin/Host rejected, `404` not found, `409` stale revision, `413` oversized, `415` wrong media type, `429/503` capacity/service unavailable, `500` unexpected backend failures. Route and error fixtures are exercised by the browser integration tests; client messages must not expose raw database paths.

## Session ownership and synchronous Storage

1. Start linked SQLite through the existing `Application::async_handler`; only then bind the listener. A failed bind must fail the application startup without leaving active provider work or an orphan listener.
2. Keep the server alive independently of individual HTTP connections. On **normal Ctrl-C**, initiate bounded graceful shutdown and join admitted Storage work before dropping providers. On **unexpected session revocation**, Riesel's current `run_scoped` drops the handler future promptly; do **not** promise that every HTTP connection drains or receives a response. Verify that started Storage work is joined before provider teardown, and interrupted requests are not silently replayed.
3. Do **not** invoke synchronous SQLite operations directly on the current-thread Tokio reactor. Use the existing **`AsyncTasks::spawn_blocking`** with a bounded per-request result channel (e.g. Tokio oneshot) and a unique task name; return capacity errors when admission fails.
4. Expected validation/conflict/not-found outcomes are **HTTP responses**, not `spawn_blocking` task failures that revoke the whole application session. The blocking closure should deliver the result and return `Ok(())` unless there is an actual fatal runtime error.
5. If a client disconnects, a previously admitted mutation may still commit. Do not infer a failed write from a closed HTTP connection or retry it automatically.
6. Verify the server's shutdown deadline, cancellation, ownership, and listener release with real application-session tests. Do not use detached `tokio::spawn_blocking` jobs or a second Tokio runtime.

The `AsyncTasks` scope currently permits at most eight admitted blocking jobs. Do not increase this limit or introduce a new core scheduler merely for Todo.

## Required verification before implementation acceptance

- **Security:** foreign Host (including rebinding-style name), missing/null/cross-site Origin, conflicting Fetch Metadata, cross-origin reads and malformed/missing content type are rejected; valid same-origin reads and writes succeed. Verify that rejected requests cannot mutate data.
- **Contract:** CRUD/paging, distinct invalid input, missing, and stale-revision responses; no duplicate title/completion validation or task state in HTTP. Require client-observed revision.
- **Resources:** oversized bodies, excessive simultaneous requests, blocking-budget exhaustion and cancellation behavior have deterministic bounded failures.
- **Lifecycle:** failed listener startup, normal Ctrl-C graceful server stop, abrupt session revocation, concurrent in-flight requests, browser disconnect/reconnect, and persistent SQLite state across backend restart. No Storage operation may outlive provider teardown; abrupt revocation does not guarantee completed HTTP responses.
- **Build:** supported pinned Rust toolchain, Linux `cargo validate --check` and `cargo smoke`, HTTP integration tests. The frontend must be able to use stable route fixtures independently of its visual design.

## Decision gate

Gate B acceptance is recorded in [Issue #36](https://github.com/aschenrot/riesel/issues/36). HTTP implementation #17 passed its security, response, bounded-blocking and lifecycle tests before merge; R2 frontend integration adds real Chromium workflows. These are bounded tests, not proof of protection against untrusted local processes, arbitrary network exposure or all production failure modes.

No general HTTP capability, browser auth product, real-time subscription, cross-host deployment, SvelteKit runtime dependency or generic Riesel networking abstraction is approved by this proposal.
