# Hello — the minimal Riesel application

From the workspace root:

```sh
cargo run -p riesel_hello_example
```

The main binary demonstrates Rust-first application authoring without schema
JSON, manual deployment configuration, connector registries, or a TCP service.

The separate lower-level example uses explicit schema and deployment APIs:

```sh
cargo run -p riesel_hello_example --bin schema_hello
```
