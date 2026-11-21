# #59 — Real Riesel Todo application under an OS-owned user service

**Status:** independent, stacked **research-only** verification. This PR builds on the verified real systemd/registration experiments in [#94](https://github.com/aschenrot/riesel/pull/94) but does not change an accepted Manager SDK interface, build installer, CLI, registration policy, public authentication, readiness protocol, or app-source behavior.

## Why this experiment

The [#94](https://github.com/aschenrot/riesel/pull/94) baseline proved that a real `systemd --user` transient service can survive the initiating controller, reconcile an OS attempt, isolate stale stops using per-attempt unit identities, and reap descendants. It used **synthetic Python applications**. That cannot establish what a *real Riesel app* considers callable or graceful, including linked SQLite and application-owned async work.

This experiment uses the existing `todo_http` executable and its accepted local-only Riesel/SQLite/HTTP integration **without modifying its source**.

## Verifiable questions

1. Can a separate initiating controller start the actual Todo Rust executable as a transient user service, exit abruptly, and leave the Riesel app running under one OS-owned `MainPID`, `ControlGroup` and `InvocationID`?
2. Is `ActiveState=active` checked independently of **actual interface callability**? `TODO_HTTP_READY` is parsed from a test-only service log, then an HTTP GET to the actual `/api/v1/tasks` endpoint must succeed.
3. Does a **specifically configured app-compatible** systemd `KillSignal=SIGINT` cause the existing Tokio `ctrl_c` observer to perform a **normal** process exit on service stop, rather than rely on the OS to forcibly terminate Rust tasks?
4. Can the service mutate a real linked SQLite database, stop, and restart as a **new unique OS attempt unit** without losing the record? Can a stale stop addressing the first immutable unit leave the replacement alive?
5. What is **not** demonstrated by a passing result: authentication, runtime/Manager registration provenance, same-UID operator authorization, a generic Riesel application stop API, remote interface security or production data portability.

## Reproduction

Run on Ubuntu with an operational `systemd --user` manager and Rust 1.100.0:

```sh
cargo validate --check
cargo smoke
cargo build --locked -p todo --features http --bin todo_http
python3 -m py_compile research/process-ownership/manager_riesel_todo_probe.py
python3 research/process-ownership/manager_riesel_todo_probe.py
```

The isolated [Linux CI workflow](../../.github/workflows/manager-real-riesel-app-research.yml) supplies a real user service manager and executes these checks on the same draft revision. **Do not claim passing evidence until exact-head CI completes**.

The fixture owns only two ephemeral `riesel-research-59-*.service` units and a disposable local SQLite database. It starts with `Type=exec`, no automatic restart, cgroup stop policy, and an **explicit app-specific SIGINT stop signal**. Journal/stderr are captured in a temporary file only for reading this test application's own `TODO_HTTP_READY`. It performs a same-origin local POST to create one record, verifies that the new independently started service can read it, and cleans up the two exact test units.

## Candidate Manager architecture implications

A service owner can truthfully report **process attempt state** from systemd while the application adapter reports **handler/interface callability** through separately verified application behavior. The first does not imply the second. A real Manager status API must state the evidence source, session/attempt identity and age; self-reported `TODO_HTTP_READY` is not an authenticated #75 publisher attestation.

A portable stop contract cannot infer graceful shutdown from `systemctl stop`. Todo's supported current Linux HTTP route listens for Ctrl-C (SIGINT), whereas other Riesel applications may use stdin, hooks or no cooperative stop at all. The app-specific `KillSignal=SIGINT` is experimental **deployment configuration**, not a proposed universal default. Compare OS service signal adapters against an application-owned, versioned stop/notification interface after #75 owner/auth semantics have been accepted. A stop response and a completed mutation remain independent uncertainty domains.

Distinct immutable OS attempt names address the wrong-attempt stop hazard shown by #94. They do **not** provide registration-level duplicate launch prevention; that requires the previously verified *cooperating-controller* durable admission policy, and still does not defend against malicious same-UID actors or out-of-band systemd user-bus operations.

**Decision recommendation (not acceptance):** Use a real Riesel application as an acceptance vehicle for installed Linux Manager process ownership, but keep a deliberately **app-specific cooperative stop** policy until a shared operational contract is defined. Do not add a universal SIGINT handler or Manager API solely from this fixture. The scoped local, trusted-user mode remains distinct from a future protected/brokered principal mode.

No source PR or main merge, implementation issue, accepted specification change, platform release promise or auth claim is authorized.
