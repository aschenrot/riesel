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

The smoke suite builds `counter_service`, `counter_client`,
`counter_crash_fixture`, and `counter_recovery_client`. It runs the local
application, verifies typed managed RPC operations, then forces the first
Counter process to crash and verifies that a new application session binds
to its replacement without reusing a stale endpoint. Linux runs in required
CI; Windows can be checked manually in the Validate workflow.

Counter uses the same `run_tcp_rpc_host` implementation as Storage for the
versioned readiness message, loopback binding and cooperative shutdown.

**Boundaries:** Counter is in-memory and non-durable; managed TCP remains
plaintext and unauthenticated on loopback. Opt-in recovery rebuilds the
entire application session; it does not live-rebind an existing Counter
client. Restarted state begins at zero and business operations may replay.
