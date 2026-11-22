# Riesel operational products — architecture direction and specification gates

**Decision scope (2026-11-21):** The product owner selected **accept** for the *directions* in [#75](https://github.com/aschenrot/riesel/issues/75), [#68](https://github.com/aschenrot/riesel/issues/68), [#59](https://github.com/aschenrot/riesel/issues/59) and [#60](https://github.com/aschenrot/riesel/issues/60). This document records the bounded interpretation for review. It does **not** accept a particular API, transport, toolkit, process-owner guarantee, threat model, implementation stage or production change. The four specification issues remain open for their independent technical gates. This record becomes authoritative only through a reviewed/merged PR as required by [delivery.md](../../project/delivery.md).

## Accepted product directions, not implementation mechanisms

| Owner | Target outcome and boundary | Not selected by this decision |
| --- | --- | --- |
| [#75 shared operations](application-operational-boundary-investigation.md) | One consistent vocabulary for application/interface identity, readiness, published access, session generations, inspection and failures across tools, Manager and GUI. Separate authority for actual execution. | Mandatory daemon, process registry, concrete protocol, authentication implementation or new crate. |
| [#68 developer workflow](developer-startup-investigation.md) | An optional cross-project Cargo/Rust workflow to build, inspect, launch and stop independently authored applications; support headless operation and opt-in interfaces without copied repository tooling. | Final command spelling, manifest, SDK hook, installer, automatic frontend build, Manager dependency or universal cleanup guarantee. |
| [#59 Manager](manager-control-plane-investigation.md) | A single-host operator product for explicit trusted registrations, status/diagnostics and authorized start/stop under one CLI/UI control contract. **Hybrid ownership is the target candidate:** one chosen authoritative process owner per application, with different OS/development adapters as justified. | First process-owner adapter, Manager-crash semantics, service installation, browser write authority, process-tree containment or automatic process adoption. |
| [#60 native GUI](native-window-host-investigation.md) | Independent native GUI process, headless app remains authoritative; UI attaches to **deliberately published**, versioned app capabilities, not internal provider state. A toolkit-neutral, application-owned export boundary is the target to evaluate. | Final exporter API, IPC/auth protocol, GUI toolkit, generic widget language, broker, subscriptions or UI framework. |

These directions are deliberately compatible, not automatically coupled. An independent headless CLI needs neither Manager nor GUI. A native GUI must work without Manager. A Manager UI may be a browser without using a native host. The runtime continues to own provider selection, session cancellation and recovery under its [existing contract](runtime-lifecycle.md); neither the CLI nor Manager becomes a second provider resolver.

## Shared invariants proposed for specification acceptance

These are the **invariants to validate and finalize** through #75, not yet a frozen public interface:

1. **Distinct identities and owners:** an authored application definition, a Manager's registration, a built artifact, an OS launch attempt, a Riesel session generation, a provider instance, an explicitly published interface, a client attachment and a Manager command are different concepts. Display names, PIDs, endpoint addresses and in-process session tokens are not security credentials or interchangeable IDs.
2. **Evidence-based observation:** distinguish definition known, plan validated, OS process spawned/alive, providers ready, handler active, interface bound, interface verified callable, terminal failure and unknown. State which authority observed each fact, when, and for which generation. A PID, provider readiness line or socket bind does not prove an authenticated application interface is callable.
3. **Safe inspection:** cached, provenance-qualified metadata inspection does not execute the application; asking an executable for its definition *does* run trusted code before inspection, as [#79](https://github.com/aschenrot/riesel/pull/79) verified. Do not silently treat the latter as passive.
4. **Deliberate external exposure:** an app exports a selected versioned contract only by opt-in and only for a valid application session. Contract version or generation matching does not authorize the caller. Do not publish all providers, infer trust from loopback or reuse provider-handshake metadata as an application access token.
5. **Recovery and command uncertainty:** whole-session recovery invalidates earlier exported attachments, including when the process PID is unchanged. A missing mutation reply has **unknown outcome**, not implied failure; reconnect reauthenticates and refetches authoritative state rather than automatically replaying commands.
6. **One actual process owner:** foreground CLI, Manager child mechanism or OS service/job owns a particular launch according to an explicit policy. Manager must not claim arbitrary descendant cleanup or adopt unknown PIDs; UI close is never implicit backend stop.
7. **Extensibility without prebuilding features:** transport-neutral capability and observation semantics may later support other GUI toolkits, subscriptions, services or game frontends. This does not authorize a message broker, universal renderer or distributed supervisor.

## First scope and independently verifiable gates

| Gate | Execution evidence required before freezing the relevant mechanism | Spec owner |
| --- | --- | --- |
| S1 — interface identity and trust | In two separate processes, export an **SDK-resolved** app capability. Reject wrong app/contract/version, unauthorized local clients and stale session generation **before** side effects; define or explicitly exclude hostile same-UID processes. Prove restart changes the authoritative generation. | #75, coordinated with #60 |
| S2 — readiness and uncertain mutations | Test provider-ready versus actually callable interface; drop a mutation response, verify no silent replay and fresh authenticated snapshot after reconnect. Establish bounded frames/errors and source/freshness of observations. | #75 and #60 |
| T1 — cross-project tooling | Independently authored Cargo workspace launches/inspects/stops without copied xtask, mandatory Manager/GUI or new schema; separately test trusted executable-inspection side effects, error reporting and correct owner cleanup. | #68 |
| T2 — persistent Todo | Stable explicit database root, dynamic port, real static browser assets, shutdown/start with persisted task; no-open, missing deps, duplicate session and failing readiness are explicit. | #68 |
| M1 — process owner | Compare direct child with a real OS-owned mechanism: Manager crash/restart, process/descendant exit, owner reconciliation, unauthorized or stale stop and shutdown drain; no blanket guarantee from the preliminary [POSIX probe](../../../research/process-ownership/process_tree_probe.py). | #59 |
| M2 — local operator authorization | Trusted registration/path policy, protected state-changing commands, ambiguous-command status and CLI/browser parity; if browser write access is unproven, label UI read-only instead of claiming full control. | #59 |
| G1 — reusable export comparison | Measure manual generated facade, Todo-style HTTP, SDK-resolved manual forwarding and a **bounded reusable exporter prototype** against two app shapes, including repeated mapping/error/security/lifecycle code. [#81](https://github.com/aschenrot/riesel/pull/81)/[#82](https://github.com/aschenrot/riesel/pull/82) prove typed communication, not exporter completeness. | #60 and #75 |
| G2 — native integration | One real separate-process window loop with snapshot/action/refetch, responsive input under slow I/O, GUI quit/reopen without app stop, restart/reconnect and accessibility/focus tests. | #60 |

For all relevant slices retain the repository's Linux validation/smoke checks on the exact revision and the manual Windows pre-release policy. Research spikes are not production interfaces.

## Technical evidence reconciliation (review annex — not accepted)

The subsequent exact-head #68/#59/#60/#75 research and its security, recovery, readiness and implementation sequencing are synthesized in the [technical decision review](operational-technical-decision-review.md). That annex proposes the common semantics and independently deliverable slices, **but does not accept a public contract, trust profile, implementation mechanism or milestone**. Each open specification Issue remains the owner of its eventual technical decision. Do not merge or infer acceptance from research-only CI. The earlier SQLite ownership regression (#102) was resolved and closed after tested merged-main corrections; validate each subsequent candidate against its own exact-head checks instead of carrying forward that historical CI failure.

## Decisions deliberately left to the respective specification issues

- **#75:** concrete identity representation and compatible versions; authorized principal/credential model, local same-UID threat assumption and interface-publication trust; whether one bounded shared SDK contract reduces repetition relative to product-specific conventions. Evaluate against [the comparative research](application-operational-boundary-investigation.md).
- **#68:** CLI packaging versus SDK metadata/participation shape; durable default data location; process shutdown signal/control and escalation; automatic frontend build rules. Respect the current mismatch between Todo's Ctrl-C shutdown and managed-provider stdin shutdown.
- **#59:** first supported process owner (direct-child or verifiable OS service unit), fate of apps on Manager death, reconciliation/command retention and secured browser state changes. The hybrid target is **not** evidence that all ownership modes work.
- **#60:** exported-capability implementation after comparative prototype, first UX/platform/toolkit, IPC choice and subscription/snapshot boundary. Keep framework, broker and Manager dependencies out of the first GUI integration.

## Delivery order and status

1. **Now:** review and record these accepted *product directions* in the [product definition](riesel.md) and linked issues. Leave specification issues **open** while material mechanism/guarantee decisions remain.
2. **Parallel research:** S1/S2 lead cross-process shared semantics; T1/T2, M1 and native event-loop feasibility can proceed independently. G1 aligns with S1/S2. Do not call all of #75 a blanket dependency for independent research.
3. **After evidence:** accept/revise/defer each full technical specification, update authoritative contracts and close the corresponding spec issue with recorded rationale. Only then create independently testable implementation issues, product stages and optional milestones. No production API, UI, daemon or roadmap commitment is approved by this decision record.

**Evidence baseline:** the completed cross-product research [PR #85](https://github.com/aschenrot/riesel/pull/85) was merged at [a35ac7c9](https://github.com/aschenrot/riesel/commit/a35ac7c9ac9749ff023a72595c6851caee5f4859), with [exact-main Linux validation and smoke](https://github.com/aschenrot/riesel/actions/runs/44873293038) passing. Its architecture candidates and research probes are not themselves implementation acceptance.
