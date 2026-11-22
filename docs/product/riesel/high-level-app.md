# Riesel — High-level Application Authoring

Status: `#[riesel::app]` Stage 5.6 verified on Linux (2026-11-19).
CI: [44510693321](https://github.com/aschenrot/riesel/actions/runs/44510693321).

The accepted model remains **application uses capabilities; component
implements capabilities; Riesel composes and runs them**. The attribute
generates registrations through the same builder, component registry,
deployment preflight and runtime as explicit applications.

## Minimal native async application

```rust
use riesel::{app, AppResult};
use riesel_counter::AsyncCounterHandle;
use riesel_counter_component::AsyncMemoryCounter;

#[app(name = "Counter", components = [AsyncMemoryCounter])]
async fn main(counter: AsyncCounterHandle) -> AppResult<()> {
    assert_eq!(counter.add(3).await?, 3);
    println!("{}", counter.value().await?);
    Ok(())
}
```

There is no manually constructed Tokio runtime, RPC connector or provider
handle. A generated `AsyncCounterHandle` is transport-independent:
linked providers are called in process, and selected managed providers use
the native nonblocking RPC transport.

The macro creates the ordinary synchronous `main` executable wrapper and
runs the user async function under Riesel's session-owned Tokio executor.
`AppResult<()>` preserves underlying domain, transport, cancellation and
session errors through their error-source chain. Recovery is opt-in and
replays whole application sessions, not individual commands.

## Explicit aliases for separate requirements

```rust
#[riesel::app(components = [AsyncMemoryCounter])]
async fn main(
    #[riesel(alias = "primary")] first: AsyncCounterHandle,
    #[riesel(alias = "secondary")] second: AsyncCounterHandle,
) -> riesel::AppResult<()> {
    first.add(2).await?;
    assert_eq!(second.value().await?, 2);
    Ok(())
}
```

Aliases are stable application requirement names, not Rust variable names.
Two aliases can share the same linked provider state. Ambiguous providers
and conflicting requirements fail validation before startup.

## Typed Records and optional development provider

```rust
use riesel::{app, AppResult, Records};
use todo::TaskData;

#[app(name = "Todo", development)]
async fn main(tasks: Records<TaskData>) -> AppResult<()> {
    todo::run_demo(tasks)?;
    Ok(())
}
```

This requires the SDK's `async-tcp` and `dev-memory-storage` Cargo
features. `development` is **explicit opt-in** for in-memory Storage.
Declaring `Records<T>` without a suitable provider does not create one.

Other supported built-in parameter types include `AsyncTasks`,
`AsyncCapability<Contract>`, and synchronous `Capability<Contract>`
when their contracts and session modes are compatible. For an application
parameter type alias or a custom type already implementing
`ApplicationParam`, use `#[riesel(parameter)]` to bypass the generated
native handle adapter.

## Model metadata without writable Storage

A Rust-derived record model can also be registered **independently** of
Storage v2 mutation authority:

```rust
use riesel::Application;
use todo::TaskData;

let declared = Application::new("MetadataOnly").model::<TaskData>()?;
let schema = declared.definition_json()?;
```

The inspectable schema contains the model definition but **no Storage
requirement**, provider selection or read permission. This helper does **not**
execute queries or publish a read endpoint. The separately accepted
[linked Record Query architecture](record-query-architecture.md) requires a
future read-designated Query capability and a validated model/data-owner
relationship before reading. Existing `Records<T>` and
`records_at<T>` remain the explicit **writable Storage v2** path.

## Typed linked record readers (schema/owner foundation)

The accepted linked Record Query architecture uses the existing high-level
typed-parameter path to declare model metadata and a **read-only capability**
without writable Storage authority. The SDK foundation has two equivalent
authoring routes:

- `ModelRead<T, ReaderContract>` in a typed handler derives a model declaration,
  one reader capability requirement and the model's linked Query relationship.
- `Application::record_query_at::<T, ReaderContract>("read")` expresses the
  same registration through a builder. Adding `Records<T>` or
  `records_at::<T>("writer")` associates that writer with the model and
  requires both to resolve to one actual linked provider/session. Declaration
  order does not change the schema. Explicit different providers fail
  preflight instead of returning inconsistent reads.

The generated reader contract must expose **Query operations only**; a
read-only Rust wrapper over writable Storage v2 is not accepted. A model
declaration without a writer is not a cross-process access grant. Schema
version 2 represents the linked relationship while old version 1 Storage
applications retain their existing serialized definition.

**Delivery boundary:** this SDK/schema/owner registration mechanism is
independently testable, but the actual provider-executed typed Record Query
contract, Memory/SQLite filtering and bounded cursors remain [#161](https://github.com/aschenrot/riesel/issues/161).
No existing Storage v2 method becomes a read-only Query API simply by
registering a model. Remote/managed readers remain unsupported under this
linked-only consistency profile. See the
[accepted technical contract](record-query-architecture.md). The
[proposed typed Query usage and bounded-paging profile](record-query-usage.md)
describes the separate `RecordQueryContract` implementation under review,
including typed `QueryPage<T>`, provider permissions and non-snapshot cursors.

## Explicit linked SQLite Storage (Stage 6)

For durable local records, enable the SDK's `sqlite-storage` Cargo feature
and select SQLite explicitly after registering the typed record models:

```rust
use std::path::PathBuf;
use riesel::Application;
use todo::run_demo;

fn main() -> riesel::Result<()> {
    Application::new("Todo")
        .handler(run_demo)?
        .sqlite_storage(PathBuf::from("todo.db"))?
        .run_from_env()
}
```

The database file path is explicit; its parent directory must exist. Riesel
uses the Rust-derived model definitions already declared by the handler,
validates them on startup and refuses incompatible persisted metadata.
Provider registration and deployment inspection do not create the file.
`sqlite_storage` requires one unambiguous `RecordStore` requirement and
rejects multiple storage aliases rather than silently assigning a durable
database to the wrong model. `development` remains an explicitly ephemeral
alternative, not a fallback. This initial linked provider is not a managed
Storage service and does not certify host power-loss durability.

## Approved catalog choices

The macro may register the approved placement catalog from a normal Rust
factory:

```rust
#[app(
    name = "Counter",
    components = [AsyncMemoryCounter],
    catalog = approved_counter_providers
)]
async fn main(counter: AsyncCounterHandle) -> AppResult<()> {
    counter.add(3).await?;
    Ok(())
}
```

The user-supplied factory returns `Result<ProviderCatalog, E>`, where
`E` converts into `SdkError`. For each approved managed/external
placement, `ProviderCatalog::register_component::<MyComponent>(choice,
endpoint)` derives the provider definition from the same authoritative
`#[component]` declaration rather than requiring a duplicate manually
written provider ID and capability list.

By default the unambiguous statically linked component runs locally.
`--select-providers selections.json` can choose a predeclared managed or
operator-owned loopback placement without changing application code.
`--inspect-catalog --select-providers selections.json` prints the
side-effect-free startup plan with pinned published provider metadata.
See [provider catalog](provider-catalog.md) for the manifest contract.

Without a linked component or explicitly selected deployment, the
application **fails** rather than silently creating a provider.
`--deployment deployment.json`, `--inspect-deployment deployment.json`
and `--print-definition` remain available for direct configurations.

## Current constraints and extension points

- The attribute is for a concrete safe `async fn` returning a `Result`;
  it supports up to four typed, named function parameters, consistent with
  the current typed handler. Generic entrypoints, arbitrary destructuring
  and mixed sync/async operations in one capability are not supported.
- Component registration is explicit; the attribute does not discover
  implementation types across crates or the network.
- Only approved loopback plain TCP endpoints are currently supported.
  Advertised provider contract metadata does not authenticate peers.
- Use the direct `Application` builder and `Context` for dynamic/library
  scenarios that require more control. Both use the same schema and resolver.
- Optional `ApplicationModule` composition and named, typed async start/stop
  hooks are implemented within Stage 5.7. They use the existing dependency DAG
  and session owner; no general scheduler is included. See
  [application modules](application-modules.md).
- Production remote security, persistence and observability belong to Stage 6.

The Counter, alias, managed, remote-only, catalog and Todo examples are
executable regression binaries. Their generated schemas and startup
plans are compared with equivalent explicit builder declarations on Linux.
