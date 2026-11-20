# Shared application operational boundary — cross-product investigation

**Status:** research coordination only. No accepted new SDK contract, crate, CLI, daemon, protocol, Manager, exporter or implementation authorization.  
**Specification:** [#75](https://github.com/aschenrot/riesel/issues/75) · **Reviewed:** 2026-11-20 · **Repository starting point:** `main` at `7456dbce`.  
This report compares common semantics relevant to [developer tooling #68](https://github.com/aschenrot/riesel/issues/68), [Manager #59](https://github.com/aschenrot/riesel/issues/59), and [native GUI #60](https://github.com/aschenrot/riesel/issues/60). **Each product retains its own Gate B decision.** The [delivery workflow](../../project/delivery.md) prefers architecture-first investment when it improves credible cross-product reuse; it does not preselect shared infrastructure.

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
| **B. Shared, transport-neutral operational semantics, with product-specific adapters** | One explicit identity/readiness/inspection/exposure model can serve CLI, Manager and GUI without a mandatory daemon; allows standalone developer work | Public semantic versioning and API changes must be justified; requires two or more integration fixtures | **Preferred investigation direction**, not accepted architecture |
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
3. **Export:** build a real two-process typed facade client and compare with an app-owned HTTP adapter plus an explicitly public SDK exporter; verify unauthorized provider access is rejected and GUI-only shutdown leaves backend running.
4. **Generation/unknown outcome:** crash/restart the app and disconnect/reconnect the client around an in-flight revisioned mutation; reject stale bindings and do not auto-replay.
5. **Manager supervision:** compare direct owned-child stop, OS-managed containment and the available operator recovery policy. Avoid claiming cleanup of all descendants from `ProcessRegistry::stop` alone.

A product specification may accept independently verifiable work that **does not depend** on the shared decision. The relevant product gate must link #75 only for semantic changes that otherwise risk incompatible identity/readiness/exposure contracts. Avoid turning this research issue into a blanket dependency for all CLI, Manager or GUI work.

## 7. Gate B recommendation and exclusions

**Recommend investigating B first as a design hypothesis**, and retaining A and C as real alternatives. **Do not accept B** until its concrete semantics and integration cost have been compared. Product-owner acceptance is required for any shared public app/operational contract, implementation Issue, crate boundary, daemon or protocol.

The research issue [#75](https://github.com/aschenrot/riesel/issues/75) owns the checklist and final accept/revise/defer/reject decision. [#68](https://github.com/aschenrot/riesel/issues/68), [#59](https://github.com/aschenrot/riesel/issues/59) and [#60](https://github.com/aschenrot/riesel/issues/60) retain independent product outcome decisions. The already accepted Storage and Todo HTTP contracts are unchanged. No roadmap stage is created merely by adding this investigation.
