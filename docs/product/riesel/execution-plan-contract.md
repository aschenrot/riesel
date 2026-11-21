# Riesel — Execution Plans, Scheduling and Inspection

**Status:** Accepted product outcomes and architectural invariants (2026-11-21); **headless plan/binding foundation delivered; runner and inspector not yet delivered**. The owner accepted the bounded direction in [specification #83](https://github.com/aschenrot/riesel/issues/83#issuecomment-7063096674), based on [research PR #84](https://github.com/aschenrot/riesel/pull/84) and its [35 passing research tests](https://github.com/aschenrot/riesel/actions/runs/44887680499). Research-only structs, JSON documents, schedulers and examples are **not** approved public APIs or interoperability formats. This contract states required outcomes, boundaries and testable invariants, not a frozen mechanism.

## Product outcome

A Rust application developer can **describe and validate executable operation relationships, inspect a plan without running operations, execute it headlessly, observe authoritative progress, and cooperate with supported pause/advance/resume safe points**. The execution model supports both event-driven/async applications and fixed-tick simulation boundaries without requiring an ECS, window, daemon or particular GUI toolkit. Optional graphical inspection presents the *same* plan and execution state. Editing executable graphs is a **long-term direction** for a deliberately bounded set of registered operations; its supported edits and source-of-truth rules require a separate specification before implementation.

A headless-only application remains a complete supported application, not an incomplete graphical project.

## Responsibility and authority

| Layer | Responsibility and authoritative owner | Prohibited conflation |
| --- | --- | --- |
| Application definition and capability requirements | Existing SDK/`AppSchema` and the application author | Not an executable operation graph or a process installation |
| Provider catalog and deployment | Existing provider resolver/preflight and chosen provider instances | Not a general scheduler; an operation may consume an already-resolved capability but must not select providers again |
| Application session, tasks and lifecycle | Existing runtime and its owned task/provider/process scopes | Not replaced by an execution-plan controller or second supervisor |
| **Execution plan** | Application-authored, validated description of logical operations, relationships, triggers and applicable constraints | Not executable arbitrary Rust source, a provider dependency DAG, or an untrusted app's verified identity |
| **General graph runner** | A reusable Riesel-owned coordinator for eligible **Riesel-owned** operations, under the existing session | Not universal ownership of all nested domain scheduling, OS processes, native threads or external side effects |
| Executor/operation adapter | Invokes supported registered Rust operations, typed capability handles or explicitly delegated domain work | Does not expose every internal capability to another process |
| Domain coordinator | Authoritative owner of the domain's **internal** tick/frame/system schedules and resources when that boundary is delegated | Riesel cannot separately admit the same inner tick/system while the domain also owns it |
| Observer/controller | Reads authoritative plan/state/snapshot and requests supported cooperative controls through the owning boundary | A lossy trace is not the state store; a UI, CLI or remote client is not an independent scheduler |

**One admission/control authority per boundary.** Riesel may admit an outer operation (which in turn invokes a domain-owned frame) or, when explicitly supported, admit distinct outer tick operations. A domain's internals remain under its declared owner. Every presented boundary must identify whether it supports pause, step, resume or only observation; unsupported system-level/native preemption is reported, not simulated. `RunenECS` is an independent possible domain integration, not a required dependency of Riesel.

## Plan construction, inspection and identity

- Validate operation IDs, dependencies, cycles, eligibility rules, declared triggers, supported access constraints and registered executable bindings **before** admitting work. Rejected plans must not run a partial prefix just because some operations are valid.
- Preserve a stable logical plan identity and an **immutable semantic revision** for each executing run. An execution observes the exact accepted plan; a new edit cannot silently alter a running revision. Application-authored executable bindings use supported Rust/capability registration; publishing a manifest alone is not permission to execute arbitrary code.
- An execution address contains at least the owning application/session generation, plan identity/revision, run identity and operation identity/attempt. A session restart or republished plan cannot be mistaken for the old run. Exact persistence, global allocation, signatures, hashes and serialization formats are **not yet selected**.
- Machine-readable plan inspection and current-state snapshots must be available to headless clients. Static inspection must not execute plan operations or create provider instances; obtaining metadata by **launching untrusted Rust code** is not passive or guaranteed safe.
- Keep presentation-only graph layout/editor state distinguishable from semantic plan changes. The eventual edit/round-trip rules must explicitly choose the source of truth for code-authored and editor-authored plans.
- Declare supported execution classes (one-shot, event-driven, fixed-tick, and any later admitted policies) with explicit clock/admission semantics. Do not claim generic determinism, real-time deadlines, fully typed data transfer or automatic hot updates merely because a DAG is valid.

## Authoritative state, results and control

- Runtime state must distinguish **pending/eligible/running** activity from terminal completion, cancelled work, known failure and an **outcome-unknown** state when a command may have executed after dispatch but the reply was lost. A specific API may use a richer result type instead of adding one status enum variant; the semantic distinction is mandatory.
- Track **confirmed committed effects separately from terminal classification**, including partial success before cooperative cancellation or failure. A cancellation request cannot roll back a committed tick, remote mutation or prior side effect. A disconnected or timed-out observer must not report a missing response as proof of nonexecution.
- Dependent success-only operations cannot advance on unknown/cancelled/failed prerequisites as though they completed. A supported failure policy must define blocked/skipped/terminal results and what authoritative reconciliation is available. No automatic retry of uncertain mutations or exactly-once execution guarantee follows from this architecture.
- Controls are **cooperative**, fenced to the current execution/session and dispatched to the single declared owner at the requested operation/domain safe point. Boundaries that cannot be safely paused/stepped fail explicitly; Riesel will not suspend arbitrary synchronous Rust, GPU operations or OS threads.
- Trace/event delivery may be bounded and lossy. Loss/overflow must be detectable and consumers must be able to **resynchronize from authoritative state**. The trace stream must correlate events to their plan/run/node/attempt without inferring revisions from an observer's local clock or untrusted strings.
- Resource and access declarations guide eligibility but do not prove Rust alias/borrow safety for opaque callbacks. Native thread-affine or `!Send` state stays with its owning executor/domain unless separately proved safe; a shared graph format does not imply automatic cross-thread or cross-process portability.

## Security and lifecycle integration

The scheduler operates **within** the existing runtime session and honors provider teardown, task cancellation and whole-session recovery. A new session invalidates old controls and handles. The runner cannot become another provider resolver, application launcher or Manager. Independent process/GUI control needs authorization, version and session semantics coordinated with [shared operational research #75](https://github.com/aschenrot/riesel/issues/75), [Manager #59](https://github.com/aschenrot/riesel/issues/59), [native GUI #60](https://github.com/aschenrot/riesel/issues/60), and [developer tooling #68](https://github.com/aschenrot/riesel/issues/68); none is a blanket prerequisite for local/headless execution tests.

A generation number, TCP loopback address, matching local OS user or provider readiness announcement is **not sufficient authentication** for privileged operations. The accepted direction does not automatically approve external control endpoints, credentials, deployment manifests, distributed execution, persistent execution history, replay or a general broker. Refer to the existing [runtime lifecycle](runtime-lifecycle.md) and [async execution](async-execution-contract.md) contracts for current shutdown and uncertain-RPC limits.

## Optional interfaces and visual authoring

**Graphical inspection is accepted as an optional product outcome**, consuming the same versioned plan, authoritative snapshots and loss-aware trace as an SDK/CLI client. It may use an independent interface process, but no toolkit, frontend transport or Manager UI has been selected. It must never be the sole place execution correctness is implemented.

**Editable executable graph composition is accepted as a long-term direction**, initially limited to a future **specified subset** of registered, typed/bound operations and supported graph edits. Before implementation, separately decide the code-versus-document source of truth, supported edits and invalid states, version/layout boundaries, operation/port typing, serialization migration, executable binding/authorization and round-trip invariants. Arbitrary Rust closure serialization, arbitrary resource transfer, automatically generated ECS system schedules and unrestricted visual programming are **not accepted guarantees**. A visual inspection graph does not itself satisfy the edit/execute outcome.

## Implementation and acceptance sequencing

Implementation issues must be separately testable and grounded in this accepted direction; they may proceed in parallel only where an agreed interface makes them independent.

1. **Plan and binding foundation:** inspectable immutable plan identity/revision, strict validation and explicit executable registration; headless operation without starting providers during static validation. Verify invalid/cyclic/unknown bindings fail closed and a running revision remains unchanged after edits.
2. **Reusable run coordinator:** bounded generic graph eligibility and execution for supported Rust/typed-capability operations, with explicit delegated domain boundaries, cancellation/error propagation, existing session/task ownership and concurrency/access constraints. Verify both event-driven async branching and fixed-tick workloads against the same advertised semantics.
3. **Authoritative state, effects and cooperative control:** run/session/attempt correlation, snapshot and observable transitions, partial/unknown outcomes, bounded trace loss and resync, generation-fenced owner-specific safe points. Verify stale control rejection, unknown results, committed-prefix preservation and live control without impeding unrelated work.
4. **Optional inspector:** a toolkit-neutral observation projection or consumer that demonstrates a graph/timeline without independent authority. An actual independent graphical frontend requires its own supported interface and authorization gate; it is not required before headless gates.
5. **Visual editor subset specification:** define editable operations, round-trip/source-of-truth, typed edges and safe publication/upgrade before any executable editor implementation issue.

Research [#83](https://github.com/aschenrot/riesel/issues/83) supports these decisions but **does not** prove public compatibility, production authorization, durable replay, arbitrary `!Send` scheduling or faithful visual authoring. This plan is **accepted product/architecture direction**, not a statement that any gate has been implemented or that the platform is production-ready.