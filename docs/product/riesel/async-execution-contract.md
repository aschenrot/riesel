# Riesel — Async Execution Contract (Stage 5.4)

Status: partial implementation verified on Linux, 2026-11-19.
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

## Remaining engineering decisions and required behavior

### 1. Native async capability contracts and local execution

The accepted surface includes `#[capability] trait Counter { async fn ... }`.
The current macro **rejects** async trait methods. Native async trait methods
are not directly dyn-compatible in Rust. We must generate a typed,
object-safe asynchronous service adapter (or an equivalent typed handle)
for component state instead of assuming `Arc<dyn Counter>` will work.
Ordinary Rust `impl Counter for MemoryCounter` syntax should remain
possible. Avoid requiring every author to manually box futures or use an ECS.

Compile-time tests must cover async receivers, owned and borrowed arguments,
typed domain errors, generic restrictions, unsupported forms and generated
contract equivalence. Changes to published contracts must retain intentional
versioning; do not silently rewrite Counter or Storage wire IDs.

### 2. Async server dispatch and local/managed parity

The current RPC host serves synchronous handlers on a bounded worker pool.
Add an async dispatcher/host with bounded admission and cooperative shutdown,
and a generated in-process async adapter that does not synchronously block
the executor. Both routes must share the same provider-owned state and
versioned contract metadata.

Tests must call the same capability locally and in a managed process without
application-level transport wiring. Blocking local implementations need an
explicit boundary; never poll blocking I/O in a Tokio task and label it
nonblocking.

### 3. Session ownership, cancellation and execution policy

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

### 4. Developer-facing injection and error model

The accepted `CounterHandle` should be a generated typed façade over the
same capability binding and runtime ownership in local and remote modes;
it should not introduce a second provider resolver. `AppResult` should
preserve failure sources. The subsequent `#[riesel::app]` macro must
generate the same validated definition as the lower-level builder.

Multiple providers still require deterministic aliases and selection.
Auto-registered private IDs are not externally stable contracts. No silent
MemoryStore fallback and no global constructors.

### 5. Security and production boundaries

Current TCP is loopback-only, unauthenticated and unencrypted. Its provider
announcement is not a trust proof. Persistent Storage, authenticated remote
transport, authorization, observability, configurable budgets and selective
recovery remain production Stage 6 requirements. Async client progress does
not change that security posture.

## Stage 5.4 completion gates

1. Native async capability definitions and generated local/managed adapters
   compile without hand-written dispatch or a required async-trait macro.
2. Genuine nonblocking calls and bounded provider execution work in both
   local and managed scenarios; no synchronous TCP hidden inside futures.
3. Runtime-owned task lifecycle, session invalidation and prompt cooperative
   cancellation pass deterministic tests, including live managed-child crash.
4. Domain/transport/cancellation/session errors remain distinguishable;
   no automatic command retry, even after timeout.
5. Generated async bindings derive the same validated contract and provider
   selection metadata as the synchronous reference where compatible.
6. `cargo validate --check` and `cargo smoke` pass on Linux; Windows stays
   manual and required before release.

Only after this gate should the high-level `#[riesel::app] async fn` milestone
claim to be ready. Stage 5.5 provider catalog work can proceed independently
without weakening these boundaries.
