# Todo — Rust-first Riesel SDK example

Todo is the first reference application for Riesel's typed model,
generated capability RPC, and runtime lifecycle.

**Actual R2 screenshots:** [verified Todo browser visual reference](../../docs/product/todo/visual-reference.md) (desktop/mobile, system light/dark, selection, Undo and permanent-delete confirmation). These are Playwright captures of the real Rust/SQLite app, not mockups.

## Local HTTP backend (Todo R2, development)

The optional Rust backend implements the accepted [local browser contract](../../docs/product/todo/http-local-browser-contract.md) without changing the R0 or R1 binaries:

```sh
cargo run --locked -p todo --features http --bin todo_http -- --db ./tasks.db --port 0
```

The process prints `TODO_HTTP_READY 127.0.0.1:PORT` once the database and listener are ready. It serves `/api/v1/tasks` with JSON operations; `revision` fields are decimal strings in requests and responses to preserve all `u64` revisions in browser clients. The backend survives browser disconnects. Ctrl-C requests graceful shutdown; abrupt runtime revocation does not guarantee HTTP responses have completed.

This first adapter is **loopback-only**, not a remote service or authenticated multi-user server. Every request must use the numeric loopback `Host` matching the announced port. POST, PATCH and DELETE additionally require `Origin: http://127.0.0.1:PORT` and JSON content type. Requests from foreign web origins, nonlocal Hosts and oversized bodies are rejected; `Access-Control-Allow-Origin` is not enabled. Host/Origin checks protect the stated browser threat boundary, not against arbitrary local OS clients able to forge request headers.

Todo's SvelteKit frontend is built separately. Pass `--assets ./examples/todo/web/build` to the same `todo_http` process to serve generated static assets and the API from one origin, without a production Node process. Asset directories must be trusted and contain `200.html`; invalid paths fail before HTTP readiness. The API is opt-in via the `http` Cargo feature; normal R0/R1 execution remains unchanged.

## Usable persistent CLI — Todo R1

The separate `todo_cli` executable is a headless task manager. Specify the
database path explicitly on **every invocation**, for example:

```sh
cargo run --locked -p todo --bin todo_cli -- --db ./tasks.db add "Build Riesel"
cargo run --locked -p todo --bin todo_cli -- --db ./tasks.db list
cargo run --locked -p todo --bin todo_cli -- --db ./tasks.db show UUID
cargo run --locked -p todo --bin todo_cli -- --db ./tasks.db edit UUID REV "Revised title"
cargo run --locked -p todo --bin todo_cli -- --db ./tasks.db complete UUID REV
cargo run --locked -p todo --bin todo_cli -- --db ./tasks.db reopen UUID REV
cargo run --locked -p todo --bin todo_cli -- --db ./tasks.db delete UUID REV
```

Replace `UUID` and `REV` with values returned by `add`, `show`, or `list`.
Each task prints its identifier, revision, completion state, and escaped title
as tab-separated fields; successful deletion prints the removed identifier.
Edit, complete, reopen, and delete **require the revision observed by the
caller** and reject stale writes. Run `todo_cli --help` for command syntax.

The path must refer to a writable local file under an existing directory.
The CLI uses the opt-in `sqlite-storage` feature and the same Rust `Todo`
domain operations as the in-memory reference example. It does **not** fall
back to MemoryStore when the database fails to open. It does not promise
power-loss immunity, managed Storage, browser access or multi-user safety.
Invalid command arguments exit with code 2; domain or Storage failures
exit with code 1. Successful operations exit with code 0.

The original `todo` executable below remains the independent **R0
in-memory reference and managed-process smoke fixture**.

## Run locally — no JSON or TCP required

From the workspace root:

```sh
cargo run -p todo --bin todo
```

`TaskData` in `src/task.rs` is the authoritative data model.
The domain exposes `Record<TaskData>` directly rather than duplicating
identifier, revision, and payload fields in a second Task structure.
`Application::new("Todo").handler(run_demo)?.development()?`
registers the model and Storage requirement from the typed
`Records<TaskData>` function parameter, then selects ephemeral in-process
MemoryStore. This mode is **explicitly for development**, not persistent
storage. The Todo package enables the SDK's `dev-memory-storage` Cargo
feature explicitly; Riesel itself no longer enables it by default.
For multiple Storage bindings, use `records_at::<T>("alias")` in the
application builder and Context.

## Inspect the generated application definition

```sh
cargo run -p todo --bin todo -- --print-definition
```

The Rust SDK generates a portable JSON application schema for the CLI,
tooling, and future visual builder. There is no manually synchronized
`app.json` in this example.

## Run the end-to-end smoke tests

```sh
cargo validate --check
cargo smoke
```

`cargo smoke` starts **two concurrent** Todo clients with independently
managed Storage services. Both services bind `127.0.0.1:0`, letting the OS
allocate ports without a pre-bind reservation race. The runtime reads a
bounded `RIESEL_READY` announcement and verifies each provider identity and
generated contract before executing Todo. It also exercises multi-service
rollback: when a dependent managed process cannot spawn, the already running
Storage process must be terminated. A separate smoke check verifies that
Storage exits normally on the managed runtime's private stdin shutdown request.

Linux CI runs these tests automatically. Windows validation remains
available manually in the Validate workflow and is required before release.

## Inspect a deployment before launching services

```sh
cargo run -p todo --bin todo -- --inspect-deployment path/to/deployment.json
```

This prints a JSON plan with the selected provider endpoint and dependency
order, without creating Storage processes or opening network connections.
It validates the local application definition and connector declarations,
**not** the real identity or availability of an external provider.

## Remote or managed deployments

To run against an *externally managed* Storage instance, first launch it:

```sh
cargo run -p riesel_storage_component --bin storage_service -- 127.0.0.1:47911
```

Then create an optional file containing an explicit remote endpoint:

```json
{
  "services": {
    "storage": {
      "transport": "tcp",
      "address": "127.0.0.1:47911"
    }
  }
}
```

Use it with:

```sh
cargo run -p todo --bin todo -- --deployment path/to/external.json
```

For a runtime-owned service, choose `transport: "managed"` and supply
the trusted compiled service executable in `program`. It chooses its own
loopback port and advertises it after binding. `cargo smoke` generates such
a deployment file with the correct platform-specific executable path.

The earlier `managed_tcp` and `managed_tcp_dynamic` names, along with the
old handwritten Storage TCP wire protocol, were intentionally **removed**.
Rebuild any clients and regenerate managed deployment files.

## Boundaries

The R0 `todo` reference uses in-memory Storage, so its state is lost on
process termination. The separate R1 `todo_cli` uses explicit linked SQLite
and retains task state across supported restarts.
The TCP transport is unencrypted and unauthenticated. Both the client and
Storage server enforce loopback addresses. Endpoint announcements and
contract verification do not authenticate a service. Use only trusted
managed executables and deployment files.

## Storage v2 example behavior

The reference application uses cursor-paginated storage, revision-checked
title edits, completion updates and deletion. `Todo::list_tasks` collects pages for the small
example dataset, while callers handling larger collections can use
`Records::<TaskData>::list_page` with a bounded page size. Development
Storage enforces the registered `TaskData` model shape on provider writes;
a standalone managed reference provider does not receive that model registry.

The `records` SDK feature is optional for applications that only use
custom capability contracts. Todo opts into `dev-memory-storage`, which
includes `records`. Todo's `edit_title`, `set_completed` and `delete_task`
require the revision last observed by the caller. Storage compares that
revision atomically with the stored record and rejects stale writes rather
than silently refreshing the revision and overwriting a concurrent edit.
This is a domain/API correctness improvement, not durable persistence.
