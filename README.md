# Riesel

Riesel is an experimental Rust-first, schema-driven application runtime.
Applications declare versioned capabilities and bind them to local, external,
or runtime-managed providers. The runtime handles provider validation,
generated typed RPC, managed child startup, and dependency-ordered teardown.

**Status:** reference applications work; production persistence, authenticated
remote transport, continuous service supervision and UI tooling are not
implemented. Do not use the in-memory Storage reference as durable storage.

## Run and validate

The workspace is pinned to Rust 1.100.0. From the repository root:

```sh
cargo run -p todo --bin todo
cargo run -p todo --bin todo_client -- --print-definition
cargo validate --check
cargo smoke
```

The Todo example derives its model directly from Rust and opts in to an
ephemeral local MemoryStore for development. The smoke suite exercises
managed TCP Storage and multi-service failure cleanup on Linux and Windows.

## Defining lifecycle dependencies

Call `Application::rpc::<Contract>("alias")` to declare a generated RPC
capability. Once an alias is declared, use
`Application::depends_on("consumer", ["storage"])` to require `storage`
to become ready before `consumer` starts. Dependencies are validated for
missing aliases and cycles. They govern startup and reverse teardown order,
not provider discovery or background recovery.

## Documentation

- [Implementation roadmap](docs/product/riesel/implementation-roadmap.md)
- [Product definition](docs/product/riesel/riesel.md)
- [Todo example](apps/examples/todo/README.md)

Riesel is under active development and intentionally permits breaking API
changes during its initial architecture phase.
