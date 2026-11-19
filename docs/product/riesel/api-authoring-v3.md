# Riesel — High-level Application Authoring Contract (v3)

Decision: **accepted as the application authoring contract** on
2026-11-18. Stages 5.1–5.6 have been implemented and Linux-verified;
later lifecycle/module features remain targets. For current executable
examples and limits, see [high-level app authoring](high-level-app.md).
The mental model is **application uses capabilities; component implements
capabilities; Riesel composes and runs them**.

## Target surface (conceptual syntax)

```rust
use riesel::prelude::*;
use counter::CounterHandle;

#[riesel::app(components = [MemoryCounter])]
async fn main(counter: CounterHandle) -> AppResult<()> {
    counter.add(3).await?;
    println!("{}", counter.value().await?);
    Ok(())
}
```

The `components` list makes an implementation available when statically
linked. Without it, configured managed/external providers may satisfy the
same capability. An application signature declares each dependency exactly
once. A simple one-shot entrypoint is distinct from long-lived lifecycle
handlers and scheduled systems. Module composition is optional.

A capability author should be able to write `#[riesel::capability]` for a
private contract. For published contracts, one stable externally recognized
ID must be retained across Rust renames, via explicit `id = "..."` initially
and potentially package metadata in a later milestone. Contract versions are
separate from identity; private contracts default to version 1, and published
contract versions must change intentionally with breaking wire changes.

The component author writes ordinary Rust trait implementations and declares
provided contracts once. An attribute macro cannot discover arbitrary
impls across unrelated crates; use an explicit contract list or an equally
explicit registration annotation. Generate local adapters, managed RPC
routes and provider metadata from that single registration. Constructors
must support explicit factories; `Default` is only the convenience case.

## Identity and dependency rules

- A private capability identity is deterministic for its fully qualified
  Rust declaration path (module + trait name) and does not require a manual
  ID. It is **not a stable external compatibility guarantee**; moving or
  renaming the Rust declaration changes it.
- Independently deployed/published contracts require a deliberately stable
  external ID. Do not silently change existing Counter or Storage IDs.
- Component definition identity, running instance identity, requirement alias
  and capability contract identity are four different concepts. Never
  combine them or infer them from a process port or variable name.
- Parameter names are not stable dependency identifiers. Unambiguous
  capabilities receive one deterministic default binding; multiple instances
  require explicit binding aliases. Ambiguity and conflicting registrations
  fail during validation, before starting providers.
- Local component availability requires static linking/registration; remote
  and managed providers require explicit trusted configuration. No global
  hidden constructors, magic linker discovery or silent in-memory fallback.
- Rust-generated application definitions remain inspectable, validated,
  portable metadata. Rust functions and closures are not serialized as schema.

## Calls, execution, failure and security

- Generated handles should be ergonomically named `CounterHandle`, using a
  shared type-safe binding implementation, not a second runtime.
- Native async capabilities, typed generated local/remote handles, nonblocking
  Tokio transport, local/managed component parity, bounded async/blocking
  tasks and supervised async startup are implemented and Linux-verified.
  Do not wrap blocking network I/O in an async function or run untracked
  blocking work on Tokio; use the explicit provider blocking-work boundary.
  The high-level `#[riesel::app]` macro remains Stage 5.6.
- Preserve separate domain versus transport/cancellation/session-expiry
  failures. `AppResult` may simplify `?` but must retain structured source
  errors and allow user-defined error types.
- Cloned handles are invalidated on application-session shutdown/recovery.
  Recovery is opt-in and can replay the whole application entrypoint;
  commands must never be implicitly retried after ambiguous transport errors.
- One component providing multiple contracts must share one state instance
  and one lifecycle unless explicitly configured otherwise.
- Production remote execution requires authentication and authorization;
  current loopback RPC handshake metadata is not identity verification.
- Native UI hosts remain separate processes. ECS and multiplayer are optional
  specialized execution integrations, not application-wide requirements.

## Low-level implementation rule

`#[riesel::app]` must generate use of a testable typed-handler/requirement
registration API; `#[riesel::component]` must build on the existing
`ComponentDefinition`. The direct builder and explicit `Context` remain
available to libraries, dynamic compositions and tests. Keep one runtime,
one schema, one provider resolver and one error model.

## Acceptance and verification

1. An ID-free private contract derives deterministic distinct IDs across
   Rust modules/names; explicit published IDs remain unchanged.
2. Minimal Counter application registers dependencies from its signature
   exactly once and works with local and managed implementations.
3. Two aliases for one capability are explicit and fail on ambiguity.
4. Todo `Records<T>` derives and validates model + Storage bindings without
   duplicate declarations or a silent memory fallback.
5. Generated component adapters expose Counter and Observer with shared state.
6. Async RPC is genuinely nonblocking and preserves structured failure
   variants, expiry and cooperative cancellation.
7. Macro and builder create equivalent definitions and startup plans.
8. Invalid macros, missing providers, collisions, dependency cycles and
   invalid bindings produce actionable diagnostics before side effects.
9. Existing lifecycle/recovery executable smoke tests stay green; Windows
   is run explicitly before releases.

## Delivery approach

Implement in small validated changes. First introduce ID-free private
capability declarations while preserving explicit published IDs. Next build
typed extraction and generated component adapters without macros. Add real
async execution and provider-catalog composition before exposing the concise
high-level async app macro. Track actual status only after CI passes.

## Implemented foundation (verified 2026-11-18)

- `#[capability]` can omit `id` and `version` for private contracts.
  Identity derives deterministically from the declaration module and Rust
  name; explicit existing IDs and versions are retained.
- `CapabilityRef::default_requirement_alias` provides an escaped, stable
  contract-derived alias instead of using a function parameter name.
