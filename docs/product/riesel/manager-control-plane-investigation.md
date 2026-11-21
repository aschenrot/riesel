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
| **D. Distributed/global orchestrator** | Multiple nodes, leases, scheduling and inter-host failure handling | Requires remote identity, authorization and durable coordination | Much larger security and reliability surface | Defer; not a Stage-6 prerequisite for local Manager |
| **E. OS-managed application unit as the process owner** | On supported hosts, an OS service/scope manager can own and report the actual app unit across a Manager UI restart | Platform-specific registration, permissions and APIs; no guaranteed portable identity or security | More upfront integration, potentially stronger lifecycle containment/reconciliation and less custom supervisor logic | **Must compare before selecting B** |
| **F. Hybrid: Riesel management semantics with pluggable process ownership** | Shared identity, observations and command contract; one of direct foreground child or OS service-unit ownership selected per deployment | Need a single unambiguous owner; do not split one app between two controllers | Potential long-term flexibility without forcing OS units on a developer command | Investigate against B/E; not automatic abstraction requirement |

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

To avoid leaving every implementation detail open, the following is **one candidate based on direct Manager-owned children (B)**; none of these are facts about current Riesel. It is **not** a decision against OS-owned app units or a hybrid process-owner adapter (E/F), which must be compared under the same operator lifecycle, portability, security and restart/reconciliation criteria:

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


## 10. Manager Gate B decision packet — 2026-11-21 (main `0798a6fb`, proposal)

