# Riesel — Shared Operational Semantics

**Status:** Accepted technical architecture **semantics** (owner decision [#75, 2026-11-22](https://github.com/aschenrot/riesel/issues/75#issuecomment-7073783350)). **Implementation status:** shared external identities, authenticated publication, secure Manager command control and general cross-process readiness are **not delivered**. This contract fixes required meanings and ownership, **not** a particular Rust API, OS mechanism, protocol, central registry, process or release.

**Sources and scope:** [Product definition](riesel.md), [delivery workflow](../../project/delivery.md), [application lifecycle](runtime-lifecycle.md), [application authoring](api-authoring-v3.md), [graph execution/observation](execution-plan-contract.md), [#75 investigation](application-operational-boundary-investigation.md), [consolidated alternatives and security review #154](https://github.com/aschenrot/riesel/pull/154). Product-specific specification owners remain [developer workflow #68](https://github.com/aschenrot/riesel/issues/68), [Manager #59](https://github.com/aschenrot/riesel/issues/59), [independent GUI and capability export #60](https://github.com/aschenrot/riesel/issues/60); separately accepted bounded graph/nested observation #83/#146 and typed Query #138 retain their own authority and delivery gates.

## 1. Architectural decision and applicability

**Selected: B — shared transport-neutral operational semantics with independent product-specific adapters.** Shared meanings prevent contradictory application identity, lifecycle, readiness, authorization and outcome claims across independent software. They do **not** require one daemon, distributed directory, shared supervisor, mandatory Manager, new manifest, generic event broker, serialized schema or public exporter for every Riesel application.

An application can be authored, compiled, run and inspected headlessly without an installed Manager, native window, publication adapter or new protocol. When a product exposes an applicable operational concept, it must preserve the semantics below and state its actual authority, evidence and limits. A product need not fabricate entities absent from its deployment; for example, an independent foreground application has no Manager registration.

**Each authority is single-purpose:** Riesel runtime owns provider composition, application sessions and in-process cancellation; the selected foreground tool or protected OS service owns each **OS launch attempt**; the application owns deliberate publication policy and domain effects; a delegated nested-domain coordinator owns its inner work; Manager owns installed registrations, exact protected OS commands, operator authorization and its own durable receipt store; a native GUI owns windows, event loop and presentation, not backend state. Sharing an observation format does not grant a product another owner's authority.

## 2. Identity, owner and lifetime distinctions

| Identity or relationship | Authoritative source and lifetime | Invariant / forbidden inference |
| --- | --- | --- |
| **Application definition** | SDK-derived author declaration and deliberately versioned published logical identity | Display name, Rust variable name, executable filename and endpoint are not interchangeable stable public identity or proof of running state |
| **Capability and operation contract** | Author/generator; versioned type, method and error definitions | Being provided internally does **not** export an interface or authorize a reader |
| **Installed registration** | Protected Manager registry or trusted installer where used; persists across launches | Optional for independent Cargo/headless use; not an OS process, build provenance or Riesel session |
| **Artifact and configuration provenance** | Verified build/installer/operator; bound to approved content/config activation | Path, self-reported hash or version string alone does not attest installed executable or executing process |
| **OS launch attempt** | The actual foreground/Manager/OS owner for one immutable attempt | PID or reused unit name alone cannot identify an attempt; the owner must reconcile the exact attempt before acting |
| **Login/session scope and execution policy** | Platform OS session / protected policy owner | User-login scope and machine-service scope differ; two sessions may share a UID; neither is the runtime session |
| **Whole-application runtime generation** | **Riesel runtime** for each new whole-session construction, including same-process recovery | Fresh after recovery even with same PID/address; no provider or adapter may invent an epoch and claim it came from runtime. An external generation is not a secret or caller authorization |
| **Provider identity/instance** | Selected provider and application runtime | Provider readiness, placement and instance lifetime do not prove a callable application interface or logical Query data authority |
| **Application publication/interface** | Explicit application author and *active* runtime session; selected contract/version/operations | No automatic export of every provider, internal capability, storage interface, graph control or domain data |
| **Client peer evidence** | Specific OS/transport/security verifier | UID, PID, MAC label, binary path, signature observation or client-supplied metadata are *claims/evidence inputs*; not automatically an individual authorized principal or proof of user intent |
| **Authorized principal and grant** | Independently trusted issuer + declared product-specific policy | Verify permitted target/operation, authenticity of principal acquisition, validity/revocation and lifetime; a grant for an app interface does not become a Manager privilege |
| **Client attachment** | Current published interface after identity/version/session/principal/operation checks | Tied to one applicable session, interface and policy. Stale attachment never silently rebinds |
| **Management command attempt/receipt** | Manager protected command authority, if installed | Binds authorized actor/policy to exact registration/OS attempt and operation; durable status and reconciliation remain Manager-owned |
| **Execution graph run/outer attempt** | Existing Riesel graph/operation owner (#83) | Its plan/revision/run/attempt identity and confirmed-effect state are not an OS launch attempt, public app session credential or blanket external stepping authority |
| **Delegated nested-domain observation** | Participating domain owner (#146), bound to exact outer session/plan/run/operation attempt and its own instance generation | Domain publishes read-only typed inner state; parent does not invent inner nodes, overwrite domain effects or inherit domain scheduler/control authority |
| **Query logical data authority** | Capability/data domain owner (#138), including model/consistency relation | Two read/write aliases may resolve different instances; read-designated method surface does not authorize an external client or establish read-your-writes |

**Relationships, not a required global ID tuple:** an installed running application may be correlated by its logical app definition, verified artifact/config, registered identity if present, authoritative OS attempt and current Riesel session. Publications and client attachments add separate contract/version/principal/grant facts. An externally visible assertion must name who issued and verified those relationships. Identity fields from one owner may be linked but never elevated to another owner's authority by matching a string or timestamp.

**Existing implementation boundary:** on `main` the runtime `SessionToken` is a revocable *in-process* identity; `shares_session_with` is not external authentication. Existing graph `GraphRunIdentity` has independently allocated run-scope generation, not the accepted externally attested application generation. The optional external runtime-epoch experiment [#95](https://github.com/aschenrot/riesel/pull/95) is **research**, not integrated public SDK or an approved async gate. The chosen implementation must preserve these distinctions and avoid duplicate competing epoch issuers.

## 3. Evidence-scoped observation and readiness

An operational observation names **the subject**, **observing authority/source**, **app generation/OS attempt/owner generation where applicable**, **evidence**, and **when/how fresh it was observed**. No specific timestamp transport or public enum is frozen. Missing, expired, unauthenticated or conflicting evidence must be reported as **unknown/stale**; a timeout does not prove stopped, failed or unchanged. Represent independent facts, not a single universal `ready`.

| Fact | Required interpretation | Must not imply |
| --- | --- | --- |
| Definition or artifact known | Valid source/provenance, static identity and permitted inspection mode | Live process, active session or callable interface |
| Preflight/plan validated | Runtime/SDK accepted that plan within its stated inspection conditions | Executing an arbitrary program for metadata is passive; external provider reachable |
| OS attempt accepted/alive | Process owner attested the specific current attempt | Provider/handler/interface ready |
| Provider started/handshaken | Runtime provider-specific readiness at that instance/generation | Entire app or public interface ready; authenticated external caller |
| App handler active | Runtime-attested current application session/handler | Public listener exists or exported operations are callable |
| Public interface bound | App-adapter binding under an actual session | Authorized client, correct method or actual successful operation |
| Specific interface operation callable | A current permitted typed operation genuinely completed or suitable current app-attested evidence | All interfaces/methods callable, immutable availability or permission for another client |
| Graph outer/nested domain observation | Separately owner-attested snapshots, versions, sequences/loss and attached exact attempts | A global atomic cross-owner snapshot or permission to step inner work |
| Revoked/stopped/failure/unknown | Scoped authoritative negative evidence or absence of current evidence, labelled precisely | That ambiguous in-flight mutations were undone |

**Inspection modes are not interchangeable.** Reading cached generated metadata can be passive only relative to its provenance and freshness. Running an application binary with `--print-definition` or invoking Cargo builds executes code and needs the appropriate trust policy. Managed-provider `RIESEL_READY`, Todo HTTP ready, systemd service `active/activating`, socket bound and real GUI event loop are separate observations; no automatic promotion to externally authenticated callability.

No application, Manager or GUI is required to implement all observations. Each reports only facts it can prove, with explicit unsupported/unknown outcomes. Logs/diagnostics are bounded and must not expose credentials or other unneeded secrets.

## 4. Deliberate publication, attachment and generation fencing

1. The application may explicitly select versioned contracts and operation subsets for publication through a product-approved adapter. Internal SDK handle, provider catalog, Query facet, graph controller or nested-domain observer is **not automatically externally published**.
2. Any externally published app-session generation must derive from **the real Riesel whole-session authority**, not an adapter-controlled random ID, OS PID, process-owner invocation or provider generation. Its external representation must carry verified current binding; its value alone is **not authentication**.
3. An attachment must validate expected app/interface/contract/version, current runtime generation, independently verified caller/principal and explicit operation grants for the selected trust profile **before dispatch**. Server authentication and client authentication are distinct; knowing a port/token/label is not by itself sufficient.
4. Whole-session teardown and restart invalidate *old* publication and **new-call admission** before successor publication is reported callable, including recovery within the same OS PID. Old attachments may not transparently rebind. An already dispatched operation may complete or remain outcome-unknown. Cancellation, revocation and a closed channel are not rollback.
5. A production async admission/revocation mechanism must specify the effect boundary, concurrency linearization, timeout and drain semantics; it cannot hold an unbounded synchronous lock across arbitrary user futures or guarantee arbitrary thread preemption. The #95 lock is evidence of a *candidate*, not an accepted implementation.
6. GUI exit/detach must not stop the app. Reconnect must verify fresh app/interface/session/principal/grant and re-read authoritative state. No implicit replay of ambiguous mutations; a read-only retry still requires a declared operation-level policy.

**Versioning/evolution:** versions must be deliberate and independently inspectable for app/public capability, nested-domain view and any published observations. A version mismatch returns explicit unsupported/incompatible outcome, not silent reinterpretation. Future event streams can reuse identity/snapshot/loss semantics without mandating an event broker now. Retain the pure local/direct SDK path without serializing every in-process graph, Query or ECS frame.

## 5. Trust-profile boundary — semantic requirement, mechanism unselected

An interface states which adversaries it excludes, which principal is accepted, which authority issues/revokes grants and which operations/objects it may touch. These are distinct from the app's runtime session identity.

- **Trusted-local app profile (possible app-specific policy, not automatically selected):** may explicitly assume co-UID programs trusted for deliberately low-risk app operations. Document the assumption. Loopback, Unix socket permissions, Host/Origin or per-UID allowlists cannot then claim hostile-same-UID isolation. No privileged Manager action may use this profile as a fallback.
- **Protected app-client profile (mechanism pending #60):** if an app promises resistance to untrusted same-UID programs, require OS/cryptographic/principal evidence whose **acquisition and use cannot be freely self-selected/relayed** by such an attacker, a protected issuer and operation-level grants. Test unconfined peers, allowed-client injection/reexecution and UI input when relevant. Fail closed when the promised profile cannot be upheld.
- **Manager protected-control profile (already mandatory #59 requirement; mechanism pending):** protected OS target and one management command authority in both execution scopes, authorizing exact operations/attempts through genuinely trustworthy operator approval or a separately authorized bounded unattended policy. A verified binary, same-UID GUI session, root *test* issuer, polkit username allow rule, unrestricted shared elevation or ordinary X11 click does not establish a protected human command. No unprotected fallback or inference from an app publication grant.
- **Cross-user/remote interfaces:** separate authentication, transport and authorization scope; this contract does not approve remote exposure or assume one platform's local peer credentials generalize to Windows/macOS.

**Executed counterexamples that bound claims:**
- [#149](https://github.com/aschenrot/riesel/pull/149) protected the actual Riesel Counter legacy TCP endpoint via real Linux private-network isolation, but two same-UID callers inherited the same UID-scoped grant.
- [#151](https://github.com/aschenrot/riesel/pull/151) correctly distinguished an already confined attacker label yet an *unconfined* same-UID program selected a permissive trusted AppArmor label and mutated the real Riesel Counter. Kernel label authenticity is not **authorized acquisition**.
- [#129](https://github.com/aschenrot/riesel/pull/129), [#134](https://github.com/aschenrot/riesel/pull/134) and [#142](https://github.com/aschenrot/riesel/pull/142) test reexecution/loader injection, real X11 GUI input influence and polkit authorization-detail/input limitations. None establishes a universally safe human approval method.
- [#147](https://github.com/aschenrot/riesel/pull/147) showed real two-session native GUIs and logind lifecycle while intentionally unauthenticated Counter RPC remained accessible to an unapproved GUI. Owning a login session is not exporting a grant.

No Linux AppArmor, polkit, UDS, sudo, systemd, X11, auth-token or broker is selected by this contract. Actual protected OS command and GUI principal choices belong to #59/#60, with Windows/macOS release proof wherever their product requirements demand it.

## 6. Operation outcomes, receipts and uncertainty

Product adapters retain explicit operation and observation provenance and distinguish:

1. **Rejected before dispatch:** authoritative validation/grant/version/generation/target failed **before any effect was admitted by that controlled path**. Do not silently report this for an unknown OS/backend path.
2. **Definitive completed result:** the correct owner confirmed success or typed domain failure. Domain failure may include partial effects unless that domain guarantees atomicity; preserve domain vs transport/RPC error.
3. **Admitted / outcome unknown:** after dispatch may have occurred, lost response, crash, timeout or revocation cannot prove absence of effects. Return/retain unknown, **never silently retry non-idempotent work**.
4. **Partial/reconciliation required:** the owner can prove some stages completed while others remain uncertain; record exact scope/attempt and require bounded reconciliation before retrying a destructive step.
5. **Observation stale/unknown:** no reliable current evidence about health or completion; never infer healthy/failed/stopped merely because status was unavailable.

**Receipt ownership:** Management command ID, immutable target/arguments, journal durability, attempt fencing, exact OS reconciliation and ambiguous stop/update/remove handling remain #59 Manager responsibilities. Application-level idempotency, transactional domain effect and operation status remain owned by the application/capability domain. A generic correlation ID or transaction in some independent store cannot magically provide exactly-once effects. An in-process graph effect recorder confirms only its own bounded outer run and does not become a Manager journal or nested owner ledger.

## 7. Adapter and extension constraints

| Consumer | May reuse shared semantics | Separate responsibility / forbidden coupling |
| --- | --- | --- |
| **Developer workflow #68** | Authored app identity, source/build provenance, owned foreground OS attempt, truthful inspection/readiness and errors | Never become required Manager, OS service owner, provider resolver or UI renderer; ordinary headless Cargo use remains valid |
| **Riesel runtime** | Own application generation/revocation and SDK capability handles; optionally provide approved app publication participation | Does not own installed app registry, protected OS commands, executable inventory or graphical event loop |
| **Manager #59** | Reuse app/attempt/session/readiness/error vocabulary for protected CLI + browser | Manager owns OS authority, registrations, artifact/update/data policy and command journal; never infer session/grants from process names or app HTTP |
| **Native GUI/publication #60** | Selected app-facing versioned capabilities, explicit attachment, current session, per-operation permission, snapshot/reconnect | No backend ownership on window close, no implicit provider exposure, no requirement for Manager or a Riesel widget framework |
| **Execution graph / inspector #83/#114** | Local run identity and owner-attested observation; can consume separately authorized publication where accepted | Run/step control stays with Riesel outer owner; external read or control is never enabled by presence of an observer alone |
| **Nested-domain observer #146** | Correlates exact outer session/plan/revision/run/operation attempt with domain owner instance/epoch and bounded loss/resnapshot | Inner owner issues its own typed snapshots; separate clocks, no global atomic order, no invented internal nodes or nested control; local-first and optional |
| **Query #138** | Versioned typed read/consistency/error semantics and declared data owner | No universal Query executor, shared implicit read/write alias authority, automatic remote read grant or forced network path |
| **Future game / engine integrations** | Reuse scoped identities, declared ownership, bounded observation, errors and optional authorized publication | Local high-frequency ECS systems/ticks remain domain owned; never require network transport, global scheduler or debugger for an application to run |

## 8. Verification, dependencies and delivery boundaries

**Acceptance of semantics does not deliver implementations.** Technical slices must retain current public behavior or deliberately gate versioned migrations and independently test the relevant proof:

- **Runtime bridge:** test existing and recovered real Riesel sessions (including **same PID**), distinct runtime-issued generations, old call rejected *pre-effect*, delayed/in-flight revocation outcome truth and bounded async teardown. Compare an async ingress/coordinator with the #95 research lock; no invented external auth.
- **Cross-product observations:** at least two independently scoped consumers (e.g. headless developer lifecycle plus a real Counter/Todo public-interface probe) report the same factual readiness distinctions and explicit stale/unknown states. No boilerplate requiring every app to install Manager. Verify passive-vs-executable inspection and false-readiness negatives.
- **Typed publication and GUI (#60 gate):** full meaningful Counter and Todo domain operations/revisions, denied wrong app/contract/version/grant and hostile same-UID cases **for whichever trust profile is actually accepted**; no insecure claims from #149/#151. Verify actual responsive GUI detach/reconnect and explicit mutable-effects uncertainty.
- **Protected Manager (#59 gate):** independently OS-owned attempts, restricted same-UID operator authority, authorized exact action, durable command journal, native CLI/browser parity and platform-specific user-session/machine-service parity. Actual Windows/macOS manual release proof remains mandatory; Linux research alone is not a product release.
- **Nested/Query growth check:** existing graph in-process and #146 nested-domain owner observation remain independent from a future public exporter; Query logical read/write data authority and consistency are explicit. No global work/receipt lock on hot simulation code or confused published query permissions.

**Proof levels must remain labelled:** research-only OS prototypes, integrated SDK tests, platform acceptance and release evidence are not interchangeable. Owner-reviewed specification [#75](https://github.com/aschenrot/riesel/issues/75) settles the conceptual boundary; implementation Issues define concrete behavioral/tested slices under the [delivery model](../../project/delivery.md).

## 9. Explicit exclusions and change control

This contract **does not accept or require**:
- A new crate, public `OperationalContract` type, manifest/JSON/protobuf, SDK trait, cross-product error enum, runtime API or serialized schema.
- A universal daemon, shared process owner or installation prerequisite for foreground/headless apps.
- A specific IPC transport, bridge, app exporter, OS credential/broker/consent flow, GUI toolkit, package manager, signing/trust anchor, Query language/executor, ECS integration or nested graph control.
- Remote or cross-user application/Manager access, exactly-once effects, automatic replay, forced task interruption, durable subscriptions or arbitrary UI graph editing.
- Production security, supported Windows/macOS parity, safe update/uninstall, Manager console or generic authenticated external inspector as already delivered.

Significant deviation from these semantic guarantees, especially security, recovery, ownership, data consistency or compatibility, requires returning to the owning specification/decision rather than silently broadening an implementation PR.

**Decision record:** owner [ACCEPTED B on #75](https://github.com/aschenrot/riesel/issues/75#issuecomment-7073783350), compared with independent per-product conventions (A) and mandatory central service (C). Alternative research [#154](https://github.com/aschenrot/riesel/pull/154) and old technical review [#86](https://github.com/aschenrot/riesel/pull/86) are historical evidence and pending drafts, not competing accepted contracts.