- SDK `ApplicationParam` describes a dependency's declaration and owned
  extraction. `Capability<C>` and `Records<T>` implement the initial
  binding; the capability wrapper preserves generated session guards.
- Typed synchronous function-signature handlers are implemented through
  `Application::handler` and `handler_with_aliases`; Counter and Todo now use
  these handlers locally and in managed smoke tests. Explicit duplicate aliases
  and model bindings are checked before startup.
- `#[riesel::component(provides = [...])]` now generates local/managed
  binding registration from the explicit contract list, and the SDK offers
  `.component::<MyComponent>()` for statically linked components. Optional
  stable IDs and factory paths are supported; no global discovery is added.
- At this 2026-11-18 checkpoint, async generated handles and the
  declarative `#[riesel::app]` were future work. Both are now implemented
  and Linux-verified (see the roadmap's Stage 5.4 and 5.6 evidence).
- Linux CI validated the changes, including workspace tests and runtime smoke
  tests: https://github.com/aschenrot/riesel/actions/runs/44411860685

## Stage 5.2 verified (2026-11-18)

`Application::new("Counter").handler_with_aliases(run_demo, ["counter"])?`
registers a typed `Capability<CounterContract>` parameter without separate
`.rpc` declarations or `Context::capability` lookups. `Todo` uses
`Application::handler(run_demo)?` with `Records<TaskData>`; two separate
model types may intentionally share one Storage requirement. Explicit aliases
are positional at this low-level stage; the future app macro will bind names
to parameters at compile time. The synchronous handler implementation is not
a substitute for genuine async transport or the proposed async entrypoint.
Linux validation and smoke: https://github.com/aschenrot/riesel/actions/runs/44414421066

## Stage 5.3 verified (2026-11-18)

`#[riesel::component(provides = [CounterContract, CounterObserverContract])]`
now derives `DeclarativeComponent`, backed by the same `ComponentDefinition`
used by local and hosted execution. Published components can provide an
explicit `id`; private ones derive a deterministic Rust declaration-path ID.
A `factory = function_path` supports non-`Default` state constructors.
Capability macros generate the local and RPC dispatcher adapters so the
component author doesn't manually cast `Arc` or instantiate handlers.
The existing Counter integration checks both capabilities share state locally
and in a managed process; compile-fail tests reject missing trait implementations.
Linux validation and smoke: https://github.com/aschenrot/riesel/actions/runs/44416458416

**At the Stage 5.3 checkpoint:** Async generated handles, declarative
`#[riesel::app]`, and a module scheduler were not yet implemented.
The first two are now Linux-verified. Application modules and lifecycle
handlers remain Stage 5.7, and generic implicit provider discovery is not
planned.

## Stage 5.4 — async transport foundation (verified 2026-11-19)

The protocol now exposes `AsyncRpcTransport`, a generated
`<Trait>AsyncClient`, and `AsyncRpcClientBinding` for typed construction.
`AsyncTcpRpcTransport` is gated behind the optional protocol
`async-tcp` Cargo feature and uses Tokio's genuinely nonblocking sockets.
It retains the existing wire contract, explicit loopback-only policy,
bounded frames, domain-versus-RPC errors, timeout handling and session-token
checks. Outbound calls are limited to 32 per transport, with async permit
waiting. Pending calls can be cancelled by dropping their futures; they are
not retried. Contract and provider metadata inspection is available, but
provider metadata is **not** service authentication.

The current `#[capability]` macro still expects synchronous Rust trait
methods and the existing local/managed application runtime remains synchronous.
The new async clients do not mean that application injection, async
provider dispatch, runtime-owned async tasks or `#[riesel::app] async fn`
are complete. The detailed design gates and these restrictions are in
[async execution contract](async-execution-contract.md).

Validated: https://github.com/aschenrot/riesel/actions/runs/44420615980

## Native async capability and host progress (verified 2026-11-19)

The `#[capability]` macro accepts entirely asynchronous Rust traits.
It generates an object-safe `<Trait>AsyncService`, a session-scoped
`<Trait>LocalClient` and `<Trait>AsyncRpcDispatcher`. Ordinary Rust
`impl Trait for Component { async fn ... }` requires no manual future boxing.
The generated method futures must be `Send`; a single capability contract
cannot mix sync and async methods. The optional Tokio TCP host can execute
the native async dispatcher with bounded concurrency and nonblocking I/O.

This does **not** yet give developers the final `#[riesel::app] async fn`
surface. The existing provider registry and component macro still bind
synchronous services. The next slice needs async component registration,
runtime-owned async tasks, manager-owned async service lifecycle and prompt
session-driven cancellation.

Verified workspace validation, async Counter integration and managed
synchronous smoke: https://github.com/aschenrot/riesel/actions/runs/44423972358

## Typed async handler progress (verified 2026-11-19)

An application can now write a typed asynchronous Rust function without
creating a Tokio runtime manually:

```rust
Application::new("Counter")
    .async_handler_with_aliases(
        |counter: AsyncCapability<CounterContract>| async move {
            counter.add(3).await?;
            Ok::<_, riesel_protocol::CallError<CounterError>>(())
        },
        ["counter"],
    )?
    .deployment(explicit_config)
    .run()?;
```

The `async-tcp` SDK feature is optional. This is an intermediate API:
the application session owns the Tokio executor and cancellation, but
startup contract inspection is still synchronous. Local async components
must be generated and integrated with the same resolver before the final
`#[riesel::app]` entrypoint is ready. Colliding sync/async connectors fail
during registration; an incompatible local-only async selection fails in
preflight, not after a provider starts. No global provider discovery,
automatic retries or extra runtime is introduced.

Verified Linux checks and managed smoke:
https://github.com/aschenrot/riesel/actions/runs/44429849161
