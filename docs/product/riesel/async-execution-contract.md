# Riesel — Async Execution Contract (Stage 5.4)

Status: native async contracts, managed-host lifecycle and generated local/managed component parity verified on Linux, 2026-11-19.
This defines the next engineering gates for the accepted
[high-level application authoring API](api-authoring-v3.md).
The user-facing mental model stays **app uses capabilities, components
implement capabilities, Riesel resolves and runs them**. There is no
mandatory ECS, transport object or executor type in the default authoring API.

## Existing and verified

- `AsyncRpcTransport` is a transport-neutral, object-safe interface returning
  boxed `Send` futures. It does not delegate to blocking `RpcTransport`.
- The capability generator emits `<Trait>AsyncClient<T>`, using
  `AsyncRpcClientBinding` for typed construction; existing synchronous
  generated clients and published capability contracts remain unchanged.
- `AsyncTcpRpcTransport` uses Tokio nonblocking sockets behind the optional
  `riesel_protocol/async-tcp` feature. The normal minimal SDK does not
  require a Tokio runtime.
- Framing remains JSON-lines wire protocol v1, bounded by 4 MiB. Only
  loopback plaintext TCP is allowed. Connect and request timeouts are
  bounded; the current client also bounds in-flight operations to 32 per
  transport via an async semaphore.
- No RPC is implicitly retried. Pending calls can be cancelled by dropping
  the future, which closes its socket and releases concurrency permits.
  A timed-out command may have executed on the provider.
- `SessionToken` is checked before and after calls, including after waiting
  for a semaphore. Escaped async handles reject calls after session expiry.
  The optional Tokio TCP client now races pending I/O and semaphore waits
  against invalidation; revoking a session immediately drops those futures.
  The cancellation does **not** prove that a previously transmitted command
  was not executed. Synchronous transports remain cooperatively bounded.
- `CallError<E>` still separates declared domain errors from RPC failures.
  Async contract shape checks and advertised provider metadata are
  inspection, not authentication or authorization.
- Counter tests cover generated async calls, real TCP, domain errors,
  expiration, cancellation by future drop, metadata and concurrency limits.
  Existing managed-process smoke and minimal-feature checks remain green.
  Evidence: https://github.com/aschenrot/riesel/actions/runs/44420615980

## Native async contract and host now verified

- `#[capability]` accepts native async Rust methods, while rejecting mixed
  synchronous/asynchronous declarations in one contract. Generated trait
  signatures use `impl Future<Output = Result<T, E>> + Send`, which normal
  Rust `async fn` implementations satisfy when their futures are Send.
- The generator emits an object-safe `<Trait>AsyncService`, an owned,
  revocable `<Trait>LocalClient` and `<Trait>AsyncRpcDispatcher`.
  Developers do not manually box futures. Internal adapter methods have
  distinct names so `self.value()` remains unambiguous inside impls.
- `AsyncRpcHandler` supplies typed contract metadata, capability identity
  checking and true async dispatch. The optional Tokio TCP host uses bounded
  frame reads/writes, 5-second deadlines, loopback enforcement, at most 32
  active connections, and owned JoinSet tasks aborted at shutdown.
- A generated native async Counter contract runs locally and through the
  standalone Tokio TCP host; tests cover a command, query, borrowed string,
  domain error, session expiry and unknown operation.
- The synchronous Counter and Storage capability contracts, RPC host and
  application manager have not been changed. Evidence:
  https://github.com/aschenrot/riesel/actions/runs/44423972358

## Native async routing and managed-host protocol now verified

- `AsyncRpcRouter` routes several native async capability contracts through
  one process and provider identity. Each handler must advertise a matching
  contract and complete the provider's declared interface set; missing,
  duplicate, unadvertised and unknown capability requests are rejected.
- The Tokio host validates routes before accepting connections and retains
  its bounded, nonblocking request-task lifetime and loopback restriction.
- `run_async_tcp_rpc_host` shares the synchronous managed-host protocol:
  a bounded `RIESEL_READY` announcement after binding a real loopback
  listener, and cooperative `RIESEL_SHUTDOWN` via private stdin.
- An executable subprocess regression checks readiness, real RPC calls to
  two interfaces backed by one shared state instance, and bounded shutdown.
  Linux workspace validation and smoke passed at
  https://github.com/aschenrot/riesel/actions/runs/44433596866
- This verifies the **protocol bridge**, not generated local/managed
  component integration: `ComponentDefinition` still binds synchronous
  adapters, and application-managed native async provider lifecycles require
  additional work.

## Generated native async component parity now verified

- A single `#[component(provides = [...])]` declaration supports generated
  synchronous and native asynchronous capability bindings. The generated
  `ComponentAdapter` metadata includes the local binding mode; runtime
  preflight rejects incompatible selection before constructing any provider.
- Native async contracts now generate a typed `<Trait>Handle` façade.
  `AsyncRpcClientBinding::Service` uses this exact handle in both modes.
  The local variant delegates directly to the generated revocable async
  service view without in-memory JSON encoding; the remote variant uses the
  existing nonblocking `<Trait>AsyncClient` and Tokio transport.
- `ComponentDefinition` owns and shares one Arc-backed component instance
  across its declared interfaces. For native async-only components,
  `serve()` dispatches through the generated `AsyncRpcRouter` and
  `run_async_tcp_rpc_host`, using the existing managed readiness, shutdown,
  provider resolver and process monitor. Mixed sync/async single-listener
  hosting is explicitly rejected before invoking the state factory.
