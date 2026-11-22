# #75 — Consolidated Operational Contract: technical acceptance proposal

**Status: DECISION PROPOSAL — not accepted, implemented or merged.**  
**Review date:** 2026-11-22. **Target:** current \`main\` at branch creation.  
**Primary owner:** [specification #75](https://github.com/aschenrot/riesel/issues/75). **Independent product owners:** [developer workflow #68](https://github.com/aschenrot/riesel/issues/68), [Manager #59](https://github.com/aschenrot/riesel/issues/59), [native GUI / app export #60](https://github.com/aschenrot/riesel/issues/60).  
**Earlier draft:** [PR #86](https://github.com/aschenrot/riesel/pull/86), \`docs/product/riesel/operational-technical-decision-review.md\` (research/decision register, **not accepted**). This proposal is based on newer \`main\` instead of replacing PR #86's older product-definition version.

## 1. Product question and decision boundary

**Outcome to preserve:** an independently authored Riesel application can be built, observed, launched by its actual owner, and deliberately connected to optional interfaces. Different tools (a foreground developer workflow, installed Manager, a native GUI, and later authorized graph/query inspectors) must not invent incompatible meanings for *the same real-world application, generation, observation, command or outcome*. Ordinary headless/local use must remain independent of Manager, GUI toolkit or an always-on daemon.

The product owner has accepted **directions** and Manager first-release product requirements, not the technical contract or a particular public API. [Product definition](../../docs/product/riesel/riesel.md), [delivery process](../../docs/project/delivery.md), [runtime lifecycle](../../docs/product/riesel/runtime-lifecycle.md) and [authoring v3](../../docs/product/riesel/api-authoring-v3.md) remain authoritative for delivered behavior. Do **not** close #75 from green CI or this proposal.

**Single proposed technical decision for #75:** **ACCEPT**, after explicit owner/maintainer review, a *small, transport-neutral operational semantic contract* with product-specific ownership/transport/security adapters and **no mandatory shared process, registry, schema file, GUI framework, transport or daemon**. This accepts **common meanings and invariant obligations**, not public Rust type names, serialization, persistence, authentication or OS service mechanisms.

**Why now:** research has tested distinct positive and negative aspects of session lifecycle, external attachments, actual OS owners, GUI calls, caller identity, pre-dispatch revocation and lost replies. More UID/label-only experiments cannot decide the shared semantic ownership question. The remaining evidence concerns implementation/trust selection under #59/#60, and runtime-to-external epoch integration.

## 2. Authority and identity model — distinct by construction

| Concept | Authority that may assert it | Scope, use and critical non-equivalence |
| --- | --- | --- |
| **Authored application definition** | Author / SDK-derived versioned declaration | Logical identity, not a process, OS user or Manager registration. Explicit published identities have intentional compatibility rules; display names are not proof. |
| **Capability / query / operation contract** | Author, generated schema and selected provider | Versioned declared operations, types and errors. An internal capability definition is **not** automatically a public interface or caller grant. |
| **Installation / Manager registration** | Trusted installer/operator and protected Manager registry, *when installed* | Stable logical registration bound to approved artifact/config/policies. An independent Cargo application does **not** require a fabricated Manager registration. |
| **Artifact / configuration provenance** | Builder, verified installation or authorized executable launch | Concrete build/content/immutable activation configuration, not a PID, session ID, app definition or attestation by the program itself. |
| **OS launch attempt** | The actual process owner: foreground tool, installed protected OS owner, or other explicitly delegated owner | One immutable attempted start plus an independently observed OS process/service invocation. PID, unit name or port **alone** cannot attest the attempt. A restart makes a new OS attempt. |
| **OS login session / execution scope** | OS session/login/service authority and declared deployment policy | User-session versus machine-service ownership; same UID may have several concurrent distinct login sessions. Not the Riesel application runtime session. |
| **Riesel application-session generation** | **Riesel runtime** at every successful whole-session construction/recovery | Fresh on every whole-application session even if OS PID and address remain unchanged. External publication can reference it only via an explicitly accepted attested bridge. It is **not** authentication, a secret or the OS attempt ID. |
| **Provider/instance identity** | Runtime/provider owner | Implementation/placement state; provider readiness or generation does not establish public-interface readiness or external authority. |
| **Execution-plan/run identity** | Existing application-owned graph/runner contract (#83) | Plan revision, graph run sequence and *current in-process run scope* are not a Manager command, app-wide externally authenticated session, or automatic permission to step a remote domain. Existing graph admission remains local/cooperative. |
| **Logical Query data authority** | Capability/storage/domain owner, subject to explicit model + consistency contract (#138) | Two independently resolved read/write aliases may not share data. A read-designated facet is not public readership permission; provider co-location does not establish logical or remotely authenticated authority. |
| **Published application interface** | Explicitly participating app + its active runtime session | Deliberate opt-in selected contract/version/operation subset; independent endpoint/attachment lifecycle; no export of all internal providers. |
| **Observed client principal** | Enforcing peer/OS/crypto verifier for the selected trust profile | Kernel UID/PID/LSM label, transport identity or certificate are **evidence attributes**, not independently an issued authorization or human approval. |
| **Trusted grant issuer / client authorization** | App policy authority for app export; separately protected operator/Manager policy authority for privileged Manager actions | Issuer, non-self-selectable principal where required, operation scope, target, lifetime and revocation. Trust level belongs to each interface; a Manager grant cannot follow from an app-facing capability grant. |
| **Client attachment** | Published app interface after verifying principal/grant/version/session | Short-lived bound right to invoke selected operations for *one* active app/session context; connection address, session generation and attachment token are not interchangeable. |
| **Management command / receipt** | Authoritative Manager control owner if installed | Exact intended registration, OS attempt, verb/arguments, authorized policy/operator, admission and actual outcome; receipt retention must be stated. Not a generic graph run or application RPC ID. |

**Identity relation:** a running app may be represented by *(logical app, approved artifact/config where applicable, authoritative OS attempt, current runtime generation)*. A published interface additionally has *(interface contract/version, policy/trust profile, current grant and attachment)*. These are **relationships with verifiable issuer evidence**, not a proposed wire tuple or universal machine key. Unmanaged headless apps legitimately have no Manager registration. A Manager can reconcile an OS attempt before an app has begun a Riesel session. The application can create a new Riesel session without a new OS launch.

**Current implementation caveat:** on \`main\`, \`SessionToken\` is an **in-process revocable identity**, with \`shares_session_with\` comparing shared ownership; it does **not** expose a production external generation. The graph runner already has its own in-process \`GraphRunIdentity.session_generation\` sourced by the graph admission owner; this is **not** an externally authenticated common session key. [PR #95](https://github.com/aschenrot/riesel/pull/95) tests a feature-gated runtime-issued external epoch **on its stacked draft branch**, not on \`main\`. Do not silently equate these three representations or claim that an export API exists.

## 3. Minimal semantic obligations, before any public API

### 3.1 Observation = scoped evidence, not a universal ready boolean

An observation must identify **subject** (definition, OS attempt, provider, app handler, specific exported interface, graph run or Query facet), **observer/authority**, **observed-at/freshness** and **app generation / OS attempt where applicable**, plus an explicit *basis* and ability to be *unknown/stale*. No mandatory timestamp clock or global enum is selected; consumers must not promote a weaker observed fact to a stronger one.

| Independent fact | Valid evidence example | Does NOT prove |
| --- | --- | --- |
| Definition/artifact known | Verified cached SDK definition or trusted artifact provenance | App is started/ready, artifact matches current process without comparison |
| Plan/preflight valid | Existing Riesel validation within an intentionally executed application | No user code executed before preflight; endpoint exists |
| OS attempt alive/owned | Authoritative OS service/job/owned-child observation | Providers, application handler or exported interface are callable |
| Provider ready | Existing validated provider handshake | Entire application session or UI export is ready |
| App session / handler active | Runtime-owned live status | An external client can attach or is authorized |
| Specific interface bound | App-published listener state for that generation | Typed **authenticated and authorized** operation can complete |
| Specific operation callable | Successful permitted operation under verified current interface/session | All other operations, entire application health, permanence of availability |
| Revoked / disconnected / unknown | Verified revocation or absence of fresh authoritative evidence | That any ambiguous in-flight command did not execute |

**Counterexamples:** real PAM/GUI research [#147](https://github.com/aschenrot/riesel/pull/147) observed a callable real native GUI while its PAM system service still reported \`activating\`. A provider \`RIESEL_READY\`, socket bind, binary's \`--print-definition\`, OS process existence and HTTP-ready line are different scopes. [#79](https://github.com/aschenrot/riesel/pull/79) demonstrated that executing a Rust application for \`--print-definition\` executes user setup; passive cached inspection must carry provenance and freshness instead.

### 3.2 Publication and session fencing

- Only the app owner deliberately publishes a selected versioned interface. No implicit export of internal providers, graph controls, storage writes, Query data or Manager operations.
- A publisher must bind that interface to the **actual runtime session** through a verified mechanism; an adapter-minted random ID, process PID or OS unit invocation is not proof of the runtime generation.
- An attachment must be bound to the expected app/interface/version, current runtime generation and a separately verified authorized principal with explicit permitted operations. A matching generation/version is **not** a grant.
- Runtime whole-session revocation ends *new* admission through the old interface **before** new-session publication becomes callable, including same-PID recovery. Do not transparently rebind old attachments. Already dispatched work may finish; cancellation is not an undo mechanism.
- Revocation and in-flight dispatch require a documented linearization/ordering boundary. A synchronous blocking lock in the #95 research can indefinitely delay teardown; it is **not** a portable async solution. A production implementation must set honest admission/drain behavior and boundedness.
- Client detach/GUI window closure never implicitly terminates the backend. Reattach requires new verification, grant, current generation and authoritative state refresh; never replay an uncertain mutation automatically.

### 3.3 Authorized client, grant and trust profile

**Distinct, explicit supported profiles are required; no single "local authenticated" claim suffices:**

1. **Trusted-local developer/app profile (candidate):** deliberately selected low-risk local app interfaces may assume same-user processes trusted. **Same-UID hostile software is excluded by declaration**, not magically prevented by loopback, socket mode 0600, origin, a per-UID grant or a random address. This profile is ineligible for protected Manager privileged mutations and must not be silently chosen when a stronger profile is required.
2. **Protected application-client profile (candidate):** if an app needs to defend against arbitrary hostile programs under its user's UID, a protected issuer **and independently non-self-selectable client identity/domain** must be demonstrated, together with safe invocation/relay/credential protections and revocation. If unavailable, the operation is not advertised as protected. The exact OS/crypto mechanism and whether every app needs this profile belong to #60, not #75.
3. **Installed Manager protected-control profile (accepted requirement, implementation open):** hostile same-UID is **in scope** for all privileged Manager CLI/browser operations in both user-session and machine scopes. A protected OS process owner and a separately trustworthy human authorization or bounded preapproved operator policy are both necessary. A root *test* controller is not an authentic approval channel. No silent fallback to profile 1 is permitted.
4. **Cross-user/remote (future):** dedicated interface/transport/security acceptance; plain TCP/loopback research, TLS availability or app-defined names do not imply approved remote identities.

**Verified limits:** [#96](https://github.com/aschenrot/riesel/pull/96) shows Linux SO_PEERCRED proves actual UID/PID but cannot tell two same-UID clients apart. [#129](https://github.com/aschenrot/riesel/pull/129) shows a verified root-owned CLI can be reexecuted/LD_PRELOAD-injected, and a permissive AppArmor label can be self-entered by unconfined actors. [#151](https://github.com/aschenrot/riesel/pull/151) reproduced this negative case **against actual Riesel Counter mutations**: a confined same-UID attacker could not transition, but unconfined same-UID code self-entered the "trusted" label and mutated the real app. Thus **observed label ≠ authorized acquisition of that label**. [#134](https://github.com/aschenrot/riesel/pull/134) demonstrated actual X11 GUI input injection; a GUI click/X11 window is not necessarily genuine human intent. [#142](https://github.com/aschenrot/riesel/pull/142) demonstrated bounded polkit AUTH_SELF_KEEP detail reuse and X11 input limitations, not universal insecurity of polkit or proven credential theft.

**Rule:** a protected channel's verifier cannot grant an operation on the basis of a peer-asserted name, readable same-user bearer credential, mutable installation path, PID alone, source-origin header, or a self-acquirable security label. When a trust profile excludes a hostile same-UID caller, document the exclusion clearly rather than disguising it as secure peer authentication.

### 3.4 Results and retries: no invented atomicity

| Outcome category | Evidence | Required client/owner behavior |
| --- | --- | --- |
| **Rejected before dispatch** | Authoritative admission/version/grant/generation refusal before any effect dispatch | No effect via that admitted path; do not generalize to unaudited bypass paths |
| **Definitive result** | Current authority confirms typed operation success or domain failure | Preserve domain error separately from RPC/transport/OS failure; partial side effects depend on domain contract |
| **Admitted, outcome unknown** | Lost reply, timeout, crash, disconnected socket or ambiguous handoff after possible dispatch | Do not claim failure/no effects; no silent replay of non-idempotent operations; reattach/inspect authoritative state or a separately accepted receipt ledger |
| **Partially completed / reconciliation required** | Durable operation phase and verified partial side effects | Explicitly report partial/blocked state; reconcile exact identity before next destructive transition |
| **Observation stale/unknown** | No fresh reliable health/command evidence | Do not invent "ready", "stopped" or "failed" from absence of evidence |

No generic exactly-once claim is implied by a correlation ID, Unix socket, event stream, idempotency *label*, transaction around a database or controller-restart test. **Manager** must own its durable command journal and OS attempt reconciliation under #59. **Application** owns domain-level idempotency/operation status if supported. A common semantic error is not a common persisted command store.

## 4. Current evidence map: verified subgates versus absent proof

| Evidence | What it demonstrates | Decisive missing link |
| --- | --- | --- |
| [#81/#82](https://github.com/aschenrot/riesel/pull/82), [#90](https://github.com/aschenrot/riesel/pull/90) | Genuine independent typed Counter calls, actual SDK-resolved facade and real independent eframe/X11 GUI | No authenticated app-client principal, export publication or Riesel runtime-bound public interface |
| [#91](https://github.com/aschenrot/riesel/pull/91) | Reusable *simulated* policy comparator against actual SDK Counter and Todo dependencies | Principal supplied by test; not authenticated externally; full Todo update/delete/revision not covered |
| [#87/#92](https://github.com/aschenrot/riesel/pull/92) | Stale endpoint replay and lost-reply negative controls, server-fenced gateway, app-authored same-PID restart trace | Gateway/app-authored external generation not runtime-owned |
| [#95](https://github.com/aschenrot/riesel/pull/95), [real research CI 44886250807](https://github.com/aschenrot/riesel/actions/runs/44886250807) | Feature-gated runtime-constructed epoch and synchronous pre-dispatch fence during actual Riesel whole-session recovery | Opt-in stacked draft only; no async production boundedness/legit external client |
| [#99/#116](https://github.com/aschenrot/riesel/pull/116), [#145](https://github.com/aschenrot/riesel/pull/145) | Genuine Todo user service survives controller loss; same-UID user owner is directly stoppable; actual distinct session-bound Riesel Counter owners, machine service survives logout | Protected first-release Manager control and real graphical login not proven |
| [#139/#143](https://github.com/aschenrot/riesel/pull/143) | Actual protected OS Counter exact attempt, wrong/replayed stop denied; durable root *test* journal and uncertain stop reconciliation | No verified trustworthy same-user operator consent or complete Windows/macOS implementation |
| [#147](https://github.com/aschenrot/riesel/pull/147) | Two genuine native GUIs in distinct same-user PAM/logind scopes; unrelated logout leaves exact app alive, bound logout ends it; both can use intentionally unauthenticated Counter RPC | Login-session association does not enforce app capability admission; shared virtual Xvfb is not trusted desktop login |
| [#149](https://github.com/aschenrot/riesel/pull/149), [real CI 44977074474](https://github.com/aschenrot/riesel/actions/runs/44977074474) | Actual Counter TCP bypass closed by Linux PrivateNetwork, Unix socket kernel-UID grants and pre-dispatch revoke, **two same-UID clients both accepted** | Per-UID grant is not distinct app-client identity; root test issuer and gateway epoch not production |
| [#151](https://github.com/aschenrot/riesel/pull/151), [real CI 44978954601](https://github.com/aschenrot/riesel/actions/runs/44978954601) | Real AppArmor SO_PEERSEC + Counter: already confined same-UID attacker rejected, **unconfined attacker self-entered trusted label and mutated app** | Trusted-client domain **issuance and complete non-bypassability** unverified |
| [#83 accepted execution plan](../../docs/product/riesel/execution-plan-contract.md), [#138 accepted Query direction](../../docs/product/riesel/riesel.md) | Local owned graph execution/inspection is implemented in bounded scope; optional typed Query semantics are accepted as product direction | Local run-control and read-designated query are **not** external authorization or permission to invent a universal publisher |

All research PRs cited above have bounded real Linux tests **on their own revisions**, not a merged compositional security proof. **No evidence supports** claiming "Riesel currently ships a secure app exporter", "protected Manager is implemented", "all clients under one UID are individually authenticated" or "Linux proves Windows/macOS". The recent #138 product decision and #83 accepted graph deliverables are already on current main; PR #86's older product-definition diff should not overwrite or silently narrow them.

## 5. Architecture candidates, compared on the same full outcome

The unit of choice is the **shared semantic boundary**, not the first IPC implementation. Assess against: correctness and authority provenance, independent app/dev UX, extensibility to GUI/Query/graph inspection, runtime recovery, input/operation security, performance, OS portability, cost of future migration, observability and testability.

| Candidate | What it preserves / where it wins | Cost / failure and future-proofing risk | Disposition |
| --- | --- | --- | --- |
| **A. Product-local adapters with independently defined semantics** | Minimal shared surface now; easy to tailor Todo HTTP, local CLI, Manager to different trust needs | Repeated incompatible definitions of app identity/ready/session/error; future cross-interface attachment migration becomes expensive | Retain as escape hatch, **not** recommended as the common normative vocabulary |
| **B. Small transport-neutral semantics + product-owned adapters** | One meaning of generation/observation/result/grant, with independent process owner, GUI toolkit, transport and local SDK paths; compatible with multi-client apps without forced daemon | Requires strict semantic versioning/evidence and tests across app shapes; risk of vague shared types that pretend to verify authority | **Recommended conceptual acceptance for #75**, without freezing types or module boundaries |
| **C. Mandatory global daemon/Manager/protocol for all apps** | Central discovery/admission could simplify some installed control workflows | Makes independent Cargo/headless/game apps depend on privileged always-on service, conflates app exports with Manager control, expands failure/attack surface | Do not mandate; may be independently justified for installed Manager under #59 |
| **D. One concrete OS-local IPC transport as common contract** | Easy common implementation/early local UX on one platform | Linux UDS/PrivateNetwork/labels not automatically correct for remote, Windows/macOS, trusted GUI clients or in-process high-frequency query/graph domains | Transport adapter only, not the semantic contract |
| **E. Fully generic capability broker/query/event graph service** | May support rich future subscriptions, telemetry and remote inspectors | Unaccepted universal serialization, data ownership, scheduling, latency, security and persistence obligations; framework bias | Defer as a platform mechanism pending real cross-domain needs; preserve extension points for optional streams |

**Counter-bias check:** The decision must not favor B merely because it is "more architectural". B is justified **only** where repeated cross-product meanings lead to incompatibility; it must be small enough that a plain headless Rust app need not implement absent fields or install a service. Conversely, do not reject B merely because A requires fewer lines today: Counter, Todo, Manager and visual inspector already need consistent session/readiness semantics. This is *one* accepted conceptual vocabulary, **not** an expectation that every domain implements one request protocol or data persistence model.

**Growth horizon stress cases:** app headless runtime and independent Cargo workflow; persistent Todo and native GUI; manager-owned user-session and machine-scope apps; multiple distinct GUI sessions with explicitly authorized sharing; later graph inspector and bounded visual composer; Query facets with stable model/data-owner relationships; event subscriptions/telemetry with partial/loss-aware state; game/engine simulation with high-frequency local tasks. The shared model must **not** force high-frequency ECS/frame/Query reads through network IPC, compel universal runtime scheduler ownership or conflate read interface publication with permission to query protected data.

## 6. Proposed #75 acceptance matrix (one owner decision, no repetitive microdecisions)

| Decision proposed | Disposition for #75 | Consequence / independent owner |
| --- | --- | --- |
| Common conceptual identity, owner and lifetime distinctions in §2 | **ACCEPT semantics** | #75; later typed representation only after version/interop design |
| Provenance-qualified, scoped observation and explicit unknown/stale facts (§3.1) | **ACCEPT semantics** | Each observer retains own authority; Manager's "OS alive" never means "app callable" |
| Actual runtime authority for app-generation publication and stale fencing (§3.2) | **ACCEPT invariant**, **DEFER implementation/async fence selection** | #75 runtime bridge with #60 publisher; cannot treat gateway-generated ID as runtime proof |
| Deliberate app interface export and per-interface authorized operation admission (§3.2–3.3) | **ACCEPT invariant**, **DEFER export API, issuer, transport and trust enforcement** | #60 implementation Gate G-A; trusted-local and protected profile distinct |
| Strong hostile-same-UID control for Manager | **ALREADY ACCEPTED requirement**, **DEFER technical authorization mechanism** | #59 owns protected OS principal, human/policy approval, browser, platform owner |
| Lost-reply/partial/unknown result semantics (§3.4) | **ACCEPT semantics**, **DEFER durable Manager receipt mechanism** | #59 owns durable OS command receipt; app owns domain status if supported |
| New shared daemon, mandatory registration, unified serializable schema, generic event broker, universal Query/graph API | **NOT ACCEPT** from this decision | Separate product need and evidence before any such mechanism |

**Requested maintainer action:** mark this **conceptual semantic specification** ACCEPT / REVISE / DEFER / REJECT in #75, noting any necessary adjustment to §2–§3. An ACCEPT would authorize drafting a **narrow durable architecture contract** and bounded implementation Issues; it **does not** authorize a public authentication protocol, OS helper, service, network exporter, release or closing unrelated #59/#60/#68 decisions. Do not close #75 until the accepted contract has been documented in the appropriate authoritative location, with exact cross-product scopes and no conflicting older text.

## 7. Implementation decomposition *after* semantic acceptance

**Common semantic and runtime bridge (a coherent owned slice, not a collection of API guesses):**
- Define minimal *internal* identity/evidence/outcome shapes only where two consumers need to exchange them; explicitly separate trusted observations from unverified metadata. No forced global serial format, app registry or public crate until it reduces actual duplication.
- Bridge an external app-session publication to the real runtime-owned \`SessionToken\` (reuse #95 research, compare bounded async admission coordinator vs lock), with deterministic new generation on whole-session recovery, stale refusal **before** side effects, session withdrawal and correct outcome uncertainty. Check against an actual SDK-resolved Counter, then Todo and graph-run observation as *consumers*, not invented global provider identities.
- Preserve compatibility with existing \`Application\`, generated capability contracts, \`Context\`, graph-run observer and Query authoring decisions. No second provider registry or a universal query executor.

**Independent product-owned increments (may research/implement in parallel once their own gates permit):**
- **#68 foreground build/launch**: independent Cargo artifact discovery, owned-child lifecycle and truthful passive-vs-executable inspection. Do not block on Manager authorization or on publishing app capabilities; authenticated interface attach needs #75 semantics.
- **#60 app publication/GUI**: compare generated facade versus bounded reusable app-owned exporter and app-specific HTTP across **full relevant Counter + Todo operations**, including read/write/revisions/error mapping and multiple explicit clients. Choose local trusted-user vs stronger verified client isolation based on product requirements; do not silently advertise the research UDS/AppArmor profile as strong auth. Native toolkit remains swappable.
- **#59 installed Manager**: trusted registration/artifact/update/data inventory, secure per-attempt OS ownership and reconciliation, CLI/browser over one **protected** command authority, no unconstrained same-UID fallback. Linux may be implemented first; **first release still requires actual Linux/Windows/macOS behavior** in both user-session and machine-service scopes, with Linux-only routine CI and real manual platform release evidence.
- **Visual inspector / Query**: preserve local operation admission and explicit domain-owned data/read consistency. External/public inspection is a deliberate authorized app interface; it does not inherit internal graph controller or storage permissions automatically.

**Do not block all implementation on M-S2 human consent.** A local runtime bridge, opt-in trusted-local app export, independent Cargo workflow, graph local inspection and unrelated data work are separable. **Do block** any *claimed protected Manager write release* or *claimed strongly isolated app client* until the actual respective trust gate passes. Product support levels must be truthfully labeled.

## 8. Acceptance verification and explicit stop rule

A decision is stronger when its falsifiable tests span independent shapes, but green individual experiments are not a production guarantee. Suggested verification of any proposed shared implementation:

1. **Two app shapes** (SDK-resolved Counter and persistence/revisions with Todo), plus independent Cargo project: same identity/observation semantics, no second provider resolver and no mandatory Manager.
2. **Whole-session restart in unchanged process**, old published descriptor and old attachment refused before mutation; runtime owns generation, not gateway; realistic async admission/teardown does not hang indefinitely. A new OS attempt and a new runtime generation remain independently observable.
3. **Authorized/unapproved client** for each declared trust profile: wrong app/interface/version/operation/grant, forged metadata, replay, same UID, cross-session sharing when expressly allowed. If stronger client trust depends on an OS domain, verify its *entry* cannot be self-selected or injected into; otherwise declare unverified/out of scope. Actual #151 negative is a mandatory regression.
4. **Readiness provenance**: definition inspected, provider ready, OS unit activating/alive, app handler and specific typed caller availability remain separately reported; historical/stale evidence never promoted to live ready.
5. **Uncertain effects**: injected post-dispatch reply loss, controller restart and old-attempt receipt; no implicit destructive retry. Verify actual state/receipt before claiming a definitive outcome; do not assert exactly-once from in-memory ids.
6. **Operation-domain evolution**: Query read facet must validate logical data owner/model/consistency; graph/run observer identity does not replace app session or grant; no default public graph stepping. Local performance remains bounded independently of network adapters.
7. **Portability/ownership**: independently tested Linux process/user-session/GUI cases and planned native Windows/macOS owner/approval cases. Do not infer Windows/macOS security merely from shared Rust types or successful Linux CI.

**Stop repeating research** once the semantic owner decision is made and the candidate mechanisms' differences are irrelevant to that decision. Focus subsequent experiments on a stated **implementation blocker**: runtime-issued external epoch integration, truly non-self-selectable authorized client, protected Manager operator consent, full Todo mapping, realistic async revocation or OS-specific parity. Do not add another UID/file-mode/label-only experiment without a decision-changing new property.

## 9. Integration audit — why not edit PR #86 in place

At this review, [PR #86](https://github.com/aschenrot/riesel/pull/86) remains a **draft** with a product-definition file originating from the older common merge base \`a35ac7c9\`, while \`main\` has advanced with **accepted executable-graph** and **reusable typed Query** product directions. GitHub comparison found the PR and \`main\` **diverged**; its copied product definition lacks current \`main\`'s newer §Execution plans and §Reusable typed Query material. Directly replacing the file on that old branch could regress accepted text or create an avoidable merge conflict. A PR86 rebase/merge must preserve all later accepted product/roadmap decisions. This supplement therefore starts from **current main**, does not modify the product definition and does not pretend older PR86 is merged or superseded as an accepted contract.

**Audit findings to preserve:** (1) prior documents called #83 a research-only scheduler, but #83 is now an **accepted** bounded execution-plan contract with actual local runner; (2) #138 Query semantics are newly accepted as direction but remain unimplemented and must not be subsumed into app-export permissions; (3) PR86's shared-operations proposal assumes a runtime-issued external generation, while only opt-in research #95 proves an implementation path; (4) OS label/client-UID checks look stronger than the actual #149/#151 negative controls; (5) Linux CI cannot imply the accepted first-release Manager's Windows/macOS parity; (6) future native/multi-GUI and game workloads are extension tests for authority and overhead, not requirements for one global daemon.

## 10. Review checklist / integration policy

- [x] Re-read current product, authoring, runtime, delivery and current #75/#59/#60 issue state; inspect prior #86 decisions and later research including #95/#147/#149/#151.
- [x] Separate **accepted product outcomes**, **proposed shared semantic invariants**, **research-only tested mechanisms**, **unverified safety claims** and **implementation owner**.
- [x] Preserve newer accepted graph/Query outcomes on current main; avoid silently changing product/roadmap text.
- [x] Compare small shared semantics with independent adapters, universal service, one-transport standardization and generic broker without hardcoding a backend.
- [ ] Owner/maintainer explicit #75 disposition and rationale on the existing specification Issue.
- [ ] If accepted, update an authoritative architecture contract preserving current product requirements and then create coherent implementation Issues; keep #59/#60 technical mechanisms gated separately.
- [ ] Exact final-head CI/workflow verification for this **documentation-only** draft PR; no implementation acceptance from CI.
