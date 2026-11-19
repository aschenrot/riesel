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

## Session lifecycle hooks (Stage 5.7)

Modules can optionally register async `Application::on_start` and
`Application::on_stop` handlers. An ordinary application does not need hooks.
They use the same typed parameter declaration and alias extraction as
`Application::async_handler`; use `on_start_with_aliases` and
`on_stop_with_aliases` when the default requirement alias is not suitable.
The high-level macro composes module hooks through the existing builder
without introducing new macro options.

```rust
impl ApplicationModule for CounterLifecycle {
    fn register(app: Application) -> riesel::Result<Application> {
        app.on_start_with_aliases(
            "counter_session",
            |counter: AsyncCapability<AsyncCounterContract>| async move {
                counter.add(1).await?;
                Ok::<(), CallError<CounterError>>(())
            },
            ["counter"],
        )?
        .on_stop_with_aliases(
            "counter_session",
            |counter: AsyncCapability<AsyncCounterContract>| async move {
                let _ = counter.value().await?;
                Ok::<(), CallError<CounterError>>(())
            },
            ["counter"],
        )
    }
}
```

Each name is unique within its phase; the same name pairs start/stop hooks.
Duplicate or invalid names are rejected during registration. Repeated
`depends_on` declarations are additive, not silently overwritten, and
duplicate dependency edges fail validation. Capability dependency cycles,
invalid bindings and provider configuration fail before provider startup.

After all providers are ready, start hooks run by the **latest required
capability's position** in the existing dependency DAG. Independent hooks
retain registration order. The entrypoint runs after all start hooks succeed.
Stop hooks run in reverse of their corresponding dependency/declaration
order while their provider handles remain usable. A stop-only hook runs on
normal completion. If a start hook fails, only completed start hooks'
matching stop hooks run; the entrypoint is skipped.

The async task scope closes admission, signals cancellation and joins owned
work before invoking stop hooks. Stop hooks cannot start new background
tasks. After those hooks, the session token expires and the normal runtime
tears down providers and managed processes. If a managed child or an owned
task has already invalidated the session, capability-dependent stop hooks
are skipped rather than given replacement handles. Old cloned handles do
not become valid in a restarted session.

Hook errors preserve phase, name and source in `HookFailure` and
`LifecycleFailure`, surfaced through `AppRunError::Lifecycle`.
Entry and hook-cleanup errors remain separately inspectable. Hooks do not
make ordinary business errors eligible for automatic recovery.

No universal deadline or forced termination is promised for Rust hooks.
A caller must use bounded/cooperative operations; cancelling an async
future cannot undo commands already sent. Managed children retain their
configured graceful deadline and process-kill fallback. Recovery replays
start hooks in a fresh session; callback closures may still capture
application-owned state outside Riesel, so callers must explicitly handle
non-idempotent side effects.

The synchronous SDK still has no implicit Tokio dependency; registered
async hooks require an async application handler. See
[runtime lifecycle](runtime-lifecycle.md) and
[execution contract](async-execution-contract.md).
