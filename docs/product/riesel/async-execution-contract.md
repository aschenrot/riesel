# Riesel — Async Execution Contract (Stage 5.4)

Status: native async contract and standalone host slices verified on Linux, 2026-11-19.
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
  A session invalidated during I/O is detected once the current operation
  completes or times out; **in-flight calls are not yet cancelled promptly
  by session invalidation itself**.
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

## Remaining engineering decisions and required behavior

### 1. Component registration and supervised runtime parity

The current `#[component]` and `ComponentDefinition` APIs still expect
synchronous `RpcClientBinding` / `ComponentAdapter` interfaces.
Generate native async component bindings from the **same** component
declaration and provider-owned state, with typed local and managed adapters.
The standalone Tokio async server is not yet connected to the managed
process lifecycle; it does not advertise supervised readiness or implement
Riesel's existing provider recovery policy. Do not create a parallel runtime
or silently call synchronous providers inside async futures.

Tests must call the same native async capability locally and through a
manager-owned process without application-level transport wiring. Blocking
implementation code needs an explicit blocking boundary.

### 2. Session ownership, cancellation and execution policy

Introduce runtime-owned async tasks with bounded concurrency, join/cancel
semantics and observable failures, rather than detached `tokio::spawn`
work. Define which runtime owns the Tokio executor and how it is configured
for CLI, service and future game workloads.

A failed managed child must promptly cancel dependent pending async calls.
Dropping a future is cancellation of waiting and socket ownership, **not**
proof that the provider did not execute the command. Keep opt-in whole-session
recovery and no implicit business-command replay.

Every async client must be tied to the active session before application
code receives it. Stale clones and results from a stopped session must not
escape. Cancellation, timers and error sources must remain structured.

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

1. Native async capability definitions and standalone local/Tokio adapters
   compile and run without hand-written dispatch or an async-trait macro
   (**implemented**); generated component registrations and supervised
   managed bindings remain.
2. Genuine nonblocking calls and bounded provider execution work for
   standalone local and Tokio host scenarios (**implemented**); supervised
   local/managed parity remains.
3. Runtime-owned task lifecycle, session invalidation and prompt cooperative
   cancellation pass deterministic tests, including live managed-child crash
   (**not yet implemented**).
4. Domain/transport/cancellation/session errors remain distinguishable;
   no automatic command retry, even after timeout.
5. Generated async bindings derive the same validated contract and provider
   selection metadata as the synchronous reference where compatible.
6. `cargo validate --check` and `cargo smoke` pass on Linux; Windows stays
   manual and required before release.

Only after this gate should the high-level `#[riesel::app] async fn` milestone
claim to be ready. Stage 5.5 provider catalog work can proceed independently
without weakening these boundaries.
