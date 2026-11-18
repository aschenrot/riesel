# Counter capability — generated RPC example

This independent capability demonstrates that the Riesel `#[capability]`
macro produces a typed RPC client and a matching dispatcher from **one
authoritative Rust trait**.

The example defines `Counter` and `MemoryCounter` in `src/lib.rs`.
The macro generates:

- `CounterContract` — inspectable, versioned API contract
- `CounterRpcClient<T: RpcTransport>` — typed methods for calling the capability
- `CounterRpcDispatcher<T: Counter + Send + Sync>` — validates operation
  names and arguments and calls the provider implementation

`LocalRpc` connects the generated client and dispatcher in process.
`TcpRpcTransport` uses the same generated code over a bounded, JSON-line,
loopback-only TCP connection. A contract-inspection request checks the remote
API shape before typed calls. Tests cover both transports.

There are **no separately maintained request enums or operation tables**.

Run with:

```sh
cargo test -p riesel_counter_example
```

This initial generator handles synchronous trait operations, owned
serializable arguments, and borrowed `&str` inputs. The generated client
returns `CallError<E>` to distinguish wire/transport failures from the
domain-specific error `E`. The adapter is not automatically a trait
implementation, because this would require an explicit policy for converting
transport errors to every domain error type.

This is the first generic, transport-independent reference implementation.
The existing Storage TCP protocol is maintained during migration; this example
does not claim to implement authenticated networking, automatic provider
discovery, production-ready RPC or automatic Rust trait error conversion.
