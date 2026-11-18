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

The `LocalRpc` transport connects those generated pieces in process.
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

It is a transport-independent reference implementation. The existing Storage
TCP protocol is maintained during migration; this example does not claim to
implement authenticated networking, automatic provider discovery or
production-ready RPC.
