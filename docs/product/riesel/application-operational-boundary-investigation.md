# Shared application operational boundary — cross-product investigation

**Status:** research coordination only. No accepted new SDK contract, crate, CLI, daemon, protocol, Manager, exporter or implementation authorization.  
**Specification:** [#75](https://github.com/aschenrot/riesel/issues/75) · **Reviewed:** 2026-11-20 · **Repository starting point:** `main` at `7456dbce`.  
This report compares common semantics relevant to [developer tooling #68](https://github.com/aschenrot/riesel/issues/68), [Manager #59](https://github.com/aschenrot/riesel/issues/59), and [native GUI #60](https://github.com/aschenrot/riesel/issues/60). **Each product retains its own Gate B decision.** The [delivery workflow](../../project/delivery.md) prefers architecture-first investment when it improves credible cross-product reuse; it does not preselect shared infrastructure.

**S1/S2 follow-up (research only):** [isolated operational identity, session-fence and same-UID threat probe](../../../research/operational-contract/REPORT.md) compares a test-only server-side gate against the real SDK-resolved Counter and identifies unverified **Riesel-owned same-process recovery** and **strong same-UID authorization**. It neither accepts a public exporter nor duplicates the unmerged #87 negative fixtures.

## 1. Product requirement and implementation evidence

**Proposed shared outcome:** independently developed applications can be built, observed, controlled where authorized and connected to optional interfaces through coherent, versioned operational semantics, without each tool inventing a conflicting application identity, session epoch or readiness convention.

The existing [product definition](riesel.md) already distinguishes application, capability, component, provider and deployment and requires consistency of supported interfaces. The proposed cross-product boundary is **not** presently an accepted public SDK/IPC API.

| Existing mechanism | Evidence and precise limit |
| --- | --- |
| [`Application` SDK and app CLI](../../../crates/riesel/src/application/cli.rs) | Can run and inspect the app's own configuration; `--print-definition`/deployment plans are side-effect-free at *preflight*, but running an external binary solely to request inspection executes that binary's Rust startup/configuration code |
| [Typed `Context`](../../../crates/riesel/src/application/context.rs) and [async handler](../../../crates/riesel/src/application/async_handler.rs) | Session-scoped capability handles and task execution exist; the application handler is **not** automatically an externally addressable public interface |
| [Managed protocol host](../../../crates/riesel_protocol/src/host.rs) | `RIESEL_READY` describes a managed **provider** handshake; it is not proof of whole-app readiness, exported frontend reachability or caller identity |
| [Runtime lifecycle](runtime-lifecycle.md) | The runtime owns provider startup, cancellation, task teardown and optional full-session restart; its in-process session token is not an authenticated external session identity |
| [`ProcessRegistry`](../../../crates/riesel_manager/src/manager.rs) | Direct child-process ownership; not a universal service manager, process-tree containment guarantee or safe adoption/reattachment mechanism |
| [Todo HTTP contract](../todo/http-local-browser-contract.md) | Application-specific loopback origin and readiness, persistent Rust domain, independently closable browser; not a general management API or authenticated local IPC |
| [Provider catalog](provider-catalog.md) | Authoritative *provider placement/selection*, not a registry of public application interfaces or Manager-owned app installations |

## 2. Responsibility and identity matrix

**These are candidate distinctions for Gate B, not new types already present in the SDK.**

| Concept | Authority | Scope / lifetime | Why it must not be conflated |
| --- | --- | --- | --- |
| **Application definition** | Rust SDK/application author | Logical app/schema and versioned requirements | Not a running process, filesystem path or opaque PID |
| **Installed/registered app identity** | Trusted operator or installer, if supported | Across launches; tied to approved executable/configuration | Distinct from definition name, binary hash and temporary process |
| **Launch attempt / process ownership** | Foreground CLI, Manager or external OS service supervisor | One attempt and the owning OS process(es) | A PID may be reused; ownership cannot be inferred from metadata or an ambient port |
| **Application session generation** | Running application/runtime | One session; changes on session recovery/restart | In-process `SessionToken` is not a cross-process credential or serializable identity |
| **Provider instance** | App runtime/selected placement | Scoped to the owning app session or external provider owner | Provider-level `RIESEL_READY` does not prove whole-app readiness |
| **Exported application interface** | Explicit application authoring and authorized exposure policy | Contract/version + app generation + endpoint lifecycle | Must not automatically expose every internal provider/capability |
| **Connected client** | Interface-specific authorization and client policy | Bounded connection/authorization lifetime | Same OS UID, loopback, origin or advertised metadata alone may not authenticate privileged operations |

The *same conceptual distinctions* may be reused by different products even if their transport, process supervisor and user experience remain separate. This is not a requirement for one global registry.

## 3. Readiness, inspection and errors

**Observation levels should be reported independently** rather than compressed into a misleading universal `ready: true`:

1. **Declared** — static definition/configuration known; this is not runtime health.
2. **Plan validated** — SDK preflight completed without app/provider startup side effects; still not an external-process health check.
3. **Process started** — OS launch succeeded; not evidence that provider startup or app handlers succeeded.
4. **Providers ready** — required managed components have passed the protocol's metadata/readiness boundary; does not prove public interface listening.
5. **Application/interface ready** — an explicitly supported app-level signal or verified interface probe establishes the relevant operation is available; scope the claim to that specific interface.
6. **Unhealthy, disconnected, stopped or unknown** — reflect actual observation age and errors. A timeout may mean the operation's outcome is unknown, not that it failed or did not execute.

**Inspection purity has two different meanings:** application SDK definition/preflight may be side-effect-free *after a trusted Rust application has been constructed*. A launcher or Manager invoking an arbitrary executable with `--print-definition` necessarily runs some code; this is an intentional trusted execution operation unless cached/verified metadata is used instead. Do not promise that all inspection is passive or that reading a definition proves live readiness.

Cross-product errors must preserve failures such as configuration invalid, unsupported version/capability, ownership conflict, not authorized, stale session/generation, app not ready, transport disconnected and ambiguous command outcome **without falsely guaranteeing exactly-once mutation execution**. No new shared public error enum is approved by this research.

## 4. Architecture alternatives against the same product outcome

| Alternative | Immediate reuse and evolution | Risk / long-term cost | Research disposition |
| --- | --- | --- | --- |
| **A. Independent product adapters over current SDK/runtime** | No core change; each app/tool can reuse existing definitions and process handles | Inconsistent app readiness, identity and exposure logic may recur; migration cost when integrating CLI/Manager/GUI | Useful comparator and escape hatch, not automatically future-proof because smallest |
| **B. Shared, transport-neutral operational semantics, with product-specific adapters** | One explicit identity/readiness/inspection/exposure model can serve CLI, Manager and GUI without a mandatory daemon; allows standalone developer work | Public semantic versioning and API changes must be justified; requires two or more integration fixtures | **Important architecture-first candidate to test**, not selected |
| **C. Universal application-management service/protocol** | Potentially supports installed apps, monitoring, remote tooling and rich UI from one control plane | Unaccepted privilege, discovery, startup, security, transport, persistence and daemon responsibilities; could duplicate runtime and OS process managers | Evaluate where needed, not a prerequisite or presumed evolution of B |

**Critical review:** a facade component, app-specific HTTP adapter and first-class export API should be compared on end-user authoring burden, authorization, versioning, session semantics, cross-interface reuse and long-term migration—not merely whether a local implementation can function. Similarly, a new generic CLI or Manager cannot be justified solely by being more abstract.

Shared semantics **do not imply one shared supervisor**. Cargo builds, a foreground CLI, an operator Manager and an OS service manager can legitimately have different process-owner authorities. The interface contract must expose which owner is responsible and report what it can prove.

## 5. Growth scenarios and extension boundaries

| Credible evolution | Boundary to preserve now | Not required to implement in this investigation |
| --- | --- | --- |
| Headless independent Rust application and Todo with a browser | One canonical SDK definition and app-scoped execution semantics, optional interface metadata | Build system plugin framework, universal JS runtime |
| Independent native GUI process | Explicit exported capability, app/session identity and authorization, reconnect after app restart | Universal renderer or widget tree protocol |
| Multiple interfaces or eventual events/subscriptions | Versioned capability addressing, typed errors, session resnapshot on reconnect, documented event ordering/overflow design questions | Event broker, durable subscriptions or exactly-once delivery |
| Installed app controlled by Manager | Trusted registration versus runtime generation, explicit process owner and authoritative observed status | Automatic adoption, system daemon, remote fleet orchestration |
| Broader platform/OS support | Transport-neutral concepts; platform-specific identity, socket and process ownership adapters | Unsupported cross-platform security or transitive cleanup promises |

For the **Manager process-owner comparison**, research OS-supported alternatives to private child-handle ownership: Linux [systemd-run transient service/scope](https://www.freedesktop.org/software/systemd/man/latest/systemd-run.html), Windows [Job Objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects), and macOS [Service Management/launchd](https://developer.apple.com/documentation/ServiceManagement). These are candidate mechanisms, **not** tested solutions or an accepted dependency. Validate app tree cleanup, exit, ownership of preexisting processes and restart/reconciliation in the actual deployment environment.

For **GUI integration**, either a generated facade component or an explicit SDK export may eventually support versioned requests plus optional event streams; research whether the minimal interface identity/versioning can be extended without changing a published contract. Do not invent an event ordering/acknowledgment policy before a product needs it.

## 6. Decision-critical experiments and coordination

1. **SDK inspection:** run independent Rust apps in separate Cargo workspaces; compare SDK preflight, explicit executable inspection and optional cached metadata, including intentional side effects and failures.
2. **Readiness:** distinguish provider `RIESEL_READY`, Todo `TODO_HTTP_READY`, process spawn and demonstrable public-interface availability. Identify whether an app-level notification can be layered over existing runtime/session without a second supervisor.
3. **Export:** test a generated typed facade across two processes, comparing app-specific HTTP with an optional SDK exporter. Distinguish *absence of a non-exported route* from actual client authorization; verify client exit leaves the backend running.
4. **Generation/unknown outcome:** crash/restart the app and disconnect/reconnect the client around an in-flight revisioned mutation; reject stale bindings and do not auto-replay.
5. **Manager supervision:** compare direct owned-child stop, OS-managed containment and the available operator recovery policy. Avoid claiming cleanup of all descendants from `ProcessRegistry::stop` alone.

A product specification may accept independently verifiable work that **does not depend** on the shared decision. The relevant product gate must link #75 only for semantic changes that otherwise risk incompatible identity/readiness/exposure contracts. Avoid turning this research issue into a blanket dependency for all CLI, Manager or GUI work.

## 7. Tested independent-Cargo inspection boundary (2026-11-21)

**Research-only evidence:** [PR #79](https://github.com/aschenrot/riesel/pull/79), executable test fixture [`research/operational-inspection`](../../../research/operational-inspection/README.md), and the [Linux validation/smoke run 44857666582](https://github.com/aschenrot/riesel/actions/runs/44857666582) on code head `2986ef46`. The CI log shows Cargo compiled the separate Rust application in an external `/tmp` directory; the test ran against the local SDK and did **not** inherit the Riesel repository's Cargo workspace or aliases. The fixture deliberately uses temporary, disposable Cargo resolution; it is not a model for distributed application dependency pinning.

| Tested operation | Application setup marker | Handler marker | Result/interpretation |
| --- | --- | --- | --- |
| Execute `--print-definition` | Present | Absent | Valid SDK-derived JSON, but executing the *binary* ran user setup before Riesel's pure preflight |
| Execute `--help` | Present | Absent | App help also runs setup, not the handler |
| Execute unsupported infrastructure option | Present | Absent | Strict CLI failure happens **after** executable setup, before the handler |
| Ordinary application run | Present | Present | Both setup and handler run normally |
| Read cached JSON metadata from a file | Absent | Absent | Passive file inspection; no proof of artifact provenance, freshness or runtime readiness |

**Finding:** an operator/developer tool must not describe arbitrary `binary --print-definition` inspection as passive or side-effect-free. Its **execution authority** is a distinct concern from the SDK's internal *preflight purity*. A future design may use (A) deliberately authorized process execution, (B) validated cached/generated metadata, or (C) an authorized live app inspection interface. Compare the three against the same product requirements; this experiment **does not choose one**.

**Remaining untested contracts:** attested metadata/version binding; provider versus app/public-interface readiness; externally represented session generation; public facade authorization, disconnect and stale in-flight mutations; Manager/OS process-tree lifecycle ownership. No new Riesel runtime protocol or SDK interface was created by this evidence.

## 8. Tested two-process typed facade feasibility and limits (2026-11-21)

**Research-only code:** [PR #81](https://github.com/aschenrot/riesel/pull/81); the [application-process host](../../../examples/counter/app/tests/fixtures/counter_public_facade_host.rs), [independent typed client](../../../examples/counter/app/tests/fixtures/counter_public_facade_client.rs), and [bounded smoke harness](../../../tools/xtask/src/smoke/facade.rs). Executable head `1bb17a6` passed [Linux validation and smoke run 44862084580](https://github.com/aschenrot/riesel/actions/runs/44862084580); the log includes `Two-process application facade research passed`. This is not an accepted application-export API, security protocol or native GUI.

The Rust application handler explicitly creates an `AsyncMemoryCounter` implementation and hosts **only its generated `AsyncCounter` dispatcher** using the existing asynchronous TCP RPC server. It is app-*process*-owned state, **not** an export of an arbitrary capability handle resolved through the application's `Context` or provider catalog. The separate headless client uses the published `AsyncCounterContract::bind_async` and an explicitly supplied ephemeral loopback address; it does not reuse application runtime handles.

| Observable | Tested outcome | Boundary / remaining concern |
| --- | --- | --- |
| Typed cross-process binding | Generated `AsyncCounter` contract verified; query and commands executed in a separate OS process | Reuses typed contract metadata but not an application-level interface registration |
| Deliberate exposure | `AsyncCounterObserver` description rejected as `WrongCapability`; only the selected dispatcher was served | **Route selection is not caller authorization**; the same counter implementation also supports Observer internally |
| Backend survives UI/client exit | First client increments to `3` then exits; second client reads `3` and increments to `5` without restarting the app | Does not yet test backend crash, process-wide recovery, multiple concurrent UI clients or long-lived subscriptions |
| Readiness | Host prints `RESEARCH_FACADE_BOUND` after listener bind; client then verifies contract and successfully sends operations | Printed bind notice alone is **not** proof of authenticated interface readiness |
| Graceful lifecycle | Research harness sends a test-only stdin STOP; handler requests host-loop stop and joins it | All this control wiring is application-specific, not an SDK-managed export or process-tree guarantee |
| Trust | Credential-**less** local client can execute state-changing `add` | **No caller authentication**, permission checks, session-generation fencing or published public-interface security policy |

### Equitable architecture comparison

| Alternative | Demonstrated reuse | Repeated wiring or future cost | Open decision |
| --- | --- | --- | --- |
| **A. Generated facade hosted explicitly by the application** | This test confirms typed serialization/dispatch, explicit route selection, separate process and persistent app-process state | Manual listener, endpoint/address passing, readiness, frontend trust, application-session ownership and reconnect; naïve loopback hosting is insecure for privileged operations | Determine whether a facade can be backed by an existing **application-resolved** capability without breaking ownership/recovery |
| **B. Application-specific HTTP adapter (Todo)** | [Todo HTTP](../todo/http-local-browser-contract.md) already proves Rust-owned domain state, browser-independent app, same-origin checks and explicit local startup | Application-specific route/JSON handling; browser-origin defense is not local privileged-client authentication; typed Rust contract reuse is not automatic | Measure the repeated cost when a second nonbrowser client/application uses the same operations |
| **C. Reusable application export contract** | Not yet implemented or measured; could reuse existing generated dispatch and canonical SDK definitions | New public SDK semantics, explicit authorization and session/readiness/version contracts; risk of duplicating provider resolution or adding premature configuration authority | Compare concrete minimal design/authoring prototype against A and B **before** Gate B, without assuming a daemon or new crate |

**Critical finding:** proving A technically possible is **not** evidence that A is the best platform architecture. It also cannot be treated as a secure public API: successful mutation from a credential-less process demonstrates the current trust gap. Equally, proposed C cannot be preferred simply because it promises more reuse; its lifecycle and authorization cost must be demonstrated.

**Next decision-critical experiments:** bind an app-*resolved* capability into an explicitly exported facade, test client identity/authorization (including a same-host untrusted process), establish a server-issued application-session generation and stale-binding behavior across restart/reconnect, distinguish ambiguous in-flight mutations from safe retries, and compare a second interface frontend without rewriting business logic. A real native window loop or installed Manager is not required to study these application-facing semantics.

## 9. Research comparison: SDK-resolved facade forwarding (2026-11-21)

**Tested executable evidence:** [PR #82](https://github.com/aschenrot/riesel/pull/82) on code head `2219599e` passed [Linux validation/smoke run 44863291624](https://github.com/aschenrot/riesel/actions/runs/44863291624), including `adapter_keeps_session_failure_distinct_from_domain_error` and `Two-process application facade research passed`. The final documentation-containing PR head requires its own validation before merge. This research extends the [two-process generated facade fixture](../../../examples/counter/app/tests/fixtures/counter_public_facade_host.rs) with a [second application host](../../../examples/counter/app/tests/fixtures/counter_resolved_facade_host.rs) that receives `AsyncCapability<AsyncCounterContract>` from `Application::async_handler` and `component::<AsyncMemoryCounter>()`. This capability **is resolved by Riesel's actual application provider registry and bound to the current runtime session**, rather than constructed as an independent component instance in the handler. The same external typed [client](../../../examples/counter/app/tests/fixtures/counter_public_facade_client.rs) is used for both host modes through the [shared smoke harness](../../../tools/xtask/src/smoke/facade.rs).

**API boundary:** generated `AsyncCounterHandle` / `AsyncCapability` exposes operations returning `CallError<CounterError>`; `AsyncCounterAsyncRpcDispatcher<T>` requires an implementation of the provider-side `AsyncCounter` trait, which returns `CounterError` and cannot directly represent session or RPC failure. A resolved client handle is **not** a provider implementation. Thus the experiment's `ResolvedFacade` manually implements `AsyncRpcHandler`, repeats operation decoding and forwarding, and deliberately maps `CallError::Domain(error)` to serialized `Result::Err(error)` while retaining `CallError::Rpc(error)` at the transport/RPC error layer. A pure adapter unit test checks that `RpcError::SessionExpired` is not serialized as a domain error; it does **not** prove externally fenced session epochs or real restart behavior.

**Comparable expected observations:** typed contract verification; query, `add` and `label` across independent clients; invalid extra payload field rejected; zero-increment domain error preserved; non-exported Observer capability refused; state continuity between client processes while the backend remains alive; explicit bounded research-only shutdown. The exact code-head CI verified these assertions using both host modes and the same external client. No caller authentication or server-issued session epoch was added.

| Hosting approach | Reused infrastructure | Added work / shortcomings | Long-term implication |
| --- | --- | --- | --- |
| **A1. App-process-created component + generated dispatcher** | Existing versioned contract, typed transport, generated method dispatch | Creates separate component state inside the handler rather than exporting an already resolved application capability | Good example of dispatch reuse; insufficient as a general app-resolved interface architecture |
| **A2. Riesel-resolved session handle + manual RPC facade** | Existing SDK resolver, session-aware capability handle, generated typed remote caller | Repeats method decoding/forwarding and careful domain vs RPC error mapping; must own listener, discovery, lifecycle and authorization itself | Direct measure of duplication that a reusable export mechanism might remove; no public abstraction accepted |
| **B. App-specific HTTP adapter (Todo)** | Application-owned domain operations and browser-origin defenses | App-written routes, authorization policy and transport models; typed public capability mapping not automatic | Viable frontend integration, not proof every app should repeat custom endpoint work |
| **C. Reusable application export contract** | Would aim to reuse existing SDK definition, typed contracts and runtime session | No proven implementation yet; needs explicit opt-in public surface, credentials/authorization, session generation and ownership semantics | Worth strategic upfront investment if it avoids duplicated adapters without creating a second resolver or control plane |

**Security and validity limit for both A fixtures:** loopback is not authentication, capability contract metadata does not establish trusted server identity, and the fixture accepts state mutations from credential-less clients. The generated bindings do **not** automatically advertise application/interface readiness, reject stale client sessions after a process restart, or guarantee uncertain in-flight mutations can be replayed safely. A GUI event loop, installed app owner and Manager remain outside these tests.

**Decision remains open.** Compare A1, A2, B and C against the accepted platform outcome and foreseeable cross-application reuse. Neither a successful manual adapter nor a sophisticated speculative exporter is sufficient by itself to decide Gate B.

## 10. Gate B recommendation and exclusions

**Recommend a comparative Gate B**, evaluating B as an important reusable-foundation candidate alongside A and C under the same product-wide outcomes, integration tests and growth costs. **Do not accept any mechanism** before its concrete identity, readiness, trust and lifecycle semantics are compared. Product-owner acceptance is required for any shared public app/operational contract, implementation Issue, crate boundary, daemon or protocol.

The research issue [#75](https://github.com/aschenrot/riesel/issues/75) owns the checklist and final accept/revise/defer/reject decision. [#68](https://github.com/aschenrot/riesel/issues/68), [#59](https://github.com/aschenrot/riesel/issues/59) and [#60](https://github.com/aschenrot/riesel/issues/60) retain independent product outcome decisions. The already accepted Storage and Todo HTTP contracts are unchanged. No roadmap stage is created merely by adding this investigation.


## 11. Coordinated decision packet — 2026-11-21 (main `0798a6fb`, proposal)

This section **updates the research recommendation**, not the accepted application contract. It incorporates the independent Cargo executable-inspection test [#79](https://github.com/aschenrot/riesel/pull/79), the generated facade comparator [#81](https://github.com/aschenrot/riesel/pull/81) and SDK-resolved handle forwarding [#82](https://github.com/aschenrot/riesel/pull/82). The latter two prove cross-process typed calls and retained app-owned state. They do **not** prove externally fenced sessions, authorization, readiness or a native event loop. Older statements in this investigation that a resolved application facade has not been exercised are superseded by section 9 and this packet.

### 11.1 Proposed shared meanings, not shared control ownership

| Concept | Authoritative issuer/owner | Stable scope and invalidation | Consumer |
| --- | --- | --- | --- |
| Application definition identity | Application author/publisher; explicit external identity must be distinguished from a display name and checked against versioned derived SDK definition | Stable across ordinary launches; any publication/version policy requires acceptance | CLI, Manager, GUI |
| Manager registration identity | Trusted operator/Manager registry | Stable within the registry, **not** a substitute for publisher identity or process ownership | Manager/UI |
| Artifact/build identity | Builder and verified installed-artifact source | A particular executable/build, not automatically the definition identity; do not trust a self-reported digest as remote proof | CLI/Manager |
| Launch attempt ID | The **actual process owner**: foreground tool, Manager or OS unit controller | New for every OS start attempt; PID is diagnostic only; one authority at a time | Tool/Manager |
| Application runtime generation | Current Riesel application session, mapped to an externally represented unpredictable generation only by an explicit app-owned interface | New after every whole-session recovery, including within an unchanged OS process; old generation cannot authorize new operations | GUI, authorized observers |
| Provider instance identity | Existing application provider runtime | Provider-scoped, may change independently of launch attempt; not a user-facing application ID | Application/runtime and observations |
| Public interface identity | Explicit publishing application, referencing versioned capability contract(s) and a particular session | A separately published surface, not all internal provider requirements; unavailable after session revocation | GUI/other authorized clients |
| Client attachment identity | Trusted connection/authorization layer | One attachment, revocable and not interchangeable with application session or registration | Export endpoint |
| Management command ID | Authoritative Manager command coordinator | A recorded request/result **within the coordinator's advertised retention epoch**; does not survive an unjournaled Manager crash by magic | CLI/UI |

A descriptive name, TCP port, PID, `ProcessKey`, in-process `SessionToken` or arbitrary provider metadata **must not** act as the public authentication credential, stable application identity, or cross-process session generation. No new Rust type names, canonical JSON schema, serialization or storage location are accepted here.

### 11.2 Observation is a set of independently evidenced facts

Report **definition known**, **plan validated**, **OS launch accepted/spawned**, **process observed alive**, **providers ready**, **application handler active**, **specific public interface bound**, **specific interface verified callable**, and **terminal/failed/unknown** independently; do not collapse these into one boolean `ready`. Every observation should carry an authority/source and observed generation; stale observations must not masquerade as current state. A successful bind/readiness line is not a complete end-to-end authenticated call. A stopped application cannot self-report fresh live readiness; missing telemetry is `unknown`, not `healthy`.

- **Passive inspection:** read already generated/cached metadata with its provenance and freshness caveat; no untrusted application execution.
- **Executable inspection:** deliberately run a **trusted** binary; #79 demonstrates user Rust setup runs before `--print-definition`. Label and authorize this as code execution, not passive inspection.
- **Live inspection:** authorize against a specific app/interface session and report disconnect/timeout without fabricating negative or healthy state.
- **Failure envelope:** preserve origin (build/tool, process owner, application startup, provider, interface, client transport), actionable details and whether the command's effect is known. Avoid exposing credentials in error/log output.

### 11.3 Contract candidate B, bounded and transport-neutral

**Recommendation for maintainer decision:** prefer a **small shared semantic contract** for identity, readiness/observations, deliberate public capability exposure, generation fencing and common error meaning, implemented by product-specific adapters. It is **not** a daemon, service manager, protocol, manifest or replacement provider resolver. Compare it in an executable design slice against (A) adapters with independent conventions and (C) a universal control service before acceptance.

A proposed interface attachment handshake verifies all of: (1) expected application and artifact/provenance where applicable, (2) explicitly exported contract identity/version, (3) active session generation, and (4) the separately authenticated/authorized client principal and permitted operations. **Version agreement and generation matching are not authorization.** The export belongs to the session; shutdown and whole-session restart revoke the previous export before any successor is treated as ready. On reconnect, obtain a fresh authorized binding, re-read authoritative state, and never replay uncertain mutations. A lost reply is *outcome unknown* unless a separate durable operation-status/idempotency mechanism is accepted and verified. Read-only requests may be retried only under an explicit operation-level policy; do not infer safety from HTTP or RPC method naming.

Under a constrained first **single-trusted-user** local threat model, same-UID processes may still be hostile. Unix peer UID, private socket directory, loopback binding and browser Host/Origin checks have different roles and do not together prove caller authorization. Either explicitly exclude adversarial same-UID code, or design a stronger principal/credential isolation policy and test it before exposing privileged commands. Remote and cross-user support require separate authorization evidence.

### 11.4 Cross-product ownership and actual prerequisite edges

| Owner | May do | Must not silently do |
| --- | --- | --- |
| Cargo/developer CLI #68 | Select/build project, own foreground launch attempt, inspect trusted artifacts, optionally open UI | Become provider resolver, app session restart controller, Manager or privileged global registry |
| Riesel runtime | Validate provider selection, own providers and session-specific tasks, revoke session handles, own public app export lifecycle **if adopted** | Own OS deployment/installed service policy, accept external control on provider metadata alone |
| Manager #59 | Own registry, authorize operations, reconcile its **actual process-owner mechanism**, expose operator status | Resolve application's internal providers, adopt an unknown PID or claim transitive process-tree containment without evidence |
| Native GUI #60 | Own OS window, renderer, UI event loop and presentation state; attach by deliberate app-facing contract | Own backend domain/provider state or terminate app on window close |
| OS owner | Own process/service/scope/job according to explicit deployment policy | Be reported as a working application interface solely because a unit is active |
| Execution research #83 | Investigate plan/trace/tick/step semantics within domain ownership | Imply a shared scheduler, global task control or node grammar in this operational contract |

**True ordering:** a generic CLI may research Cargo workflow independently of Manager/UI; native event-loop/renderer tests need no Manager; Manager UI design may proceed independently of native hosts. Any real **cross-process control, GUI attachment, or unified live inspection** must first agree compatible app/interface identity, readiness, authorization and generation semantics. #83's local scheduler/stepping study is independent; only authorized remote observation shares this boundary.

**Verified lifecycle mismatch requiring a common meaning, not necessarily a common signal:** the managed provider host accepts advertised `RIESEL_SHUTDOWN` on stdin, while the Todo application HTTP host reacts to Ctrl-C. A foreground tool or Manager must not infer an application-level graceful-stop contract from a provider handshake. The owning launcher/OS service mechanism and the app runtime need an explicitly tested stop-to-session-drain mapping, including platform signals and escalation; the native GUI must not silently trigger app stop on window close.

### 11.5 Non-negotiable Gate B evidence before implementing a common public interface

1. A real SDK-resolved app-facing capability across two processes **after session restart**, proving fresh generation, old descriptor rejection and no call to a revoked session; inject a lost mutation response and verify no automatic replay.
2. Authorized and unauthorized local clients: wrong app/interface/version, different Unix UID where testable, an explicitly excluded or addressed same-UID attacker, malformed and over-budget requests; no mutation before verified authorization.
3. App interface readiness measured *after* listener bind and authenticated typed operation; prove process spawn and provider handshake alone cannot set it.
4. Compare author code duplication in Todo and **another independently authored app** for (A) generated facade, (B) manual HTTP/adapter and (C) bounded reusable exporter, including listener lifecycle, error translation, reconnect and security.
5. Manager process-owner/restart scenario with real OS-group/service mechanism; distinguish top-level process exit from contained descendant cleanup.
6. Record transport/version compatibility and metadata provenance choices; fail closed on unknown versions rather than silently accepting a different contract.

Existing #79/#81/#82 evidence establishes only the indicated subsets. The new [POSIX process probe](../../../research/process-ownership/process_tree_probe.py) is a research comparator, **not** Riesel or systemd validation. Keep #75 OPEN and request maintainer **ACCEPT / REVISE / DEFER / REJECT** for the *shared semantic invariant*, separately from any particular exporter/daemon or implementation milestone.
