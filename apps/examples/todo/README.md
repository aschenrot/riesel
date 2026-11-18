# Todo — remote Storage composition

## Automated integration test

From the repository root, run:

```sh
cargo smoke
```

This builds both executables, starts Storage on a temporary loopback port
through Riesel's application runtime, generates a temporary deployment config,
runs Todo through the real TCP transport, and verifies that the runtime stops
its managed service when the application exits.
CI runs this check on Linux and Windows.

## Manual execution

This example declares `riesel.storage.records@1` in `app.json`, but keeps
transport details in `deployment.json`. The application does not import the
MemoryStore implementation or hardcode a TCP endpoint.

From the repository root, in terminal 1:

```sh
cargo run -p riesel_storage_component --bin storage_service -- 127.0.0.1:47911
```

Then, in terminal 2:

```sh
cargo run -p todo --bin todo_client
```

To change the endpoint, edit `apps/examples/todo/deployment.json` or pass a
different file to `todo_client`:

```sh
cargo run -p todo --bin todo_client -- path/to/deployment.json
```

The client discovers the running provider, resolves the capability requirement,
checks the remote Rust-derived contract, and binds a typed `RecordStore`
implementation before running Todo operations.

**Limitations:** This example uses unauthenticated, unencrypted TCP on loopback,
a manually started service, and in-memory data that is lost when the service
stops. Provider advertisements are not cryptographically authenticated.
It is a composition demonstration, not a production deployment.

## Runtime-owned service mode

A trusted deployment can configure `transport: "managed_tcp"` instead of
`"tcp"` and provide `program`, `args`, `address`, and optionally
`startup_timeout_ms` (default: 10000, maximum: 120000).

The runtime launches that executable (without invoking a shell), verifies
the service's declared Rust capability contract before handing it to Todo,
and terminates the child on shutdown or failed startup. Managed endpoints
are restricted to loopback because this initial TCP protocol has no
authentication or encryption. Relative executable paths resolve from the
launching process's current directory, not from `app.json`.

`cargo smoke` is the recommended fully automated example on Windows and Linux.
`deployment.json` remains in externally managed TCP mode.
