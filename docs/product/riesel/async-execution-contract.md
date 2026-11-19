# Riesel — Native Async Execution Contract

**Status:** Stage 5.4 reference implementation complete and Linux-verified, 2026-11-19. This document defines the execution and safety invariants maintained by Stage 5.6 authoring and later lifecycle work. It does not claim authenticated remote execution, durable provider state, or transparent recovery.

## Scope and ownership

Native async capability contracts and generated typed handles use the same capability schema and provider resolver as synchronous contracts. The optional `async-tcp` feature adds a Tokio-based nonblocking transport and session-owned async application execution; the minimal synchronous SDK does not require Tokio.

A native async linked provider is invoked directly without serializing local calls through JSON. A managed native async provider uses generated routing and the shared managed readiness/shutdown protocol, with one component instance shared across its declared capabilities. A mixed synchronous/asynchronous contract is rejected rather than silently blocking an async worker.

## Transport contract

- `AsyncRpcTransport` is a transport-neutral async interface; it does **not** wrap blocking TCP in an async signature.
- The reference async TCP transport is loopback-only plaintext, uses bounded JSON-line framing (up to 4 MiB), and has explicit connection/request deadlines.
- Outbound requests are bounded (32 concurrent calls per transport in the reference implementation). Admission waits are cancellable and use the Tokio executor without blocking it.
- The optional Tokio host bounds active connections (32 in the reference host), frame processing and request ownership. Hosted request tasks have explicit shutdown handling.
- Generated `CallError<E>` preserves declared domain errors separately from protocol, connection, cancellation and session-expiration errors.
- Requests are not implicitly retried. Closing a socket, cancelling a future or reaching a deadline **does not** establish whether a previously transmitted operation executed.
- Contract and advertised provider metadata checks detect mismatches; neither proves remote service identity or authorization.

## Session ownership and cancellation

- Each async application session owns its Tokio executor, scoped entrypoint and cancellation token. The startup coordinator shares one dependency/resolution path with synchronous applications, while async transport inspection is nonblocking.
- Session invalidation wakes pending Tokio RPC calls, including semaphore waits, and drops pending application entrypoint futures; old typed handles reject new calls after session closure.
- `AsyncTasks` is a bounded (64 active tasks), session-owned spawner. It tracks named task failures/panics, rejects stale or late spawns, cancels work and joins it before provider teardown. When lifecycle stop hooks are registered, owned async work is drained before those hooks; the capability session is revoked after eligible hooks finish.
- Application-owned `AsyncTasks::spawn_blocking` uses a distinct bounded scope (eight concurrent workers in the reference implementation). It requires cooperative cancellation and joins work during shutdown.
- Provider-owned blocking work is an **explicit** `ProviderBlocking` boundary (up to eight named workers per instance in the reference implementation). The provider retains and joins work before component state is destroyed, even if its RPC caller drops a future.
- Arbitrary synchronous native code cannot be forcibly preempted safely inside a Rust process. Managed child processes retain a separate, bounded process shutdown/kill fallback where configured.

## Managed hosting and recovery

The managed async host uses the same `RIESEL_READY` announcement and private `RIESEL_SHUTDOWN` lifecycle channel as the synchronous host. Ready messages and contract selection are validated before use. Components can expose several async contracts through one router and state instance.

A managed-child failure invalidates the current application session, cancels pending calls/tasks, and runs dependency-aware cleanup. Opt-in bounded recovery rebuilds the **entire application session**, including new provider instances, handles and task scopes. It does not selectively rebind an old client, preserve in-memory state, or automatically retry individual business commands. Business, startup and task errors are not silently converted into restart eligibility.

The application entrypoint and external side effects must be safe under explicitly configured session replay. No exactly-once guarantee is provided.

## Diagnostics and stage boundaries

- Invalid mode combinations and unsupported local/provider registrations should fail during preflight, before constructing provider state or launching managed children.
- Preserve original failures, task names and cleanup diagnostics; do not reduce distinct error sources to a single unstructured string.
- The accepted `#[riesel::app]` API uses these same execution invariants; see [high-level authoring](high-level-app.md).
- Optional application modules register ordered, session-scoped async start and stop hooks through the existing builder, `AsyncTasks` and supervision path; see [application modules](application-modules.md).
- Production transport authentication, authorization, persistence, observability, configurable budgets, and selective stateful recovery are **outside** Stage 5.4 and require separate acceptance.

## Verification

Reference regressions cover native async local and managed Counter/Observer behavior, bounded nonblocking TCP, domain errors, session expiry, cancellation, multi-contract state sharing, controlled shutdown, scoped tasks, provider-owned blocking work, managed-child crash and whole-session restart.

Stage 5.4 Linux verification: [native async provider work and shutdown](https://github.com/aschenrot/riesel/actions/runs/44493550783). Latest stage acceptance state: [implementation roadmap](implementation-roadmap.md). Windows remains a separate manual pre-release validation gate.
