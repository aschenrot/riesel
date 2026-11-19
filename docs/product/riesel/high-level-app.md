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
