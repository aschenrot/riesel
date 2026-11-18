# Todo — Rust-first Riesel SDK example

Todo is the first reference application for Riesel's typed model,
capability composition, and runtime lifecycle.

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

`cargo smoke` tests the real Todo and Storage binaries in two modes:

1. Legacy managed TCP with an explicitly selected temporary port. It
   verifies the listener closes after the app exits.
2. Two concurrent managed TCP instances whose Storage processes bind
   to `127.0.0.1:0`, letting the OS allocate distinct ports. The runtime
   reads a bounded, versioned `RIESEL_READY` message and verifies the
   running provider's contract.

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

For a runtime-owned service without a configured port, choose
`transport: "managed_tcp_dynamic"` and provide the trusted compiled
service executable in `program`. `cargo smoke` generates such a
deployment file with the correct platform-specific executable path.

The old `managed_tcp` endpoint mode remains available for compatibility,
but no fixed port is required for an in-process provider or dynamic managed
service.

## Boundaries

Current Storage is in-memory, so state is lost on process termination.
The TCP transport is unencrypted and unauthenticated. Dynamic endpoint
announcements and contract verification do not establish cryptographic
service identity. Use managed executables and deployment files only
when trusted, and do not expose the current TCP server to untrusted
networks.
