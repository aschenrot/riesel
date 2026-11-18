# Todo — Rust-first Riesel SDK example

Todo is the first reference application for Riesel's typed model,
generated capability RPC, and runtime lifecycle.

## Run locally — no JSON or TCP required

From the workspace root:

```sh
cargo run -p todo --bin todo
```

`TaskData` in `src/task.rs` is the authoritative data model.
`Application::new("Todo").records::<TaskData>()?.development()?`
registers the generated model metadata and selects ephemeral in-process
MemoryStore. This mode is **explicitly for development**, not persistent storage.

## Inspect the generated application definition

```sh
cargo run -p todo --bin todo_client -- --print-definition
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
generated contract before executing Todo.

Linux and Windows CI execute these tests.

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
cargo run -p todo --bin todo_client -- --deployment path/to/external.json
```

For a runtime-owned service, choose `transport: "managed"` and supply
the trusted compiled service executable in `program`. It chooses its own
loopback port and advertises it after binding. `cargo smoke` generates such
a deployment file with the correct platform-specific executable path.

The earlier `managed_tcp` and `managed_tcp_dynamic` names, along with the
old handwritten Storage TCP wire protocol, were intentionally **removed**.
Rebuild any clients and regenerate managed deployment files.

## Boundaries

Current Storage is in-memory, so state is lost on process termination.
The TCP transport is unencrypted and unauthenticated. Both the client and
Storage server enforce loopback addresses. Endpoint announcements and
contract verification do not authenticate a service. Use only trusted
managed executables and deployment files.
