# Todo — remote Storage composition

## Automated integration test

From the repository root, run:

```sh
cargo smoke
```

This builds both executables, starts Storage on a temporary loopback port
through Riesel's `ProcessRegistry`, generates a temporary deployment config,
runs Todo through the real TCP transport, and stops the managed service.
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
