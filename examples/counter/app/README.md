# Counter — independent capability and reusable component

Counter verifies that Riesel's Rust-generated RPC and application lifecycle
are not specific to Storage.

| Layer | Package | Responsibility |
| --- | --- | --- |
| Contract | `examples/counter/contract` | `Counter` trait, domain error, generated client/dispatcher |
| Implementation | `examples/counter/component` | In-memory Counter, local factory, executable server |
| Runtime | `crates/riesel_app`, `crates/riesel_protocol` | Provider selection, managed launch, shared TCP host |
| Example | `examples/counter/app` | Local application, managed RPC client, contract tests |

## Run locally

```sh
cargo run -p riesel_counter_example --bin riesel_counter_example
```

The application registers the typed `Capability<CounterContract>` parameter
through `Application::handler_with_aliases`, then selects
`memory_counter_provider()` for local execution. No separate `.rpc()` call or
manual `Context::capability()` lookup is required. Alternatively,
`Application::provide::<CounterContract, _>("riesel.counter.memory", || ...)`
registers an implementation without handwritten provider/factory metadata.
No JSON, TCP endpoint, per-operation RPC client or explicit process management
is needed.

## Inspect the generated definition

```sh
cargo run -p riesel_counter_example --bin riesel_counter_example -- --print-definition
```

## Test managed Counter

```sh
cargo smoke
```

The smoke suite builds `counter_service`, `riesel_counter_example`,
`counter_crash_fixture`, and `counter_recovery_client`. It runs the local
application, verifies typed managed RPC operations, then forces the first
Counter process to crash and verifies that a new application session binds
to its replacement without reusing a stale endpoint. Linux runs in required
CI; Windows can be checked manually in the Validate workflow.

Counter now exposes a second, independently versioned
`CounterObserverContract`. Both local and managed components bind it to the
same `Arc<MemoryCounter>` as the writable `CounterContract`. The managed
component uses a `RpcRouter` to serve both over one socket and process.
`cargo smoke` includes a dual-capability typed handler that confirms a write
through Counter is observed through Observer from one explicitly named
managed instance.

Counter uses the same `run_component_from_env` helper as Storage for
readiness, bind-address selection and cooperative shutdown. Its capability contract is **version 2**.
Generated `Arc<dyn Counter>` handles reject calls after their application
session expires.

**Boundaries:** Counter is in-memory and non-durable; managed TCP remains
plaintext and unauthenticated on loopback. Opt-in recovery rebuilds the
entire application session; it does not live-rebind an existing Counter
client. Restarted state begins at zero and business operations may replay.

## Project organization

The contract, component implementation and application live together in
`examples/counter/{contract,component,app}`; no Counter crate is part of
Riesel's framework core. The application has one local or deployment-aware
binary. The crash, recovery and shared-instance binaries are explicit
`tests/fixtures` targets, rather than normal application commands.
`ComponentDefinition` produces both local provider registration and managed
RPC hosting from one capability list.
