# Riesel — Product Definition

## Purpose

Riesel is a Rust-first, schema-driven application platform. Applications declare the capabilities they need; components provide implementations; Riesel validates, composes, and runs them.

The product objective is to remove repeated infrastructure wiring without forcing application code into one UI, process, or execution model. Applications can combine generated definitions with ordinary Rust behavior.

This document specifies intended product outcomes, **not** the current implementation status. See the [implementation roadmap](implementation-roadmap.md) for verified delivery.

## Problem and users

Application developers repeatedly rebuild model validation, capability communication, service lifecycle, state management, and integration with tooling. This complicates reuse and introduces differences in behavior between local and independent execution.

Riesel primarily serves Rust developers building applications, tools, and headless services. Real-time applications and games are potential specialized uses of the same foundation; they do not dictate the core runtime architecture.

## Product model

| Concept | Responsibility |
| --- | --- |
| **Application** | Logical software unit with requirements, behavior, and a lifecycle; not necessarily a single process |
| **Definition** | Inspectable, non-executable description of models, requirements, and configuration |
| **Capability** | Versioned contract that consumers depend on, independent of implementation |
| **Component** | Implementation of one or more capabilities, potentially sharing state |
| **Provider** | A registered implementation selected for a requirement |
| **Instance** | Actual running provider execution with a distinct identity and lifecycle |

A component is **not inherently an operating-system process**. Local and independently executed providers can implement the same contract, subject to compatible semantics. Deployment and execution policy determine where components run.

The application uses capabilities; components implement capabilities; Riesel resolves and runs them.

## Accepted product requirements

### Composition and developer experience

- Developers can define application requirements, models, and capability contracts using supported Rust authoring APIs; portable, inspectable definitions are derived without a second manually synchronized schema.
- A capability requirement can be resolved to a local, explicitly managed, or configured external implementation.
- Normal application authoring does not require handwritten transport clients, dispatch loops, or manually assigned ephemeral infrastructure IDs.
- Ambiguous requirements, invalid definitions, and incompatible provider selection fail with actionable diagnostics rather than implicit fallback.
- Ordinary custom Rust behavior remains possible without requiring a particular ECS, GUI, or transport implementation.

### Execution and lifecycle

- Application logic can execute headlessly, with no graphical window.
- Application, component, provider-instance, and process lifecycles are distinguished explicitly.
- Startup ordering, shutdown, cancellation, and recovery respect declared dependencies and stated ownership boundaries.
- Recovery never implies that ambiguous external side effects were not executed; individual commands are not silently replayed.
- Independent native window hosts use separate operating-system processes, with lifecycles independent from application logic. This is an **accepted target requirement**, not a claim of current delivery.

### Executable plans and operational inspection (partially delivered)

