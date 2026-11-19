# Todo local HTTP API

This is an application-specific adapter for [Todo R2](../../docs/product/todo/todo.md), delivered before the graphical frontend in [Issue #18](https://github.com/aschenrot/riesel/issues/18). Task state and validation belong to the existing Rust Todo domain and the explicitly selected Riesel Storage provider.

## Run

From the workspace root:

~~~sh
cargo run --locked -p todo --bin todo_http -- --db ./tasks.db
~~~

The required `--db` path selects linked SQLite. The optional `--port PORT` defaults to `0` (OS-assigned); read the actual `http://127.0.0.1:PORT` address from stderr. The parent directory must exist. The process remains running independently of browser connections and exits cleanly on Ctrl-C or Unix SIGTERM, allowing Riesel to tear down Storage.

This first delivery exposes only JSON API routes. It does **not** serve a browser UI yet; Issue #18 supplies the static frontend.

## Endpoints

All paths are relative to the reported origin. UUID and revision are taken from the latest successful response.

| Method | Path | JSON request body | Result |
| --- | --- | --- | --- |
| `GET` | `/api/tasks` | none | Array of tasks |
| `GET` | `/api/tasks/{uuid}` | none | Task |
| `POST` | `/api/tasks` | `{"title":"Example"}` | 201 + task |
| `PATCH` | `/api/tasks/{uuid}` | `{"expected_revision":1,"title":"Edited"}` **or** `{"expected_revision":1,"completed":true}` | Updated task |
| `DELETE` | `/api/tasks/{uuid}` | `{"expected_revision":1}` | 204 |

The JSON task representation is `{"id":"UUID","revision":1,"title":"Example","completed":false}`. PATCH changes **exactly one** field; send the caller-observed `expected_revision` for both PATCH and DELETE. Stale writes fail without modifying current state.

Errors use `{"error":"code"}`: malformed input (400/415), invalid title (422), missing task (404), revision conflict (409), forbidden browser origin or host (403), and unavailable Storage (503). The adapter does not expose internal database error messages to browsers.

## Local security boundary

The listener binds only to `127.0.0.1`. Requests must carry the exact `Host: 127.0.0.1:PORT`; mutations require an exact `Origin: http://127.0.0.1:PORT`, JSON content type, and a bounded request body. Cross-site fetch metadata is rejected. No cross-origin access is enabled.

These controls address unintended cross-site browser requests and DNS rebinding; they **do not authenticate other local processes**. Localhost access is therefore intended for a trusted single-user machine, not an untrusted shared host or publicly exposed service. No reverse proxy, remote HTTP access, CSRF bypass, authorization, PWA synchronization or production security guarantee is delivered.

The first frontend will be hosted on the same origin rather than enabling permissive CORS. The API currently collects the small Todo task list in memory; large dataset pagination and resource budgets remain future reliability work.
