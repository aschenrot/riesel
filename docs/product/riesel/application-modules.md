# Riesel — Optional Application Modules

Status: Stage 5.7 **partially implemented**, module registration verified
on Linux, 2026-11-19. CI:
[44512577572](https://github.com/aschenrot/riesel/actions/runs/44512577572).

## Purpose and boundary

An `ApplicationModule` is a **reusable group of ordinary Rust application
registrations**, not a provider, process, component, scheduler or new
dependency-injection container. It calls the existing
`Application::component`, `async_rpc`, `records_at` and `depends_on`
methods. Its registrations produce the same canonical `AppSchema`,
`ConnectorRegistry` and `DeploymentPlan` as a manually assembled
application.

Module registration order does **not** determine process startup. The
capability requirement dependency DAG remains authoritative, so explicitly
declared `depends_on` constraints determine startup order regardless
of which module was registered first.

## Minimal composition

```rust
use riesel::{Application, ApplicationModule, AppResult, app};
use riesel_counter::{AsyncCounterHandle, AsyncCounterObserverHandle};
use riesel_counter_component::AsyncMemoryCounter;

struct CounterProvider;
impl ApplicationModule for CounterProvider {
    fn register(app: Application) -> riesel::Result<Application> {
        app.component::<AsyncMemoryCounter>()
    }
}

struct CounterDependencies;
impl ApplicationModule for CounterDependencies {
    fn register(app: Application) -> riesel::Result<Application> {
        app.depends_on("observer", ["counter"])
    }
}

#[app(modules = [CounterDependencies, CounterProvider])]
async fn main(
    #[riesel(alias = "counter")] counter: AsyncCounterHandle,
    #[riesel(alias = "observer")] observer: AsyncCounterObserverHandle,
) -> AppResult<()> {
    counter.add(2).await?;
    assert_eq!(observer.observe().await?, 2);
    Ok(())
}
```

The same module types can be used with
`Application::new(...).module::<CounterProvider>()?` and with typed
prepared handlers. Module identity comes from the Rust type path, so no
manual module ID is required. Re-registering a module type fails before
startup; declaring the same provider twice is rejected by the existing
provider registry. A module returning an unrelated newly built
`Application` is rejected instead of silently discarding prior modules.

This composition layer is optional. For small applications, registering
components directly is still the simpler and recommended API.

## Remaining lifecycle design gate

Stage 5.7 is **not complete**. Module registration does not automatically
provide lifecycle hooks. The next acceptance slice must preserve all of
the following:

- **Startup ordering:** validate capability dependencies before launching
  any provider; run start hooks only in a prepared, owned application
  session and use deterministically declared ordering rules
- **Failure propagation:** retain both the original entrypoint/hook error
  and any cleanup failures; no silent fallback or automatic command replay
- **Cancellation:** scope asynchronous hooks and spawned work to the
  current session; a managed child failure must revoke their handles
- **Shutdown:** run eligible stop hooks before provider state is destroyed,
  then close and join owned work before stopping children; define a
  cooperative timeout contract and process fallback for non-cooperative code
- **Recovery:** opt-in whole-application restart must create fresh hook and
  provider scope; never reuse expired handles or silently replay commands

This is a specification for the **remaining work**, not a claim that
`on_start`, `on_stop` or a scheduler already exist. Reuse
`RunningApp`, `AsyncTasks` and the existing supervisor; do not add a
second lifecycle coordinator merely because modules are optional.