This addendum is a **reviewable proposed specification, not acceptance**. It incorporates [#75](application-operational-boundary-investigation.md)'s tested independent workspace/typed-export evidence without treating the counter facade as a management API. It also adds a reproducible POSIX comparator: [`research/process-ownership/process_tree_probe.py`](../../../research/process-ownership/process_tree_probe.py). Run `python3 research/process-ownership/process_tree_probe.py` on Linux: in the tested sandbox, `parent_exit_leaves_child_running=True` and `explicit_process_group_kill_stopped_child=True`. This only checks a cooperative process-group shape, **not** Riesel supervision, adversarial descendants, systemd, OS reboot or Windows jobs. Do not infer transitive cleanup from `ProcessRegistry::stop`.

### 10.1 Full target versus first independently useful product

**Target product:** a Manager that operates explicitly registered Riesel applications through one authoritative command/inspection contract, with an operator UI and CLI presenting the same state. Its responsibility is application registration, process-owner coordination and authorization of state-changing operations, not SDK provider selection, component hosting or generic GUI rendering.

**Proposed first delivery, subject to maintainer acceptance:** single-host, explicit trusted registrations, passive registry/definition inspection, observed process and app-interface status, manual **start and stop where ownership is proven**, bounded diagnostic history and one local browser operator UI. Do not silently downgrade "Manager" to a read-only dashboard while claiming lifecycle management is delivered. Conversely, begin with a read-only UI plus a secured CLI control channel *as a separately labeled intermediate slice* if browser command authorization cannot be demonstrated.

| Operation | First-delivery proposal | Boundary / deferred |
| --- | --- | --- |
| Register/remove | Operator-controlled trusted allowlist with stable registration ID, executable/config provenance, ownership policy and duplicate rejection; file-based setup acceptable | Browser arbitrary command upload, ambient discovery, package installer |
| List/inspect | Passive cached definition/config + fresh observation with explicit timestamps; executable inspection requires a separately authorized trusted launch | Unknown binary scan, live introspection of stopped process |
| Start/stop | Authorized generation-conditional operation against the actual owner with command-status query | Force-killing external PIDs, transparent duplicate-run adoption |
| Restart | Explicit stop followed by new launch attempt **only after** old ownership is resolved | Silent "restart" of unknown/orphan generation |
| Status/health | Distinguish process alive, provider health evidence, app session and public interface readiness; report unknown/stale | Inferred whole-app health from PID or provider `RIESEL_READY` |
| Diagnostics | Bounded sanitized status/exit/errors and control-command results | Distributed logging/tracing and guaranteed durable audit trail |
| Configuration | Display active selected/declared configuration; modify only through a separate, deliberate trusted operator action if accepted | Implicit config hot swap or second provider resolver |
| Recovery | Explicit owner reconciliation; fail closed on orphan/unknown generation | Automatic adoption by PID, across-host failover, idempotent unknown mutations |

### 10.2 Process-owner comparison and explicit policy choice

| Candidate | Normal start/stop | Manager crash / restart | Portability and long-term tradeoff |
| --- | --- | --- | --- |
| **Direct `ProcessRegistry` child ownership** | Existing Rust primitive handles one direct child and graceful/kill fallback | Abnormal Manager death may orphan application; app death may orphan grandchildren; PID insufficient for safe adoption | Low initial cost; weak installed-service reconciliation if relied on alone |
| **OS service/scope/job owner** | Delegate to an OS unit/job with a verifiable stable owner reference and authorization | A service unit may persist independently of UI/Manager, while Windows kill-on-close jobs may instead terminate; platform policy is decisive | Stronger potential ownership boundary and resource controls but different APIs/install requirements |
| **Hybrid shared control contract + one selected owner per app** | Same operator semantics; either OS unit owner or explicit foreground child owner, never two simultaneous supervisors | Observed owner kind determines whether restart reconciliation is possible; no fallback PID guessing | Recommended **target architecture candidate** because installed and development uses have different legitimate ownership, but added interface/test burden needs proof |
| Embedded per-app endpoint | Cannot start an app while it is stopped | Vanishes with app; not a Manager | Useful diagnostics adapter only |
| Distributed orchestrator | Outside local scope | Requires remote consensus/fencing not accepted | Defer |

**Recommendation:** evaluate hybrid ownership as the long-term architectural target, but make the *first supported ownership mode an explicit release decision*. For durable installed applications, prefer testing OS-controlled units (Linux systemd transient service/scope; Windows Job Objects have **different** crash semantics). For an ephemeral foreground Manager, direct children are defensible only with an openly bounded crash/orphan guarantee. Neither choice permits an unverified claim of full process-tree containment. A child process created with `start_new_session` / `setsid` and `killpg` only provides a conditional group strategy, not a complete service-manager replacement. Source comparison: [systemd-run](https://www.freedesktop.org/software/systemd/man/latest/systemd-run.html), [systemd scope](https://www.freedesktop.org/software/systemd/man/latest/systemd.scope.html), [Windows jobs](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects).

**Required policy decision:** select whether the **first actual Manager** promises (A) top-level owned-child shutdown with unresolved descendants reported, or (B) proven contained application unit cleanup and restart reconciliation. The product definition's existing planned "supervision" should not be interpreted as a demonstrated guarantee of B. Do not blend opposite Manager-crash semantics as if equivalent.

### 10.3 Candidate authoritative management contract and threat model

**Contract shapes are conceptual, not a published API.** The Manager's registration points to a trusted executable/build/config and selected process-owner policy; application runtime separately owns its provider selection. Manager-issued launch attempt and command identifiers are separate from runtime-generated app session epoch and published interface identity (see [#75](application-operational-boundary-investigation.md#11-coordinated-decision-packet--2026-11-21-main-0798a6fb-proposal)).

- **Queries:** `list/inspect/status` never invoke app code implicitly. Each item states observed-from owner, observation timestamp/freshness, process status, any reported app-generation status, public-interface readiness evidence, terminal errors and `unknown/unreconciled` if provenance is lost. Explicit executable inspection is a distinct side-effecting **trusted launch**.
- **Commands:** `start(registration, expected_state, command_id)`; `stop(registration, expected_attempt_or_generation, command_id)`; optional `command_status(command_id)`. Validate caller permission **before** executing, atomic state/owner preconditions and no double-start. A command's acceptance is not completion or app readiness. Retain bounded command state and communicate that retention can be lost on Manager crash unless durably journaled.
- **Conflicts and uncertainty:** reject `not_registered`, `not_authorized`, `stale_generation`, `already_running`, `wrong_owner`, `owner_unreconciled`, `app_not_ready`, `unsupported_version`, `resource_limit` and `unknown_outcome` with origin and safe next action. Error identifiers are examples, not a frozen enum. A timeout **never** means a mutation did not execute; fetch status or require deliberate operator reconciliation.
- **Failure/restart:** persist registration identity and desired policy separately from last observed state. On restart, verify process-owner identity and active attempt **through the owner** (not a stale PID); if not provable, block a duplicate start and display `unreconciled` until operator intervention. Remote/external processes remain view-only unless a separately accepted owner contract exists.

**Security baseline proposal:** one controlled OS operator, no untrusted same-UID processes, no public network. The control service must enforce allowlisted commands, bounded frames, path/executable validation, config secret isolation, log redaction and caller authorization. Private Unix sockets and peer credentials can identify a local UID but **not distinguish a hostile process running as that UID** ([Tokio UnixStream peer credentials](https://docs.rs/tokio/latest/tokio/net/struct.UnixStream.html#method.peer_cred)); same-origin Host/Origin defenses for browser requests do not authenticate the operator. A browser control bridge must use a separate validated operator bootstrap/session, CSRF protection, strict Host/Origin and bounded request rules; tokens cannot be exposed through URLs, logs or arbitrary web origins. If this is not demonstrated, allow **read-only browser UI** and authenticated local CLI only, clearly marking incomplete browser lifecycle control. A multi-user or hostile same-UID threat model cannot be promised without stronger separation.

**Interface choice:** start with a local browser frontend consuming the same operations as CLI, because it works while headless and does not depend on #60's native toolkit. A native Manager client is a replaceable future consumer, not a prerequisite or competing backend. A dashboard displays stale/unknown with explicit timestamps and never conflates registration, process, app session, provider and interface readiness.

**Application shutdown integration is a separate contract:** `ProcessRegistry::enable_graceful_shutdown` is opt-in after a managed provider's readiness handshake; it does not prove a top-level Riesel application supports that stdin control. The existing Todo HTTP app waits on `tokio::signal::ctrl_c()` instead. A Manager or systemd unit stopping an arbitrary app therefore needs an **app-level verified graceful-stop interface or tested signal policy**; SIGTERM, SIGINT, provider stdin shutdown and force-kill must not be conflated. Add a stop test demonstrating application-owned task/provider drain and explicit timeout/escalation, rather than verifying only that the OS process disappeared.

### 10.4 Executable acceptance and work decomposition after acceptance

1. **M1 — registration and passive inspection:** canonical ID, duplicate/path swap rejection, cached artifact provenance and exact SDK definition, no spawn on list/inspect, explicit executable-inspection side-effect marker. Test independent Cargo-built app and Todo.
2. **M2 — process owner:** select direct-child versus real OS-unit mode; test normal/abnormal Manager death, app exit, descendants, escaped children, owner restart, stale generation, duplicate start, no external kill, bounded escalation and systemd unavailable. Use [process-tree probe](../../../research/process-ownership/process_tree_probe.py) only as preliminary *negative evidence*.
3. **M3 — app-level readiness and command semantics:** provider handshake alone cannot mark whole app ready; establish explicit app/interface generation and bounded observation; concurrent start/stop and lost-response tests guarantee no silent mutation retry.
4. **M4 — local authorization:** forbidden operation, untrusted separate UID, same UID within the *declared* threat model, hostile browser origin/Host, token leak, malformed input and executable path replacement; no state transition before policy checks.
5. **M5 — UI/backend parity:** one shared API implementation, CLI/UI status and errors agree, keyboard navigation and accessible error/focus states, reload/reconnect do not restart apps, headless operation remains valid.
6. **M6 — exact revision verification:** Linux `cargo validate --check`, `cargo smoke` and new integration tests; Windows pre-release manual Job Object/named-pipe/UI review if claimed. CI-green cannot substitute for the selected process-owner failure experiments.

**Possible independent accepted slices:** (a) identity/registration/passive observations, (b) selected owner lifecycle and crash recovery, (c) secured command contract, (d) browser UI on the same backend. UI layout can be designed independently after schema agreement; browser write actions must wait for secured M4. Manager does not depend on the native window-host product or on #83's scheduler.

**Maintainer decisions:** ACCEPT / REVISE / DEFER / REJECT the first operator outcome, the first owner/crash guarantee, local threat model, explicit registration policy, command retention, app-ready proof and browser-mutating scope. Do not close #59, introduce a daemon, new crate, implementation issues or milestone until these are accepted.


## 11. Manager M1/M2 ownership and authorization research — 2026-11-21

**Status: review candidate, NOT an accepted process-owner adapter or authorization to implement.** Base reviewed: main `a35ac7c9ac9749ff023a72595c6851caee5f4859`. This section specializes #59 and does not modify #75's shared identity/readiness semantics, #68's developer launcher, or the application's provider-selection authority. The earlier Gate B packet and accepted *product direction* remain in force; concrete OS and management mechanisms remain a #59 decision.

### 11.1 Recommendation: first installed Manager ownership mode

**Recommended first supported *installed-application* owner: Linux systemd user-manager transient `.service` units, conditional on real systemd-backed M1/M2 verification.** This is a recommendation to accept/reject at #59, **not** a claim that the adapter works today. Riesel Manager would be the *authorized controller/observer*, while systemd would remain the *sole OS process owner*. The Manager browser and CLI must not also manage a private `Child` handle for the same launch. A process already owned by another controller is view-only or refused, never silently adopted.

Why a **service** rather than a `--scope` as the initial installed mode: a transient `.service` is started by the OS manager rather than requiring an already-running caller to place externally spawned processes into a scope. That better fits the intended application survival across Manager crashes and deliberate reconciliation through OS-owned units. A scope remains a separately testable adapter, not an equivalent default. Use a private, reserved unit-name namespace plus a trusted registration-to-unit mapping; neither a PID nor a matching unit name alone proves the intended registered build. For service instance verification, require authoritative unit state, executable/provenance checks and a launch incarnation (such as systemd's `InvocationID`) with generation-conditional commands. Verify all relevant D-Bus properties/races on a real supported systemd version before freezing the implementation.

**Proposed contract if verified:** on a host with a functioning user-manager bus, an authorized Manager can request a trusted, explicitly registered executable to launch in one OS-owned service unit; it observes unit-active/exited/failed and the specific invocation. Ordinary Manager-process termination/restart does not *by itself* kill that OS-owned service; on restart, it queries the authoritative OS unit before issuing commands. Stop addresses an exact registered unit/invocation, not an ambient PID; configured `KillMode=control-group` and deadlines are independently verified for ordinary descendants. This does **not** guarantee survival if the user systemd manager dies, the user session is torn down without linger, the host reboots, a process deliberately escapes its cgroup, or the app has not reported its own readiness. Nor does it establish automatic reboot activation, exactly-once launch, or authenticated app exports. If the required unit/identity verification fails, the Manager must report `unreconciled` and refuse mutating actions rather than adopting or killing a guessed process.

**Environmental gate:** when systemd `--user` is unavailable, report `owner_unavailable` and **do not silently switch** a registered service-owned app to a direct-child owner. An opt-in direct-child **ephemeral development/preview adapter** can be specified separately, explicitly guaranteeing at most top-level managed-child stop while the owning controller is alive; crash or descendant cleanup is unknown and duplicate start may require manual reconciliation. That limited adapter is *not* a completed installed Manager supervision guarantee. This prevents a low-cost first implementation from freezing an unachievable service promise.

**Long-term hybrid:** select exactly one owner per registration/deployment, with an explicit policy describing survival and cleanup. Linux installed service units can be extended to supported system-manager installations where authorized; Windows installed applications require separate SCM/service-owner evaluation (Job Objects alone are not a persistent OS service), and macOS requires launchd/SMAppService evaluation. A Windows job configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` favors fail-closed process cleanup on its last handle closing; it is not an app-survives-Manager-crash/restart adapter. Do not promise one portable fate across incompatible platforms. Foreground developer tooling (#68) remains independent of this Manager installed mode.

Primary documentation: [systemd-run](https://www.freedesktop.org/software/systemd/man/latest/systemd-run.html), [systemd.service](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html), [systemd.kill](https://www.freedesktop.org/software/systemd/man/latest/systemd.kill.html), [systemd unit D-Bus API](https://www.freedesktop.org/wiki/Software/systemd/dbus/), [Windows Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), [Windows AssignProcessToJobObject](https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-assignprocesstojobobject), [macOS SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice). **These are documentation evidence, not executed platform experiments.**

### 11.2 Equal-criteria comparison of concrete process owners

| Criterion | Direct `ProcessRegistry` child | systemd user `.service` owner (proposed Linux installed mode) | Hybrid one-owner contract |
| --- | --- | --- | --- |
| Normal start/stop | Direct `Child`; provider-only stdin stop does not apply to arbitrary apps; kills one direct child | OS `StartTransientUnit` / `StopUnit`; independently confirm unit, incarnation and cgroup stop policy | Delegates to exactly one selected owner; does not implement an extra second supervisor |
| Manager crash | Handle lost; intentionally detached app can remain; synchronous drop cleanup cannot run on abrupt death | OS unit can remain while user service manager survives; must verify on a real bus | Explicit per-owner fate; never conflate survival with kill-on-controller-exit |
| Restart reconciliation | No handle transfer; PID/start time are evidence, not authority to adopt/stop | Re-query existing owned unit and invocation; verify provenance before control | Stable registration maps to selected owner; unknown ownership blocks start/stop |
| Application crash | Top-level exit observable only while handle exists; descendants may survive | Unit failure/restart policy and cgroup lifetime must be tested; no implicit restart chosen | Report app/process failure independently of Manager UI lifetime |
| Nested descendant cleanup | `Child::kill` is not tree containment; `killpg` only affects current group | Default cgroup-based containment is promising but must test child `setsid`, app exit, stop and escalation | Report scope-specific cleanup evidence, residuals and `unknown` |
| Duplicate launch | Registry key only prevents duplicates inside one live Registry; controller-only lock expires on crash | OS unit admission helps serialize by name but unit reuse/restart/race and impersonation need active verification | Generation and expected owner state fenced before side effects; no fallback duplicates |
| Identity/stale PID | Direct handle authoritative only during process lifetime; PID alone reusable | Unit name, validated registration/provenance and invocation together; do not equate ActiveState with app-ready | Stable application ID, artifact, attempt, runtime session, provider instance and command ID remain distinct |
| Portability/installation | Cross-platform process APIs; weak durable installed semantics | Linux systemd user bus required; operator policy, login/linger, version and permissions affect availability | More adapters and tests; supports independent Cargo and future installed-mode differences without masking guarantees |
| Authority/security | Caller command launch is sensitive; same-UID threats remain | User D-Bus rights may manage more than Riesel's allowlist, so Manager must never expose arbitrary unit or executable control | All CLI/UI mutations route through one authorized Manager policy; not a second provider resolver |

**Rejected as a *complete installed Manager default*:** unqualified direct-child ownership, `setsid`/process groups alone, PID files, controller-held `flock` alone, and a Windows job presented as a cross-restart persistent service. These may be useful building blocks under narrower explicitly stated guarantees. **Not rejected categorically:** OS scopes for developer sessions or Windows jobs for fail-closed contained launch; each is a separate mode requiring evidence.

### 11.3 Executed research-only tests and exact limits

Added [`research/process-ownership/manager_owner_probe.py`](../../../research/process-ownership/manager_owner_probe.py), executable with `python3 research/process-ownership/manager_owner_probe.py`. Its Git blob SHA is `f057185ef26e257a17fc0840702cea1436f44f97` (same bytes as the locally tested fixture); local SHA-256 `5af95dab77f0e37c86af5aca4ccf8df159f9ea42f1ff18c6680696c2c7371441`. **9/9 research-only tests passed** in the Linux sandbox, Python 3.13.5, Linux kernel 6.18.57, 2026-11-21. This environment's PID 1 was `supervisord`; `systemctl --user show-environment` failed with "No such file or directory"; cgroup v2 existed but was mounted read-only. No systemd unit, Windows job, macOS service or Riesel binary was exercised. Cargo/rustc were not installed; local Rust `cargo validate --check` and `cargo smoke` could not run here.

| Probe | Result in isolated fixture | What this **does not** establish |
| --- | --- | --- |
| 01: direct-owned process start/stop | PASS: cooperative app started; explicit SIGTERM exited gracefully | Riesel application stop hook, provider drain or startup readiness |
| 02: owner crash | PASS: deliberately detached live app survived abnormal controller `os._exit(23)` | All Manager crashes or all deployment models |
| 03: controller restart observation | PASS: a fresh controller process observed the surviving app and returned `unreconciled` without adoption/stop | Safe adoption, actual service-owner reconciliation or ability to stop it |
| 04: app + escaped descendant | PASS: killing top-level app left a child launched in a separate session alive | cgroup/service-unit containment or arbitrary adversarial children |
| 05: wrong-owner/stale stop | PASS: model validator refused changed registration, owner, generation and PID birth marker without killing app | Production manager authorization, POSIX PID-reuse race safety or OS identity attestation |
| 06: duplicate start after owner crash | PASS: lock refused competing acquisition while owner lived; after crash `flock` released, surviving app plus a second launch coexisted | Proper OS-unit atomic duplicate-start prevention |
| 07: graceful vs escalation | PASS: cooperative SIGTERM exit and stubborn SIGKILL fallback after timeout | Application-level Rust graceful drain or bounded hooks |
| 08: lost command response | PASS: command effect recorded, response intentionally withheld, caller recovered result only via status lookup | Atomic durable journal, exactly-once external side effects or Manager crash recovery |
| 09: same-UID clients | PASS: Unix `SO_PEERCRED` identified distinct PIDs with the same UID | Permission checks against different UIDs or browser authorization |

The earlier [POSIX process-tree probe](../../../research/process-ownership/process_tree_probe.py) remains independent negative evidence. The new fixture uses temporary directories, local processes, and PID-birth-checked cleanup; it does not operate on installed applications. A model refusing stale operations is a **policy test**, not proof of production enforcement. Passing Linux CI on this fixture would verify reproducibility only.

### 11.4 Proposed authoritative management contract (shape only)

One internal command/observation coordinator must serve both CLI and browser adapters, without either frontend authorizing OS process operations independently:

| Entity | Authority / lifetime / required observation |
| --- | --- |
| Registration | Trusted operator; stable opaque ID within one Manager registry; resolved immutable executable/build provenance, vetted config/args/working directory, selected OS owner and policy |
| Artifact/build | Builder/trusted installer; capture and verify content/version/provenance before launch; reject path swaps/symlink races, untrusted mutable artifacts and arbitrary shell invocation |
| OS owner/attempt | Actual selected process owner issues or verifies new attempt ID; OS unit identifier and invocation if systemd; PID is diagnostic only |
| Riesel application session | Application runtime, not Manager; can restart inside unchanged OS PID; Manager accepts only authenticated, generation-scoped readiness reports coordinated with #75 |
| Provider instance | Owned/selected by application runtime; Manager may display reported metadata, never resolve, kill or re-place providers |
| Published interface | Opt-in app-owned interface; `bound` is not `callable`; readiness includes version, app session and evidence timestamp; a running unit is not interface-ready |
| Management command | Manager creates/adopts an authorized command ID scoped to a retention epoch; records accepted/running/completed/failed/unknown separately from actual OS observation |

**Queries:** `list`, `inspect(registration)`, `status(registration)`, `command_status(command_id)` (names illustrative). Pure queries must neither execute application code nor mutate observed process state; return evidence source/time, `unknown` or `unreconciled` explicitly. Read trusted cached SDK definition only with provenance; launching `--print-definition` is a **separate authorized execution**, bounded and audited, not passive inspection.

**Commands:** trusted `register/remove` through operator-owned configuration or authenticated local administrative path (first mode: file-based allowlist, no browser arbitrary registration); `start(id, expected_absent_or_owner_state, command_id)`; `stop(id, expected_attempt_and_owner_invocation, command_id)`; `restart` *optional*, only as verified stop then a new start. Authorization and exact expected-owner/generation checks must happen **before** process/service side effects. Serialize conflicting commands across CLI and UI and compare the authoritative OS state again; do not assume an in-memory mutex or registry prevents cross-process duplicates. Refuse `wrong_owner`, `stale_attempt`, `unreconciled`, `already_running`, `owner_unavailable`, `not_authorized`, `registration_changed`, `unsupported_operation` or `unknown_outcome` with safe diagnostic state and no arbitrary PID kill.

**Crash/unknown result:** sending a stop or start to an OS owner may succeed even if the Manager loses its response. Command IDs deduplicate only within a *declared, verified durable retention boundary*; if the Manager journal does not survive a crash, `command_status` must explicitly return `unknown/expired`, and the caller must reconcile fresh authoritative OS state. Never auto-replay a potentially executed start/stop (or app mutation) solely because TCP/IPC timed out. Any persistent journal must define atomic admission versus side-effect ordering, durability/fsync, crash windows and bounded retention rather than advertising exactly-once guarantees. The browser reloads a snapshot instead of replaying commands.

**Graceful stop:** separate Riesel application-level cooperative drain (application work, app hooks and managed provider teardown) from provider-host `RIESEL_SHUTDOWN`, Todo's current Ctrl-C handler and OS `SIGTERM`/service escalation. Specify a verified application-wide deadline and observable `graceful`, `forced`, `failed`, `residual_unknown` outcomes before claiming clean lifecycle management. Any stop timeout followed by uncertain OS kill yields `unknown` until owner inspection.

### 11.5 Trust and failure matrix

**First threat model proposal:** one trusted local OS operator, explicitly trusted installed executables/configuration, no public network, no hostile same-UID processes. State this limitation in product UX and documentation; it does not provide isolation *within* the same user account. If hostile same-UID or multi-user isolation becomes a requirement, introduce a separately privileged service boundary with independently protected credentials and OS permissions, not another check of `SO_PEERCRED`.

| Actor/event | Authorization/failure rule | Required counterexample test |
| --- | --- | --- |
| Unregistered or replaced executable | Only operator-installed allowlisted artifact/config; immutable or verified open/exec provenance; reject path swap | Replace symlink/build after registration but before launch; verify refusal |
| Untrusted different UID via CLI socket | Private directory and Unix socket permissions plus OS peer credentials; deny unauthorized user | Separate-UID connect/register/start attempt rejected before any side effect |
| Hostile same UID | **Out of first supported security claim**: Unix UID and same-user-readable tokens do not distinguish it | Demonstrate limit explicitly; if required, test independent trust separation |
| Hostile web origin / DNS rebinding | Strict Host/Origin/Fetch Metadata; no permissive CORS; CSRF token/session/bootstrap for writes; local address alone is not authorization | Cross-origin form, DNS rebinding, credential/token leakage and CSRF suite |
| UI/CLI duplicate or stale command | One operation coordinator, conditional owner/generation state, durable command ID within advertised epoch | Two simultaneous CLI/UI launch requests, stale stop after replacement invocation |
| Manager crash during start/stop | OS owner reconciled first; command outcome remains unknown until confirmed | Kill Manager before owner accepts, after accept, before acknowledgment, after effect |
| App crashes with grandchildren | OS owner provides unit/cgroup cleanup evidence or reports residual/unknown | App kills itself; escaped `setsid` child; noncooperative descendants |
| Unit/user-manager disappears | Cannot assume systemd running, lingering, reboot survival or automatic service re-creation | Kill/restart user service manager and log out; verify exact recorded guarantee |
| App reports provider ready but interface fails | Separate bounded, authenticated app-export/interface readiness probe | Provider frame accepted, public handler unavailable, session generation changes |
| Invalid or oversized request / logs | Bound input size, concurrency, timeouts, executable args; redact credentials and secrets | Fuzz malformed frames, exhaust rate limits, inspect diagnostic redaction |

**Browser sequencing:** First implement secure CLI controls and a read-only, same-origin Manager browser consumer of the **same state backend**, if this permits independent verified progress. A read-only UI is an **intermediate slice only**, not completion of the accepted Manager lifecycle UI. Browser start/stop may be added only after a tested operator bootstrap, CSRF/origin/Host protections and shared authorization policy; an OS UID-checked Unix socket cannot be called directly from an ordinary web page.

### 11.6 Required real-platform tests before owner-mode acceptance

**Linux systemd user-manager test plan (NOT EXECUTED in this container):**

1. Run on a controlled Linux host with `systemctl --user` and D-Bus operational, a non-root test user and a disposable unit namespace. Record `systemd --version`, unit properties and exact commit. Separately test a session without linger and, if claimed, a configured lingering user.
2. Launch a trusted, verified fixture through a **transient `.service`** with explicit `KillMode=control-group`, stop timeout, resource limits and chosen service `Type`; capture unit path, main PID, `InvocationID`, control-group membership and startup result. Show a different result for process-spawn versus actual app-interface readiness.
3. Test an ordinary stop, graceful cooperative shutdown and forced escalation. Verify the app's own drain marker, OS unit inactive state, and descendants including a `setsid` child. Inspect residual cgroup processes rather than assuming stop success.
4. Kill only the Manager controller at each state transition; leave systemd running. Reconnect a new Manager, verify exact registration, unit and invocation, and compare app survival and command reconciliation. Repeat after the application itself crashes.
5. Race two Managers with identical registration; test a conflicting unit, unit-name reuse, changed executable, forged/stale registration, wrong invocation stop and missing/expired command journal. Assert no unverified PID adoption and no duplicate.
6. Test user-manager unavailable, restarted and terminated, logout without linger, resource/authorization denial and failure to create or stop a unit. No silent direct-child fallback. Only claim reboot/linger survival if an independently installed persistent unit mode is accepted and tested.
7. For CLI/browser trust, use distinct local OS users, malicious same-UID fixture (documented exclusion), disallowed origins, CSRF, path replacement, credentials not shown in logs and bounded diagnostics. Capture exact result and source revision.

**Other platforms (NOT EXECUTED):** Windows: compare SCM-owned installed services to Job Object controller-bound containment; exercise assignment, nested jobs, breakaway, kill-on-last-handle, ACLs, unexpected Manager death, restart and Windows release checks. macOS: validate launchd/SMAppService registration, GUI user-session lifecycle, signature/distribution restrictions, stop/restart, descendants and authorization. Neither platform inherits Linux results.

### 11.7 Implementation acceptance decomposition, pending explicit #59 approval

- **M1 registration/provenance and passive inspection:** two explicit registrations (independent headless workspace and Todo); reproducible artifact pinning, duplicate/config/path-swap rejection, passive `list/inspect` never launches code; explicit active inspection visibly executes trusted code. Stable ID not PID; no provider resolution.
- **M2 OS owner adapter and exact fate:** live systemd-backed `.service` tests above must pass before calling installed ownership supported. Preserve a scoped alternate direct-child developer-preview mode only if separately accepted. Restart reconciliation must use OS-owned verified invocation, refuse stale/wrong-owner stop and duplicate starts, and surface unknown/residual state.
- **M3 app lifecycle and shared semantics:** integrate #75's accepted **meanings** (do not preempt its concrete protocol): distinct attempt/session/interface identities, bounded authenticated app readiness, task/provider drain, timeout/escalation, generation fencing, app/interface ready versus unit active.
- **M4 single management command engine:** CLI and browser use same internal service; expected-state admission, idempotency/retention epoch, queryable command outcome and no automatic replay; test manager crash in every command window. Persist registry and journal only under an accepted explicit durability policy.
- **M5 protected local UI bridge:** trusted CLI authorization and error model; read-only browser as labeled interim; mutating browser operations enabled only after adversarial origin/CSRF/bootstrap tests and parity tests. No remote/public-network exposure, general provider catalog service or native GUI dependency.
- **M6 release evidence:** exact SHA `cargo validate --check`, `cargo smoke`, reproducible research probes and new real Linux integration suite, plus explicitly scoped manual Windows/macOS release checks if supported. A CI green result on documentation/Python alone is **not** M2 acceptance.

**Decision to record on #59:** (a) accept/revise/reject **systemd user `.service` as the *first proposed installed Manager mode*** with the environmental and crash limits stated above, (b) explicitly accept the trusted-single-UID boundary or require privilege separation, (c) choose Manager command durability/retention and application graceful/readiness protocol gate, (d) confirm read-only browser as interim versus secured mutating-browser first delivery. Until recorded, #59 stays open, this research is not an implementation issue, and no daemon, installer, production API, Milestone or roadmap adjustment is authorized.
