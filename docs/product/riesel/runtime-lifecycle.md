# Riesel — Runtime Lifecycle and Recovery

**Status:** Implemented within the documented reference/cooperative scope. This document states lifecycle invariants that are independent of the user-facing application macro. It is **not** a production deployment guarantee.

## Definition, selection and preflight

An application definition lists capability requirements, models, and explicit lifecycle dependencies. Dependency edges control startup order and reverse teardown order; they do not imply provider discovery or selective live replacement.

Runtime preflight validates local schema and requirement declarations, provider connector compatibility, instance-sharing constraints and dependency cycles **without** constructing providers, opening sockets or launching managed processes. It cannot establish that an external service exists, cryptographically authenticate a provider or guarantee readiness.

Provider-catalog choices and direct deployment configuration compile into the same runtime planning and start path. Named managed requirements can share one process and provider state only when their launch settings and provider declarations are compatible. Invalid or conflicting configurations fail before side effects.

## Starting and supervising providers

- Start requirements in validated dependency order. Local factories build shared component state as declared; managed binaries start with an OS-assigned loopback endpoint.
- Managed providers advertise a bounded readiness frame with their declared contract/provider metadata. The runtime validates it, but this channel is not cryptographic proof of identity.
- Failed startup rolls back already started components/services, including healthy siblings, without leaking a partially started application session.
- The runtime monitors unexpected exits of required managed children. A required child's exit invalidates that application session and signals its cancellation token.
- External TCP providers are not supervised as owned children; current lifecycle guarantees must not be described as external-service health monitoring.

## Application-owned work and teardown

An application can register cooperative tasks. Synchronous owned worker threads have a bounded active-task registry; completed tasks are joined and their names/capacity can be reused. Async sessions additionally own `AsyncTasks` and explicit bounded blocking scopes.

Task errors and panics are propagated with useful sources and trigger session cancellation. Shutdown cancels the session, prevents new uses of stale typed capability handles, joins owned tasks, and then tears down providers in reverse dependency order. Cleanup failures must not silently replace the original business failure.

A Rust thread that never cooperates with cancellation may block local teardown indefinitely; terminating a Rust thread in place is not a safe general recovery mechanism. A configured managed process may be killed after its graceful shutdown deadline.

## Session identity and recovery

Generated local and remote handles are bound to an application session and check revocable session identity. Cloned handles retained after shutdown or restart reject new calls. Pending or in-flight synchronous calls are not forcibly undone.

The reference recovery policy defaults to fail-fast. Explicit `restart_application` configuration sets bounded whole-session restarts with backoff; it rebuilds dependency-ordered providers and application contexts from scratch.

Recovery is eligible for unexpected managed-process loss under the supported cooperative conditions. It does not silently retry ordinary application business errors, startup failures, unrelated task errors, or individual RPC commands.

**Important:** Rerunning an application entrypoint can repeat external side effects. Memory-only provider state does not persist. A timeout, lost response or cancelled RPC request does not prove the command was never executed. There is no exactly-once or transparent per-provider rebinding guarantee.

## Future lifecycle work

Stage 5.7 lifecycle handlers must run within the existing session and dependency DAG, preserve structured cleanup failures, stop before required providers are destroyed, and honor cooperative cancellation. Optional modules group registration only; they do not invent a new service supervisor.

Future production behavior requires explicit contracts for safe stateful recovery, external-provider health, configurable budgets, observability and separately authenticated transports.

See [implementation roadmap](implementation-roadmap.md), [async execution contract](async-execution-contract.md), and [provider catalog](provider-catalog.md).
