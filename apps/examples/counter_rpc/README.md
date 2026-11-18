# Counter — independent capability and reusable component

Counter verifies that Riesel's Rust-generated RPC and application lifecycle
are not specific to Storage.

| Layer | Package | Responsibility |
| --- | --- | --- |
| Contract | `crates/riesel_counter` | `Counter` trait, domain error, generated client/dispatcher |
| Implementation | `components/counter` | In-memory Counter, local factory, executable server |
| Runtime | `crates/riesel_app`, `crates/riesel_protocol` | Provider selection, managed launch, shared TCP host |
| Example | `apps/examples/counter_rpc` | Local application, managed RPC client, contract tests |

## Run locally

```sh
cargo run -p riesel_counter_example --bin riesel_counter_example
```

The application registers `CounterContract` and `MemoryCounterFactory`
directly through the Rust SDK. No JSON, TCP endpoint, per-operation RPC client,
or explicit process management is necessary.

## Inspect the generated definition

```sh
cargo run -p riesel_counter_example --bin counter_client -- --print-definition
```

## Test managed Counter

```sh
cargo smoke
```

The cross-platform smoke suite builds `counter_service` and `counter_client`,
generates a trusted managed deployment with an OS-assigned loopback port,
and verifies the typed `value`, `add`, and `label` operations end to end.
It also runs the local application.

Counter uses the same `run_tcp_rpc_host` implementation as Storage for the
versioned readiness message, loopback binding and cooperative shutdown.

**Boundaries:** This is in-memory and non-durable. Managed TCP remains
plaintext and unauthenticated on loopback. Restarting a crashed provider does
not yet transparently rebind or recover existing client handles.
