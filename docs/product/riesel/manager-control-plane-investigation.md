# Riesel Manager — product, control plane and UI investigation

**Status: research proposal; NOT an accepted architecture or implementation authorization.**  
**Specification:** [#59](https://github.com/aschenrot/riesel/issues/59) · **Evidence date:** 2026-11-20 · **Repository baseline:** `b2702907` (`main`).  
**Decision required:** maintainer accept / revise / defer / reject under [delivery.md](../../project/delivery.md). This document is investigation evidence, not a new roadmap stage. **Cross-product semantic coordination:** [#75](https://github.com/aschenrot/riesel/issues/75) / [operational boundary research](application-operational-boundary-investigation.md).

## 1. Problem and observed baseline

An operator needs to discover explicitly installed Riesel applications, see what is actually running, inspect their capabilities/deployments, understand failures and intentionally start or stop applications without inventing individual scripts for every app. The Riesel [product definition](riesel.md) already names Manager and Manager UI as planned, but neither has a delivery contract.

Code/document audit:

| Existing artifact | Verified capability | Not provided |
| --- | --- | --- |
| [`riesel_manager::ProcessRegistry`](../../../crates/riesel_manager/src/manager.rs) | Per-owner child spawn, status, orderly stop and fallback kill; registry drop owns cleanup | Persistent manager process, application registry, remotely callable control API, GUI, or cross-process ownership transfer |
| [`riesel_app::ServiceMonitor`](../../../crates/riesel_app/src/supervisor.rs) | Within one running application, monitors required managed providers and revokes their session on failure | Cross-application inventory, controllable manager service, live provider rebinding |
| [Provider catalog](provider-catalog.md) | Exact registered providers, named managed/external placements, explicit selection, side-effect-free inspection | Ambient discovery or global scheduler; external providers are not manager-owned |
| [App authoring/runtime](high-level-app.md), [lifecycle](runtime-lifecycle.md) | Headless running and session-scoped cleanup/recovery | Independently running control plane that can start an app which is currently stopped |
| [Todo R2](../todo/todo.md), [local HTTP contract](../todo/http-local-browser-contract.md) | Application-specific loopback browser UI and app-owned HTTP service | Authenticated general manager API or reusable management UI |

**Architectural constraint:** `ProcessRegistry` is an ownership primitive: moving it behind an API without clarifying *which process owns which child* would create contradictory lifecycles. A stopped application cannot serve its own control API. Likewise, metadata handshake or localhost binding is not authorization.

## 2. Proposed bounded outcome and users

**Primary user:** one trusted local developer/operator, initially on a controlled single host. **Secondary consumer:** a CLI and local browser dashboard using the same control contract. Do not call the result a cluster manager.

| Operator job | Recommended first slice | Deferred until separate acceptance |
| --- | --- | --- |
| See configured applications | List explicit trusted registrations, executable identity, intended configuration and declared capabilities/placements | Network scanning, auto-discovery, arbitrary installed executables |
| See live state | Start/stop pending, running, exited/failure/unknown; distinguish process state from application readiness | Generic remote health and guaranteed provider-level metrics |
| Start and stop | Explicit operator action with fixed trusted executable/configuration and ownership-aware, bounded shutdown | Arbitrary shell command execution; killing externally owned services |
| Inspect | Read validated cached definitions when available; invoking a trusted application's inspection command is an explicit process launch | Pretending arbitrary executable inspection is side-effect-free |
| Diagnose | Last observed status, bounded/log-sanitized failure details, readiness/timeout and next action | Full log aggregation, tracing platform, audit server |
| Reconnect | Browser reload/reconnect to running Manager and read fresh state | Transparent recovery of in-flight mutations or exactly-once operations |

The requirement is an *operator-facing management product*, not a requirement that all future `riesel_manager` internals become public.

## 3. Architectural candidates, compared on identical criteria

| Approach | Process ownership and restart | API/UI and security | Complexity/long-term fit | Assessment |
| --- | --- | --- | --- | --- |
| **A. Per-application management endpoint** | App can report/stop itself; endpoint disappears when stopped, so cannot start it | Each app exposes its own surface and authentication | Small per app but duplicates bootstrap/discovery and cannot meet start-stopped-app outcome | Reject as complete Manager; may supply an optional in-app diagnostics adapter |
| **B. Separate single-host Manager owning application processes** | Manager owns launched app children; each app still owns its internal managed provider children | One local management API; browser UI is a client | Additional service/lifecycle and failure-reconciliation design | Viable candidate; direct-child-only ownership must not be misreported as process-tree control |
| **C. Static read-only dashboard over inspections** | Does not own processes; cannot manage start/stop | Easiest UI and minimal trust surface | Useful interim viewer, but fails management outcome | Viable reduced-scope fallback if mutating API cannot yet be secured |
| **D. Distributed/global orchestrator** | Multiple nodes, leases, scheduling and inter-host failure handling | Requires remote identity, authorization and durable coordination | Much larger security and reliability surface | Defer; not a Stage-6 prerequisite for local Manager |\n| **E. OS-managed application unit as the process owner** | On supported hosts, an OS service/scope manager can own and report the actual app unit across a Manager UI restart | Platform-specific registration, permissions and APIs; no guaranteed portable identity or security | More upfront integration, potentially stronger lifecycle containment/reconciliation and less custom supervisor logic | **Must compare before selecting B** |\n| **F. Hybrid: Riesel management semantics with pluggable process ownership** | Shared identity, observations and command contract; one of direct foreground child or OS service-unit ownership selected per deployment | Need a single unambiguous owner; do not split one app between two controllers | Potential long-term flexibility without forcing OS units on a developer command | Investigate against B/E; not automatic abstraction requirement |

**Conditional candidate B:** when Manager directly launches an application, it owns **that application's OS child handle**; the running application's existing `riesel_app` runtime still owns its managed provider children. When an OS service manager owns the application instead (E/F), the Riesel Manager may **request** OS-unit operations under an explicit authorization policy but must not claim to own a `ProcessRegistry` child. Provider resolution and application-session identity remain in Riesel. An externally operated application is never silently adopted. Compare ongoing child supervision and OS-owned lifecycle using the same operator jobs, failure modes and total ownership cost.

**Critical consequence:** a Manager that must survive an application closing cannot be *only* a library executing inside that application. A Manager service process (or another externally owned launcher) is necessary; this is not automatically a mandate for a new crate or always-on system service.

**Inspection risk:** `App::preflight` and definition generation are side-effect-free after the application's Rust configuration is constructed; invoking an arbitrary registered executable with `--print-definition` still executes that program. Manager must use a previously validated artifact or deliberately launch a trusted executable under an explicit inspection policy. Do not treat its output as a live state guarantee.

## 4. Proposed control-plane contract for review

**Boundary:** single host, explicitly configured trusted executables. Local scope is a **proposed limitation**, not verified authentication. Manager maintains a separation between:
- **Registration**: stable operator-supplied application identity, trusted binary path, validated arguments and configuration references; registrations are not process handles.
- **Attempt**: unique launch generation and Manager-owned OS process; PID is diagnostic only, not a stable identity.
- **Runtime report**: declared definition/plan, readiness and failure report; distinguish live observations from cached ones.
- **Provider/instance**: reported by running Riesel app; Manager does not claim authority to move, select or stop a provider owned by that application.
- **Command**: explicitly authorized start/stop against an expected application identity/generation; returns accepted/in-progress/completed/failure status, not a fictional exactly-once result.

Proposed minimal conceptual operations (names and serialization deliberately *not* API commitments):

| Operation | Precondition / result |
| --- | --- |
| `list`, `inspect(id)`, `status(id)` | **Passive** inspection reads cached/validated metadata and observed state without starting app code; **active** inspection that executes a trusted binary is an explicit authorized operation with its own side effects/exit result. A stale/unknown state must be reported as such |
| `start(id, expected_state)` | Trusted registered executable only; reject duplicate launch and conflicting generation |
| `stop(id, expected_generation)` | Applies only to Manager-owned process; graceful stop then bounded escalation with explicit reporting |
| `command_status(command_id)` | Acknowledge accepted command separately from observed process outcome to handle client disconnects |

**Open authoring decision:** static file registration versus an authenticated registration operation. First release can use a validated operator-owned configuration file; this avoids exposing an arbitrary executable-launch API to the browser and simplifies persistence. Durable desired/observed state after Manager restart must be specified separately: do **not** assume running orphan processes are automatically reattached or safe to kill. If reliable restart adoption is required for the first slice, evaluate OS service ownership/identity and persisted journal before accepting B.

**Open application readiness decision:** a spawned process is not necessarily a successfully started Riesel application. Evaluate an opt-in app-level ready/termination report or known supported readiness probe; do not confuse current *provider* readiness announcement with *whole-app* readiness. Without such a signal, report `process_running / app_readiness_unknown`, never `ready`.

**Process-tree gap:** `ProcessRegistry` owns direct child handles, not an OS-enforced transitive process tree. Manager death can leave launched apps running; application death can leave their managed provider children running. An implementation must either verify OS-level descendant containment/cleanup or explicitly limit its stop guarantee to the top-level child and report potentially unreconciled descendants. Do not claim that stopping the application proves all providers/resources stopped.

### Control and threat model

An HTTP dashboard can reuse the **local same-origin pattern** proven for Todo R2 (single `127.0.0.1` authority; strict Host and Origin/Fetch Metadata; no permissive CORS; bounded requests), but Todo's browser protections explicitly **do not** authenticate arbitrary local clients. Management writes have greater consequences than task editing. Before exposing start/stop through a browser, specify a verifiable **operator authorization/bootstrap** mechanism, secret handling and OS file permissions; fail closed without it. Do not place executable paths/secret tokens in web errors or unauthenticated responses. Browser UI cannot be treated as the process authority.

Compare:
- Loopback HTTP + same-origin assets for browser-facing API: simple UI deployment and CLI compatibility; vulnerable to unauthorized local OS clients unless separately authenticated; subject to browser-origin/DNS-rebinding controls.
- Unix domain socket with restricted path / peer credentials for local CLI and privileged control: supports OS-local authorization under tested filesystem/UID rules, but browser UI requires a small authenticated bridge. Windows named pipes need independently reviewed ACL and remote-client policy.
- Bound loopback TCP only: portable, but loopback is *network location*, not an access-control identity.
- Remote HTTPS management: deferred until separate trust/authentication and secrets requirements are accepted.

**Recommended separation for research acceptance:** trusted local control channel and single-origin UI bridge that delegates to the same command/service layer, with one authorization policy; not two independently implemented backends. No guarantee of security until adversarial and platform tests pass. The application-definition/epoch/readiness concepts should be compared with CLI and native interface needs in [#75](https://github.com/aschenrot/riesel/issues/75), without making the GUI or developer CLI dependent on a persistent Manager.

## 5. Failure-trace analysis

1. **Start:** operator selects a registered app; Manager checks authorization and expected state, records new attempt identity, starts trusted executable, observes process and bounded application readiness. UI reports `starting` until readiness evidence, not just `running` after `spawn()`. Failed startup transitions to failed with an actionable reason and owned-child cleanup.
2. **Stop:** operator targets the current generation. Manager rejects stale generation/externally owned target; asks child to stop; existing application shutdown handles its providers. If child does not cooperate, Manager reports bounded escalation and reaps *only its own child*. Do not assume external side effects rolled back.
3. **Crash:** a Manager-owned app exits unexpectedly. Application-internal provider monitor cannot repair its own exited parent; Manager observes exit, marks attempt failed, makes *new operator action* explicit. Automatic restart requires a separately accepted policy and reconciliation with Riesel whole-session recovery.
4. **Client request timeout:** the stop/start may have executed even if the browser lost its response. Client queries current attempt/command status and never silently issues the same command again.
5. **Manager crash/restart:** process ownership, service manager vs direct child creation, orphan handling and persisted desired state are unresolved. First MVP must either define tested recovery/adoption or fail closed and require explicit manual reconciliation. Do **not** promise restart-safe process adoption by PID.

## 6. Acceptance/verification matrix for future implementation

| Test gate | Expected observable evidence |
| --- | --- |
| Registration/inspection | Explicit trusted program/args, invalid configuration and duplicate ID fail; passive metadata inspection never spawns app, while explicit trusted executable inspection may launch and must report side effects/authority |
| Ownership | Distinguish Manager-owned application vs app-owned provider vs external process; no accidental termination |
| Lifecycle | Cold start, duplicate start, readiness timeout, normal stop, refused stop, crash and escalation; PID reuse/stale generation guarded |
| API semantics | Same error/status meanings through CLI and UI; concurrent commands and lost response do not silently replay |
| Security | Cross-origin requests, DNS rebinding-style Host, untrusted local client, token/bootstrap leak, path/command injection and overbudget requests rejected within declared trust scope |
| UI | Accessible keyboard operation, explicit loading/empty/stale/error states; refresh reconnects without restarting app |
| Process failure | Manager crash/restart policy tested; no false claim of owned-process adoption or externally controlled providers |
| Build/CI | Rust 1.100 compatibility, Linux `cargo validate --check`, `cargo smoke` and focused process/browser integration tests; Windows manual pre-release gate |

Source evidence for browser-origin risk: [Todo local boundary](../todo/http-local-browser-contract.md), [MDN local network access](https://developer.mozilla.org/en-US/docs/Web/Security/Defenses/Local_network_access). OS IPC documentation: [Tokio Unix peer credentials](https://docs.rs/tokio/latest/tokio/net/struct.UnixStream.html#method.peer_cred), [Windows named pipe options](https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/struct.ServerOptions.html).

### Bounded first-release policy proposal (explicit decision requested)

To avoid leaving every implementation detail open, the following is a **specific candidate** for first acceptance; none of these are facts about current Riesel:

- **Deployment and trust:** Linux, one trusted interactive OS user on a machine without adversarial local processes; no remote/browser-origin trust. Run a separate Manager foreground/service process only while needed. No multi-user host or internet-exposed HTTP claim.
- **Source of applications:** an operator-edited, validated allowlist of executable paths and immutable per-application arguments; define trusted file ownership and resist pathname replacement at launch (a path string is not an executable integrity guarantee). No executable uploads, shell interpolation, arbitrary command entry, network discovery or mutable registration API in the browser.
- **Ownership:** only start, stop and inspect applications launched by *this* Manager instance. Each application keeps owning its own provider processes. An app already running outside Manager is displayed only if independently and explicitly inspectable, never adopted or stopped.
- **Readiness:** distinguish `spawned/running` from `application ready`. Unless a separate app-level readiness signal is implemented, display `readiness unknown` rather than inventing proof. Provider readiness does not establish application readiness.
- **Manager failure:** normal shutdown attempts to terminate owned direct children, not necessarily descendants; after abnormal Manager loss, do **not** reattach/kill by PID alone. Mark prior state unreconciled and require explicit manual inspection/cleanup or verified OS-level containment before restarting a potentially duplicate app. Automatic system-daemon resurrection is excluded.
- **Commands:** generation-fenced start/stop for allowlisted applications only. A response timeout is not a reversal; clients fetch observed state/command outcome and never automatically resubmit a mutation.
- **Local control transport:** on Linux, evaluate a Unix socket inside an operator-owned private directory, requiring client OS identity/credential checking and bounded framed requests. Checking the peer UID identifies an OS user, not a trusted application or another process sharing that UID. Do not infer owner authorization solely from socket pathname. A Windows equivalent needs explicit ACL/remote-client tests ([Windows named pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)).
- **Browser access:** the Manager may serve one *separate*, same-origin loopback UI bridge to the same authorization/operation layer; no direct arbitrary executable control endpoints. Apply strict Host/Origin checks, no CORS, bounded requests and a short-lived operator-bootstrapped session secret; avoid URL query-string credentials, logging secrets and auto-starting Manager on arbitrary page visit. Treat session bootstrap and hostile local-client tests as mandatory before enabling browser mutations. A read-only dashboard plus authenticated local CLI is the safe fallback if the bridge is not proven.
- **Scope tradeoff:** local MVP with explicit limitations is preferable to claiming cross-user or remote security without evidence. The maintainer may instead require authenticated multi-user management, but that is a distinct larger product gate.

This narrows the decision to an identifiable single-host product and gives engineering acceptance tests a stable target. It does **not** certify Unix credential policy, token handling, UI bridge bootstrap or crash recovery; those must be implemented and tested in the accepted security scope.

## 7. Dependencies, decision risks, and proposed delivery slices

1. **Must decide before acceptance:** whether first delivery really requires app **start/stop** versus read-only visibility; whether Manager is allowed to own launched app processes; trusted operator and local-client threat model; a safe authorization mechanism; application-level readiness and Manager-restart ownership policy. These are material and cannot be silently treated as implementation details.
2. **Possible phase order after acceptance:** (a) management identity/state semantics and app report contract; (b) ownership and application-level readiness with CLI tests; (c) secured local control API; (d) same-contract browser UI and end-to-end lifecycle tests. Split issues only where independently verifiable.
3. **Parallelism:** UI interaction design can be explored after API schema/semantics are accepted; UI deployment/testing depends on the secured control bridge. Native window host [#60](https://github.com/aschenrot/riesel/issues/60) is **not** a prerequisite.
4. **Excluded:** containers/Podman/Docker, distributed orchestration, internet-exposed management, generic auto-discovery, infrastructure monitoring, multi-user permissions and a custom UI framework. Such features need their own scope and security acceptance.

## 8. Critical review and bounded feasibility evidence

**Review recommendation: REVISE before Gate B.** The research comparison identified a plausible local Manager but originally under-specified full process-tree cleanup, executable-inspection side effects and the limitations of per-UID authorization. The new architecture-first audit also found that OS-owned process units and a hybrid operational contract were omitted from the original comparison; selecting direct Manager children alone may create avoidable process ownership and recovery infrastructure.

**Focused OS-level probe (Linux 6.18, Python 3.13; NOT a Riesel or Rust integration test):** a short launcher created a `sleep` process and exited immediately via `os._exit(0)`; the child PID was still alive afterwards and was explicitly terminated. An AF_UNIX socket pair also exposed a matching OS peer PID/UID/GID using `SO_PEERCRED`. These results establish that parent exit does not itself guarantee descendants are terminated, and that peer credentials can be queried on this Linux environment. They do **not** prove cross-platform service management, Rust integration, same-UID adversary isolation, or an effective OS containment/recovery policy. References: Linux [unix(7)](https://man7.org/linux/man-pages/man7/unix.7.html); [Windows named-pipe ACLs](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights).

**Remaining decision-critical evidence:**

1. State whether stop promises direct-child exit or **all descendant/provider cleanup**; if all, test OS-level groups/cgroups/job ownership against abrupt Manager and app loss and PID reuse. Otherwise report cleanup as incomplete/unknown.
2. Delimit passive inspection versus executing trusted code; defend executable/config path replacement, arguments, secrets and permission checks.
3. Define *whole-application readiness* separately from OS spawn/provider readiness; test unknown and failure reporting.
4. Define threat model separately for foreign web origins, other OS users and **hostile same-UID processes**. A UID check or same-user-readable session token cannot exclude the last class. Secure browser bootstrap and command authorization remain unproven.
5. Agree on exactly which first-slice actions are required; CLI-only mutations plus a read-only UI may be a legitimate intermediate step, but are **not** completion of a full Manager UI.

Only implementation tests on the accepted platform and exact revision can close these gates. Research probes do not.

### Architecture-first addendum — process ownership and growth (2026-11-20)

**Missing candidate evidence:** direct child handles are not the only process-owner model. On Linux, [systemd transient service/scope units](https://www.freedesktop.org/software/systemd/man/latest/systemd-run.html) may provide service-level observations and scoped process ownership. On Windows, [Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects) support grouped process management; on macOS, [launchd service management](https://developer.apple.com/documentation/ServiceManagement) has distinct ownership rules. These are **documented options, not Riesel integration tests or guaranteed transitive cleanup**. Test launch, process tree, restart, exit and authorization in the supported OS environment before relying on one. OS units may impose operator/deployment overhead unsuitable for one-shot `cargo run`; no universal OS-service dependency is accepted.

**Comparable growth scenario:** a developer runs one foreground app during development, then installs it as a managed service, with an independent GUI attached. The same app *definition*, runtime *session generation*, and optional exported capability should retain their meanings; the **process owner may change** from the CLI to an OS service manager. Cross-product [Issue #75](https://github.com/aschenrot/riesel/issues/75) owns the semantic comparison, not process-control policy or the Manager UI. Record a migration path from B to E/F if B wins, or explicitly accept its limitations.

**Verification expansion:** assert that a passive `status/inspect` never executes the app; a separate explicitly requested executable inspection *does* execute trusted code and is never advertised as side-effect-free. Exercise OS-unit ownership versus direct-child shutdown, abrupt controller failure, orphan reconciliation and stale-generation commands. Distinguish **service health**, **app session health** and **interface readiness**. Neither CLI nor UI can terminate an externally owned process by inference from a PID, hostname or registration label.

## 9. Proposed Gate B outcome — pending owner review

**Recommend REVISE, not unconditional acceptance yet.** Compare a dedicated Manager-owned child, an OS-owned application unit and a hybrid model **on equal terms** against the original operator outcome, including future installations, restart recovery and migration cost. Then resolve authority, process-tree guarantee, passive versus executable inspection, application-level readiness and local authorization. Keep browser mutations deferred if their trust boundary is unproven. Do not prematurely approve a full Manager daemon, persisted registry or service installation.

The maintainer must choose **accept / revise / defer / reject** and record rationale on [#59](https://github.com/aschenrot/riesel/issues/59). Only an accepted decision may update [riesel.md](riesel.md), a durable architecture contract, [implementation-roadmap.md](implementation-roadmap.md), implementation Issues or an optional GitHub Milestone. This research document must not be cited as accepted architecture until then.