- Real native async Counter and Observer implementations run unchanged from
  one typed async application handler in linked and manager-owned modes.
  Integration tests cover shared state, commands, queries, domain failures,
  and rejection of cloned handles after shutdown. Existing synchronous
  Counter/Storage behavior remains validated by workspace regression tests.
  Linux evidence:
  https://github.com/aschenrot/riesel/actions/runs/44439704442
- Native async provider crash/recovery and structured background tasks need
  dedicated application-level regression tests. Async startup inspection
  still uses bounded synchronous I/O; this is an explicit limitation.

## Typed async application execution now verified

- The existing supervisor now invalidates the same `SessionToken` carried
  by typed service handles on managed-child failure; shutdown also revokes
  the token before task joins. Tests cover child failure, racing async
  cancellation subscribers, pending TCP read and semaphore admission.
- The Rust SDK's optional `async-tcp` feature exposes
  `AsyncCapability<C>`, `Application::async_rpc::<C>`, and
  `Application::async_handler` /
  `Application::async_handler_with_aliases`. These use the *same*
  application schema and provider resolver as synchronous handlers, reject
  conflicting connector modes and preserve contract-derived aliases.
- A runtime-session-owned current-thread Tokio executor runs each async
  handler. On session invalidation the pending entrypoint future is dropped,
  even when waiting on unrelated async work. Whole-session recovery uses
  new handles, not retries of individual business commands.
- Remote contract/provider inspection during startup still calls bounded
  **synchronous** TCP before constructing the nonblocking Tokio client.
  This is not a fully async startup pipeline. Local sync-only components are
  rejected for an async-only binding during deployment preflight instead of
  failing after startup with a Rust downcast error.
- Real Counter tests exercise the typed async application handler through
  the existing TCP endpoint, session expiry after teardown, ambiguous
  transport-mode registration and invalid local selection. Additional tests
  cover zero-argument async handlers, alias-count errors and cancellation
  of an otherwise indefinitely pending handler future. Evidence:
  https://github.com/aschenrot/riesel/actions/runs/44429849161

## Remaining engineering decisions and required behavior

### 1. Component registration and supervised runtime parity

**Implemented and tested on Linux:** `#[component]` now derives local and
managed native async bindings from the same declaration and state factory.
Native async-only components use the Tokio host within the existing managed
service lifecycle, including readiness, private-stdin shutdown and provider
selection. The same typed application handler runs against both modes.
Mixed-mode hosting is rejected rather than blocking a Tokio worker.

**Remaining:** explicit native async provider crash/recovery tests through the
application monitor, and an explicit blocking-work boundary for native async
implementation code. No independent service runtime or transparent replay of
commands may be introduced.

### 2. Session ownership, background tasks and execution policy

The SDK now owns a current-thread Tokio executor per async application
session and cancels its entrypoint future on session invalidation.
**Still required:** session-owned async background task groups, bounded
concurrency, joining and structured task error propagation, integrated with
the existing Rust-thread task group and service manager. Do not introduce
detached `tokio::spawn` work or hide blocking operations in the executor.

A failed managed child now promptly cancels dependent pending Tokio TCP
calls and the pending async entrypoint. Dropping a future cancels waiting
and socket ownership, **not** proof that the provider did not execute the
command. Preserve opt-in whole-session recovery and no command replay.
Keep structured domain/transport/session errors and stale-clone rejection.

### 3. Developer-facing injection and error model

The accepted `CounterHandle` should be a generated typed façade over the
same capability binding and runtime ownership in local and remote modes;
it should not introduce a second provider resolver. `AppResult` should
preserve failure sources. The subsequent `#[riesel::app]` macro must
generate the same validated definition as the lower-level builder.

Multiple providers still require deterministic aliases and selection.
Auto-registered private IDs are not externally stable contracts. No silent
MemoryStore fallback and no global constructors.

### 4. Security and production boundaries

Current TCP is loopback-only, unauthenticated and unencrypted. Its provider
announcement is not a trust proof. Persistent Storage, authenticated remote
transport, authorization, observability, configurable budgets and selective
recovery remain production Stage 6 requirements. Async client progress does
not change that security posture.

## Stage 5.4 completion gates

1. Native async capability definitions and generated component registrations
   are **implemented and tested** locally and in manager-owned child processes,
   without handwritten dispatchers or an async-trait macro.
2. Genuine nonblocking remote calls, bounded Tokio request admission and
   direct native async local invocation are **implemented and tested**.
   One generated typed handle works in local and managed modes.
3. Session invalidation on managed-child failure and prompt async RPC /
   entrypoint cancellation are **implemented and tested** independently.
   Native async provider crash/recovery regression coverage and bounded
   runtime-owned asynchronous background task groups remain. The manager-owned
   native async host's readiness, shared state and cooperative shutdown
   are **verified**.
4. Domain/transport/cancellation/session errors remain distinguishable;
   no automatic command retry, even after timeout.
5. Generated async bindings derive the same validated contract and provider
   selection metadata as the synchronous reference where compatible.
6. `cargo validate --check` and `cargo smoke` pass on Linux; Windows stays
   manual and required before release.

Only after this gate should the high-level `#[riesel::app] async fn` milestone
claim to be ready. Stage 5.5 provider catalog work can proceed independently
without weakening these boundaries.