- Developers can compose and validate inspectable, versioned **execution plans** of registered Rust/capability operations and run them **headlessly**. This operation graph is distinct from capability/provider selection and the startup dependency DAG.
- Riesel provides reusable general graph execution for the operations it owns, with **one declared admission/control authority per execution boundary**. Specialized domain runtimes may retain their own internal tick, frame and system scheduling.
- Developers can inspect the exact plan revision and authoritative run state, correlate session/run/attempt events, and cooperatively pause, advance or resume **supported** safe points without arbitrary thread suspension.
- Cancellation, failure and **uncertain outcomes after dispatch** preserve confirmed committed effects; lost events or replies never imply rollback, nonexecution or permission to retry.
- **Optional graphical inspection** is an accepted product goal over the same headless semantics, not a required GUI or separate execution owner.
- **Editable executable visual composition** is an accepted **long-term direction** under [#111 option C](https://github.com/aschenrot/riesel/issues/111#issuecomment-7074722751): Rust-authored and document-owned plans share validated execution semantics, with **one authoritative semantic authoring source per plan**. The owner additionally [accepted S2 as the first editor subset, conditional on an extensible path to S3](https://github.com/aschenrot/riesel/issues/111#issuecomment-7074822182): add/remove and bind only expressly permitted registered nodes; connect control dependencies; edit only publisher-authorized triggers and symbolic constraints/domain boundaries, with whole-plan validation and no partial execution. **Executable typed ports/value edges (S3) remain a future supported evolution target**, not a guaranteed delivered runtime. S2 control dependencies must retain their meaning when S3 arrives; unsupported executable features must fail closed rather than be silently discarded. The [accepted V2 plan-version and ownership direction](https://github.com/aschenrot/riesel/issues/111#issuecomment-7075163935) separates **immutable semantic plan revisions** from **non-executable layout revisions**, requires deliberate Rust/document source-ownership transfer, and makes stale semantic/layout writes fail visibly. Layout-only changes cannot publish an executable revision; existing runs retain their exact semantic revision. The [owner also accepted A2+B2+C3](https://github.com/aschenrot/riesel/issues/111#issuecomment-7075539465): **application-owned inert, versioned editor-safe operation descriptors** with runtime confirmation of compulsory resource/domain/affinity constraints; **strict executable document versions and required-feature validation**, preserving unsupported future source read-only rather than rewriting it; and **application-authoritative immutable candidate publication with separately authorized runtime activation**. A headless Rust app needs no Manager or GUI to author/execute its plans. The exact descriptor SDK/API, document codec, durable revision/CAS store, identity/grant mechanism and activation recovery adapter remain implementation design choices, not delivered features. Arbitrary Rust closure serialization, unrestricted visual Rust authoring and delegated ECS-internal editing are not accepted guarantees.

**Accepted specification decisions (22 November 2026):** [C](https://github.com/aschenrot/riesel/issues/111#issuecomment-7074722751), [S2 with future S3 requirements](https://github.com/aschenrot/riesel/issues/111#issuecomment-7074822182), [V2 semantic/layout/source ownership](https://github.com/aschenrot/riesel/issues/111#issuecomment-7075163935) and [A2+B2+C3 descriptor/document/publication architecture](https://github.com/aschenrot/riesel/issues/111#issuecomment-7075539465). Research-only [PR #169](https://github.com/aschenrot/riesel/pull/169) and [PR #185](https://github.com/aschenrot/riesel/pull/185) verified bounded actual-SDK behavior and V2 local persistence, not a product document ABI, authenticated exporter, live editor or crash-safe activation. Execution-port/value-transfer research is tracked separately in [#187](https://github.com/aschenrot/riesel/issues/187); it does not delay S2 implementation. #111 is a **specification**; acceptance does not imply delivery of a graphical editor.

Durable boundaries and delivery gates are defined in the [accepted execution-plan contract](execution-plan-contract.md), based on [specification #83](https://github.com/aschenrot/riesel/issues/83). A bounded session-owned headless runner, live observation, confirmed-effect prefixes and cooperative **outer admission** controls are implemented in the reference SDK; see the [runner's verified scope](execution-runner.md). The optional graphical inspector, editing contract, external authorization and deeper domain-owner integration remain separate and are **not** delivered by these local APIs.

### Bounded typed-node authoring and Grid Workbench reference outcome (accepted direction; not delivered)

- **Accepted product direction — [#175, 22 November 2026](https://github.com/aschenrot/riesel/issues/175#issuecomment-7076113507):** Riesel shall support a genuinely **editable, typed input/output node experience** for a Grid Workbench reference application. The initial user journey is `Noise2D -> Threshold -> CellularAutomata2D -> DualGrid2D -> [optional] Render2D`, with explicit connected data types, parameters, branching, errors, per-stage and per-iteration inspection, revision-aware reruns and bounded retention. The simple `Noise2D` node directly produces a sampled scalar grid; optional field-valued/lazy nodes may be added separately. A fixed ordered pipeline or a read-only graph is a useful increment, **not completion** of the editable user outcome.
- **Accepted cross-application architecture direction — [#187, 22 November 2026](https://github.com/aschenrot/riesel/issues/187#issuecomment-7076117427):** Future S3 typed value edges are an **optional, explicitly versioned extension of the existing Riesel-owned validated execution graph**, for eligible registered operations. Riesel's accepted S2 control dependencies remain control-only, cheap and semantically unchanged. Unsupported *required* S3 features fail closed and cannot be executed through an S2-only projection or stripped by older editors. Existing #111 semantic source-ownership, immutable revision, editor-safe descriptor, publication and activation requirements apply to new executable data edges.
- **Domain and data authority:** A visual **node is not automatically a Riesel capability**. Grid remains authoritative for reusable grid storage, coordinates, algorithms and dual-grid topology. Domain owners may keep large, `!Send`, native/GPU/ECS or remotely placed values and their internal scheduling; Riesel coordinates only the explicitly admitted outer boundary. Typed values may be owned immutable artifacts or authorized scoped references where appropriate; no universal transport/codec, `Arc<dyn Any>` store, RPC-per-node requirement or serialized Rust closure is accepted. A domain-owned internal graph must not compete with a separately authored Riesel outer semantic plan.
- **Reference delivery boundary:** Develop and test neutral scalar sampling, explicit occupancy classification and configurable protected-write cellular automata through independently approved Grid APIs; integrate the workbench as a **separate Cargo application** using Riesel headlessly; then deliver genuine typed-port editing through accepted platform-compatible semantics, and optional RunenRender-backed preview. Canonical numeric/occupancy/topology data survive without a renderer. No Manager, mandatory GUI, game engine or ECS is required to use the headless workflow.
- **Specification versus implementation:** #175 and #187 remain open for concrete Grid algorithm/version/precision/CA rules, real end-to-end proof, handler/port binding, data-ready/failure/unknown/cancellation, lifetime/backpressure/affinity and authorization details. No executable S3 value-edge runtime, generic editor, renderer integration or independent authenticated host is implied by accepting this direction. The independently delivered S2 authoring work proceeds unaffected.

### Optional domain-owned nested execution observation (accepted; trusted-local implementation delivered)

- A developer **may opt in** to an application/domain-owned, **read-only** observation of nested work executed inside a delegated operation, such as an ECS frame/tick or a non-game processing pipeline. An application without such an adapter retains the complete existing **outer-only** inspection and execution workflow. Neither Riesel nor the application must use an ECS to participate.
- Riesel shall offer reusable, minimal **correlation, snapshot and loss semantics** associating a domain-authoritative nested view with an exact outer application session, plan/revision, graph run and operation/attempt, and the domain's own owner-instance generation. Domain-owned work identities, state, revision/high-water marks and published capabilities originate with that owner, **not** from synthesized outer graph nodes.
- Nested observation may use **versioned domain-specific capability views and typed payloads**, with optional specialized inspector presentation. The platform-wide part must provide discoverable support/absence, bounded retention, detectable event loss and authoritative resynchronization without requiring one universal inner ECS/system/stage model. A domain's internally meaningful state and scheduling semantics remain under its control.
- Nested owner snapshots cannot imply a global event order with the outer graph, change already reported outer terminal or attempt-confirmed effect classifications, establish durable replay, or make ambiguous mutations safe to retry. A domain owner replacement invalidates the old domain attachment independently of the outer session generation.
- **No nested control is accepted in this increment.** Internal pause/step/resume, system breakpoints and request admission remain exclusively domain-owned and require separate future specification if exposed through Riesel. This accepted product outcome does not authorize a public ABI, cross-process exporter, privileged UI reader, new provider resolver, ECS dependency or automatic child-run scheduler.

**Decision:** [#146 owner acceptance (22 November 2026)](https://github.com/aschenrot/riesel/issues/146#issuecomment-7073454321). **Evidence:** [research PR #148](https://github.com/aschenrot/riesel/pull/148), using real Bevy ECS, pinned RunenECS and a non-ECS coordinator; research-only fixtures are not approved public contracts. The [execution-plan contract](execution-plan-contract.md) specifies the durable authority/identity boundaries. The first **trusted-local read-only** Rust SDK foundation has been delivered in [#153](https://github.com/aschenrot/riesel/issues/153) / [PR #155](https://github.com/aschenrot/riesel/pull/155), using owner-authored immutable views, versioned typed payloads and a generic loss-aware header. Authenticated external inspection still requires accepted #60/#75 security and interface semantics; internal ECS stepping/control remains deferred.

### Contracts, identity, and interfaces

- Applications depend on versioned capability contracts rather than specific providers.
- Private identifiers can be derived by the framework; independently published contracts retain a deliberate stable external identity.
- CLI and programmatic interfaces should expose comparable supported platform operations with consistent error semantics.
- New capabilities and specialized execution integrations can be added without replacing the foundational application model.

### Shared operational semantics (accepted architecture; implementation pending)

- Riesel adopts one **transport-neutral semantic foundation with independent product-specific adapters** for application identity, operation/readiness evidence, app-session lifetime, deliberately published interfaces, authorization meaning and uncertain results. This is an **accepted architecture contract**, not a deployed shared service, public SDK/IPC schema or completed Manager/GUI. An ordinary headless Rust application does not require Manager registration, a GUI, daemon or exporter.
- Keep distinct and source-attributed: authored application/capability identity, verified artifact/configuration, optional installed registration, one authoritative OS launch attempt and login/service scope, Riesel whole-session generation, provider instances, published contract/version, authenticated principal/grant and client attachment, Manager command identity, and separately owned graph/Query/nested-domain identities. Matching names, PID, UID, endpoint, contract version or kernel label are not sufficient proofs of authorization.
- Operational observation is **specific, authority- and freshness-qualified evidence**, not a universal `ready` flag: process alive, provider ready, handler active, interface bound and an authorized interface operation callable are independently demonstrable. Missing or stale evidence is unknown; executing an app for inspection is not passive.
- Only deliberately selected, versioned app-facing operations may be published. The **Riesel runtime** is authoritative for each whole-application session generation; old attachments are invalidated on recovery even when the OS process remains alive. Grants, revocation and per-operation dispatch admission are separate security concerns. A lost response after possible mutation means **outcome unknown**, not implied failure or safe automatic retry.
- Each product owns its mechanism: developer tooling owns foreground runs; Manager owns protected OS control and installed lifecycle; app/UI adapters own deliberately published capabilities and GUI event loops; existing graph/nested-domain and Query owner contracts retain their own local authority. No generic Query/ECS scheduler, broker, centralized registry or remote publication is implied.
- **Security limits remain unresolved:** a trusted-local app access profile, if independently accepted, cannot be advertised as hostile-same-user protection. The Manager's already accepted privileged-control requirement **must** resist hostile same-UID processes, with separate verified operator/policy authority on its supported platforms. Current TCP loopback, Unix UID/label checks and research prototypes do not deliver that guarantee.

**Decision:** [#75 owner acceptance (22 November 2026)](https://github.com/aschenrot/riesel/issues/75#issuecomment-7073783350). **Durable boundary:** [Shared Operational Semantics](operational-contract.md). **Independent product gates:** [#68 developer workflow](https://github.com/aschenrot/riesel/issues/68), [#59 Manager](https://github.com/aschenrot/riesel/issues/59) and [#60 native GUI/export](https://github.com/aschenrot/riesel/issues/60). Concrete runtime bridge, exported API, trust mechanism and transport selection remain future implementation decisions.

### Reusable typed Query semantics and linked Record Query (accepted; first linked profile implemented)

- Riesel shall support **optional, reusable, transport-neutral typed Query semantics** for selective, correctly typed, bounded retrieval of capability-owned data. Specialized ECS, spatial, graph, search, streaming and analytical domains retain their own authoritative execution and mutation behavior and need not implement one universal record Query engine.
- Shared semantics make supported predicates, model/schema binding, ordering/continuation, bounds, consistency/completeness, unsupported-feature errors and execution budgets explicit and testable. The authoritative provider/domain owns physical query planning, indexes, consistency and authorization decisions.
- For **registered records**, the owner has **accepted a separately versioned, read-designated Query capability facet** for the first **trusted linked, same-application-session** profile. **Query reads; Storage v2 or domain-owned capabilities create/update/delete and own mutations.** A typed read-only application must register its Rust-derived model without implicitly acquiring a writable Storage RPC contract.
- An application requiring both Query and Storage uses **one declared logical model/dataset authority selection**, verified at side-effect-free preflight and again when the actual linked facets attach to **one provider instance in the same session**. A confirmed write followed by a supported live read has read-your-writes; unknown writes, snapshots, multi-record transactions and cross-session/replica guarantees are **not** implied. Split, unsupported or ambiguous ownership fails closed.
- Storage v2 applications remain compatible. The first linked Record Query profile now provides native provider-owned Memory/SQLite execution, typed correctness, bounded work and explicit errors; neither hiding writable methods nor running an unbounded SDK full scan suffices. Canonical typed model-only registration and versioned linked reader/optional writer authority were delivered through compatible app-schema/SDK evolution under [#160](https://github.com/aschenrot/riesel/issues/160) and [#161](https://github.com/aschenrot/riesel/issues/161).
- **Architectural decision versus implementation choice:** #138 did not preselect a Rust trait/contract ID, serialized query AST, schema field name or generic authority framework. The first implementation [PR #177](https://github.com/aschenrot/riesel/pull/177) now uses a separate `riesel.query.records@1` read-only facet with bounded typed predicates and owner-local continuations; this is **not** an accepted universal query engine. Model permission credentials, independent remote Query endpoints, physical index migrations, exported GUI readers, managed/external ownership, snapshots and replicas remain unimplemented and require separately accepted identity, grant, session and consistency profiles under #75/#60.

**Decisions:** [#138 shared typed semantic acceptance](https://github.com/aschenrot/riesel/issues/138#issuecomment-7073259869) and [#138 linked Record Query architecture acceptance](https://github.com/aschenrot/riesel/issues/138#issuecomment-7073879626). **Accepted architecture:** [Linked Record Query contract](record-query-architecture.md). **Implemented first profile:** [#160](https://github.com/aschenrot/riesel/issues/160), [#161](https://github.com/aschenrot/riesel/issues/161), merged [PR #177](https://github.com/aschenrot/riesel/pull/177); [usage and exact limitations](record-query-usage.md). **Research:** [PR #140](https://github.com/aschenrot/riesel/pull/140), research-only. Future cross-process grants and domain Query profiles remain separately gated.

## Product interfaces and delivery boundaries

| Interface | Intended role | Current boundary |
| --- | --- | --- |
| **Rust SDK** | Primary application and component authoring API | Experimental public `riesel` facade |
| **Application CLI** | Deployment selection, inspection, and application-specific commands | App-scoped inspection and runtime arguments exist; a full platform CLI is not implied |
| **Riesel Manager** | Registration, discovery, process supervision, and coordination | Existing `riesel_manager` crate is a process-ownership primitive, not a completed user-facing manager |
| **Manager UI** | Inspect and manage applications and infrastructure | Planned, not delivered |
| **Native window hosts** | Independent interfaces attached to application logic | Planned, not delivered |

The in-memory reference Storage provider is ephemeral; a linked SQLite provider is available for explicit single-host local persistence. Loopback TCP is not authenticated. Passing development tests does **not** establish production-wide durability, authenticated remote networking or distributed recovery.

### Durable Record Storage evolution

- Linked local SQLite Storage shall support an **explicit, offline, authorized** transformation of Rust-derived application models, with pre-mutation verified backup, preserved UUIDs, checked revision fencing, exact source/target validation, and bounded recovery.
- Ordinary application startup remains fail-closed on incompatible stored models; automatic/online upgrades and remote/managed parity are not accepted.
- The [accepted model migration architecture](sqlite-offline-migration-architecture.md) defines the controlled single-host ownership, legacy process rollout and verification boundary. Provider ownership, offline backup and an internal typed migration engine have partial delivered evidence; **the approved operator preview/consent workflow is not delivered**, so the end-to-end product outcome remains incomplete. See the [roadmap](implementation-roadmap.md) and implementation [#50](https://github.com/aschenrot/riesel/issues/50)/[#51](https://github.com/aschenrot/riesel/issues/51).

## Planned infrastructure extensions

- Deliver the accepted bounded SQLite model-evolution workflow and separately plan any broader Storage support.
- Authentication, authorization, and protected remote transport.
- Operational visibility, configurable resource budgets, and reliable deployment management.
- Standalone manager workflows and independently executed native interfaces.
- **Shared application-operational semantics are accepted** in the [#75 architecture contract](operational-contract.md), with implementation pending and no mandatory daemon, supervisor, public exporter or configuration source. **Cross-project developer tooling #68**, **Manager #59** and **native GUI/export #60** retain separate technical specification, security and implementation acceptance gates; acceptance of common meanings does not choose their adapters or shipped scope.

These outcomes require separate designs and acceptance criteria; they must not be treated as already implemented.

## Exploratory product directions

Game/ECS integrations, automatic multiplayer, subscription or messaging services, a custom UI framework, and an **unrestricted general-purpose visual application builder** remain exploratory specialized directions. These are not core-runtime prerequisites and do not automatically require new crates. This does **not** supersede the accepted optional execution inspector or the bounded long-term executable-graph editing direction above.

## Product-level success criteria

1. The same declared capability can be consumed through supported local and managed providers with equivalent contract behavior, verified by integration tests.
2. Applications can run headlessly and expose inspectable definitions with deterministic dependency and provider validation.
3. Ordinary example applications use typed authoring interfaces without repeating manual RPC or process-wiring code.
4. Session shutdown and recovery have documented, testable cancellation, identity, and side-effect boundaries.
5. Platform interfaces provide consistent operations where both interfaces are supported.
6. Future application types can extend the platform through contracts or specialized integrations without altering its core composition model.

A requirement is accepted when its specific implementation gate passes; broad product success criteria are not proof that every planned feature ships today.
