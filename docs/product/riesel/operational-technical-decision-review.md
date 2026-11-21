# Operational platform — technical decision review (#75 / #68 / #59 / #60)

**State: proposed technical specification, NOT accepted.** Reviewed against `main` at `439350f9247717542696f963e6fc6406df2e5b0f` on 2026-11-21 and the exact-head research cited below. This document is a review annex to the [accepted **product directions** under review](operational-products-direction.md), not a new source of implementation authority. Do not infer new SDK, wire, trust, CLI, Manager, GUI or milestone approval from its inclusion in draft PR #86. Acceptance must be recorded in the respective specification Issues and reviewed architecture contracts under [delivery.md](../../project/delivery.md).

## 1. Decision boundary and critical review

The product owner accepted four *outcomes*: one coherent set of operational meanings (#75); optional tooling for independent Cargo projects (#68); a registered, single-host Manager with authorized CLI/UI operations (#59); and independent native GUI processes consuming deliberately published application interfaces (#60). **No concrete implementation mechanism has been selected.**

The research establishes meaningful positive evidence without closing the security boundary:

| Topic | Executed evidence | Not proven by that evidence |
| --- | --- | --- |
| Cross-project developer workflow | [#88](https://github.com/aschenrot/riesel/pull/88), Linux [research 44877585370](https://github.com/aschenrot/riesel/actions/runs/44877585370) and [Validate 44877585382](https://github.com/aschenrot/riesel/actions/runs/44877585382): independent Cargo, trusted inspection, built static Todo browser, retained SQLite, error probes | A shipped generic CLI, universal graceful stop, authenticated application readiness, automatic frontend tooling |
| App identity / stale external clients | [#87](https://github.com/aschenrot/riesel/pull/87) shows stale-client mutation after endpoint reuse and committed mutation despite lost reply. [#92](https://github.com/aschenrot/riesel/pull/92), [CI 44878797167](https://github.com/aschenrot/riesel/actions/runs/44878797167), fences in research gateway and app-authored same-PID recovery | Authenticated issuer or production app exporter |
| Runtime-issued session evidence | [#95](https://github.com/aschenrot/riesel/pull/95), [CI 44886250807](https://github.com/aschenrot/riesel/actions/runs/44886250807): opt-in test feature ties a generation to the actual runtime-created token and rejects stale calls after same-PID restart | Production async admission/drain or an externally verifiable app identity; the research lock may stall on uncooperative work |
| Local principal boundary | [#96](https://github.com/aschenrot/riesel/pull/96), [CI 44886275238](https://github.com/aschenrot/riesel/actions/runs/44886275238): real Unix peer credentials and separate service UID; same-UID clients remain indistinguishable by UID-only policy | General cross-platform or hostile-same-UID authorization; secret files mode 0600 do not solve it |
| Installed process ownership | [#94](https://github.com/aschenrot/riesel/pull/94), [CI 44884335628](https://github.com/aschenrot/riesel/actions/runs/44884335628): 7/7 real systemd user-service/admission tests, actual cgroup reconciliation and stale-stop counterexamples | A production Manager, protected OS control authority, atomic registry+OS transitions, app-specific graceful stop |
| Real Riesel OS service | [#99](https://github.com/aschenrot/riesel/pull/99) adds a genuine persistent Todo under systemd experiment | Its required CI failed before test execution; this result is **pending**, not passed |
| Native process and reusable export | [#90](https://github.com/aschenrot/riesel/pull/90), [CI 44877911293](https://github.com/aschenrot/riesel/actions/runs/44877911293): real eframe/Xvfb independent GUI event loop; [#91](https://github.com/aschenrot/riesel/pull/91), [CI 44877794719](https://github.com/aschenrot/riesel/actions/runs/44877794719): policy reuse against SDK-resolved Counter and Todo | Selected toolkit, authenticated exporter, complete Todo CRUD/revision export, elimination of method-forwarding boilerplate |

The candidate shared-model approach is **preferred for review** over (A) independently duplicated product conventions and (C) mandatory control daemon. It reuses the runtime as session authority while allowing replaceable process/toolkit/transport adapters. This choice is **not yet a frozen SDK or protocol**. A mandatory service adds an operational single point of failure without established user benefit; independent adapters risk conflicting session and error meanings. Neither “minimal patch” nor “maximal framework” is an architectural argument on its own.

## 2. Proposed common semantics for #75

### 2.1 Identity and authority must not collapse

| Concept | Authority | Stability / non-equivalence |
| --- | --- | --- |
| Application definition and published logical identity | Application author / SDK contract | Stable within deliberate version policy; not a running process or security proof |
| Executable build and deployment provenance | Cargo/build tool, trusted installation or supervising operator | Artifact/config identity; a filename or application display name is insufficient |
| Manager registration | Manager's trusted registry | Stable across launches; not a runtime session or proof that an arbitrary binary is trustworthy |
| Launch attempt / OS process-owner reference | Owning foreground tool, Manager policy or OS service | One owned process attempt; PID and reused service name alone are insufficient |
| Riesel application-session generation | **Riesel runtime**, on each whole-session start or recovery | Fresh and non-reusable for that runtime session; distinct from PID, provider generation, gateway/transport lifetime; **not a bearer credential** |
| Provider instance | Runtime and selected provider owner | Distinct from entire application and published interface |
| Published application interface + contract version | App/session, using explicit author selection | Explicitly allowed operations, independently versioned; no implicit internal-provider exposure |
| Authorized client attachment / grants | Transport-specific verifier + interface admission | Bound to server, app/interface, version, runtime generation, principal and allowed operations; must be checked at dispatch |
| Operator command attempt / receipt | Authoritative process/control owner | Must identify target registration and exact launch attempt, preserve status and an *unknown outcome* when acknowledgments are lost |

No persistent registry, new schema file, wire message or public Rust type is implied by this conceptual matrix. The binding among a build, Manager registration and running app must be verified, not inferred from the same name or endpoint.

### 2.2 Observations are scoped facts, not one misleading linear “ready” state

Report each applicable independently observable fact with **subject, observer/authority, generation or launch attempt, evidence, observed-at time and freshness**; stale or absent observations are *unknown*. Examples:

- Authored definition known / validated runtime plan. Reading a verified artifact is passive; executing an app for `--print-definition` runs trusted Rust setup, and Cargo builds execute build scripts.
- OS process accepted/start/alive, plus owner reconciliation; *not* provider or application readiness.
- Required provider handshake ready; *not* app handler or exported interface readiness.
- Application handler initialized; published interface bound; **specific authenticated interface callable**, established by permitted endpoint-level evidence.
- Failed, degraded, disconnected, stopped, unauthorized and indeterminate observations remain distinguishable. Facts can be revoked and different interfaces may be at different stages simultaneously.

Never promote a socket bind, PID, port, `RIESEL_READY`, `TODO_HTTP_READY` or successful binary inspection into an authenticated callable guarantee. A read-only inspector may show weaker evidence clearly labelled rather than fabricate stronger guarantees.

### 2.3 Session fencing and lifecycle

The real whole-application session is the source of an externally conveyed fresh generation. Publication/withdrawal of the interface is coupled to runtime startup, revocation and teardown. Admission checks expected generation and currently authorized grants **before dispatch**. An attachment from an earlier generation is never transparently rebound to a new session, even when PID, address or OS process does not change. Closing a GUI does not stop an app. Revocation prevents *new admission*; operations already dispatched may finish or have uncertain side effects and must be accounted for. Research-only synchronous locking in #95 is **not** an acceptable async blocking-shutdown guarantee.

### 2.4 Trust, authentication, authorization — an explicit fork in policy

An identity assertion, version match, loopback bind, Unix socket file permission or `SO_PEERCRED` UID is not sufficient to distinguish two hostile processes running as the **same UID**. Different protections are required for:

- **Trusted local developer profile (candidate):** every same-UID participant is explicitly assumed trusted, only deliberately opt-in low-privilege interfaces are permitted, and UI/docs disclose the absence of hostile-same-UID isolation. This profile must never be described as an authorization mechanism protecting privileged Manager control.
- **Protected local control profile (candidate):** a separately protected OS principal/service or equivalent proven confinement/broker has authority for sensitive operations; the caller verifies server authority, and the server verifies caller identity and granular grants. Cross-platform equivalents require their own tests. If no defensible boundary exists, refuse protected operations rather than silently downgrade.
- **Remote profile:** separate future specification for authenticated/encrypted transport, credentials and secrets; no automatic promotion of today's plaintext loopback RPC.

**Decision still needed:** does the first installed Manager require hostile-same-UID resistance, or is an explicitly trusted single-OS-user operator the accepted initial environment? The former needs a separate protected control principal before writable Manager UI; the latter is lower assurance and must be stated in product scope. This is a product/security decision, not something #96 alone can authorize.

### 2.5 Command and mutation outcomes

Distinguish at minimum:

1. **Rejected before dispatch**: invalid version, revoked generation, absent grant, malformed request, invalid configuration. No command was admitted under the checked boundary.
2. **Definitive application result**: success or typed domain failure when authoritative completion is known. Domain failure does not automatically imply no partial effects unless that domain contract provides atomicity.
3. **Outcome unknown after possible dispatch**: timeout, lost response, disconnect, owner/controller crash or revocation while an operation may be in flight. Do **not** report “failed without effects”, silently retry mutations or claim exactly-once delivery.
4. **Observational uncertainty**: the observer cannot establish current process/interface state or receipt; status is stale/unknown, not false “stopped/ready”.

Reattach only with a new verified current generation and grants, then obtain an **authoritative snapshot or command status** appropriate to the domain. Idempotency/durable command deduplication, event subscriptions and exactly-once results need separately specified durable authority; they are not generic transport flags.

## 3. Product specifications to accept, revise or defer separately

### #68 — Cross-project developer workflow

**Proposed decision: ACCEPT the two-layer boundary** as a technical direction: a thin, optional Cargo-aware foreground orchestrator reuses Cargo artifact discovery and an SDK/runtime-defined *participation* boundary. Ordinary headless `cargo` and app-scoped CLI remain valid; no second schema, dependency resolver, process supervisor, manifest, Manager, Node or frontend is mandatory. Do **not** accept public command spelling or SDK hooks yet.

**Proposed first implementation slices, after mechanism acceptance:**

- **T-A — independent Cargo foreground tooling.** Identify exact binary through Cargo JSON compiler-artifact records; distinguish passive artifact metadata from trusted executable inspection; run a headless app as a directly owned child; show build/app/handler exit and provenance separately; stop/reap only an owned child. No claims of universal graceful or descendant shutdown. Test a Cargo workspace outside the repository, missing targets/tools, duplicate targets, early exit and interruption.
- **T-B — opt-in interface/development adapter.** Accept app-declared static-asset/frontend participation without copying Todo-specific scripts. Require explicit stable SQLite/data paths initially; an OS state-dir default and precedence are a later decision. Verify dynamic application-interface readiness before opening a browser; independent browser close cannot stop the app; prove retained data across restart. No unannounced package install, hidden migration or silent temporary default data.
- **T-C — failure/portability hardening.** Reject false/oversized/stale readiness and ambiguous duplicate starts; bound output, timeout and cleanup. Define an app-owned cross-platform graceful-stop participation mechanism rather than assuming Todo SIGINT or managed-provider stdin applies universally; Windows stays an explicit manual release gate.

**Independent progress:** T-A's Cargo artifact/build and explicitly owned foreground child can be implemented without privileged #75 export or a Manager. General *authenticated interface* verification in T-B must consume #75 semantics rather than invent a launcher token. Do not call T-A blocked by completing #59 or #60.

**Remaining acceptance questions:** packaging/commands, minimal SDK participation, portable graceful stop, data-path precedence and whether first browser asset build is only explicit opt-in. See [#88 report](https://github.com/aschenrot/riesel/blob/research/68-cross-project-developer-workflow/research/developer-workflow/REPORT.md).

### #59 — Installed Manager and process ownership

**Proposed decision: ACCEPT for further specification** a single-host Manager control contract over explicitly trusted registrations; prefer a **hybrid with exactly one selected owner per attempt**. For the *first installed Linux mode*, real systemd user transient `.service` ownership plus durable registration/attempt admission is a strong candidate; the foreground direct-child development mode is distinct and must not silently substitute when systemd is unavailable. No platform-wide process-tree cleanup or automatic PID adoption claim.

**Critical choice:** immutable **per-attempt** unit identity + durable transactional reservation is the safer current candidate than reusing one unit name and issuing a non-atomic check-then-`StopUnit` against `InvocationID`. Research observed that a stale name-only stop can kill a replacement and a unique per-attempt name prevents this *particular* error but allows duplicates unless an independent durable registration gate exists. SQLite reservation and systemd activation are **not one transaction**: preserve unresolved launch as unknown and reconcile the exact recorded attempt rather than retrying automatically. Neither invariant protects against a privileged or same-UID actor able to bypass the controller and edit registry/OS services.

**Proposed implementation slices after trust/owner specification acceptance:**

- **M-A — durable registration and admission:** validated trusted binary/config provenance, immutable launch-attempt identity, competing-controller serialized admission, revocation/unknown-state record, explicit no-duplicate semantics under the accepted actor model. No arbitrary PID adoption.
- **M-B — Linux OS ownership adapter:** actual systemd user service start/observe/stop for the recorded attempt, cgroup/process reconciliation after Manager crash, delayed-start and stop-before-start cases, unavailable user manager, grace/escalation, and a genuine Riesel Todo persistence/ready/graceful-stop test. The [#99](https://github.com/aschenrot/riesel/pull/99) test is **not yet verified**.
- **M-C — unified management contract and frontends:** one authoritative CLI/UI action model for list/inspect/start/stop, explicit command identity and unknown outcomes, truthful process versus application/interface readiness, verified authorizations at every state-changing boundary. A browser control UI must be read-only or not offered until appropriate write authority and CSRF/origin/peer/credential protections are proven; framework choice is a separate UI decision.
- **M-D — platform adapters / release evidence:** Windows job/process ownership and any macOS equivalent require platform-specific gates; no Linux proof automatically establishes parity.

**Independent progress:** Manager process start/stop and registered-attempt fencing derive authority from the trusted OS owner and Manager registry, **not** from an app's exported capability or Riesel runtime generation. Manager can report OS-owned application status while callable app-interface status remains unknown. The #75 shared identity/error vocabulary is reused, not a requirement for one protocol or daemon.

**Remaining acceptance questions:** trust profile (especially same-UID adversary), OS owner policy, registration admission across OS calls, stop/receipt reconciliation, durable command journal semantics and portable Riesel graceful stop. A Manager UI does **not** depend on a native #60 host. See [#94 report](https://github.com/aschenrot/riesel/blob/research/59-systemd-user-owner-probe/research/process-ownership/systemd-user-service-report.md).

### #60 — Native GUI and application-owned capability export

**Proposed decision: ACCEPT for further specification** independent headless app + separate GUI OS process and toolkit-neutral **deliberate, opt-in, versioned app-facing interfaces**. Favor a reusable app-owned publication/policy boundary over indefinitely duplicating facade policy, but require a full two-app comparison and real authorized session attach before approving exporter code generation, transport or public SDK syntax. A reference `eframe` GUI proves feasibility; it is not a toolkit selection.

**Proposed implementation slices after shared safety semantics acceptance:**

- **G-A — typed app-owned publish/attach proof:** existing SDK-resolved Counter and Todo *full relevant CRUD/revision* interfaces, explicit grants, verified server/client principals for selected trust profile, runtime-issued generation/revocation, callable readiness, no implicit internal-provider exports; compare generated forwarder with manually delegated facade for repeated boilerplate and domain semantics. No new requirement to use a custom UI schema or a Manager.
- **G-B — independent native adapter:** real event loop, responsive UI when backend slow/disconnected, snapshot/action/refetch, GUI exit/reopen without backend exit, backend crash/restart on same endpoint with stale-client refusal, multiple-client permission separation or explicit deferral; manual accessibility/focus and Windows checks.
- **G-C — optional subscription/event semantics only if accepted:** versioned event stream plus snapshot/resync, backpressure, freshness and authorization; no accidental subscription broker or guaranteed delivery claim.

**Independent progress:** GUI event-loop and toolkit comparative work can proceed without completing the #75 external protocol, but *production-secure publication and attach* cannot. No obligation to replace Todo HTTP or build a Riesel widget framework. See [#91 report](https://github.com/aschenrot/riesel/blob/research/60-app-capability-export/research/application-export/REPORT.md).

## 4. Dependency and decision sequence

```text
Accepted product outcomes (#75, #68, #59, #60) — already owner-selected
  ├─ #75 shared semantics: identities / lifecycle / observations / unknown outcomes
  │     ├─ app runtime-issued session + authorized exported attachment
  │     │     └─ #60 production published-capability / native-GUI attachment
  │     └─ observation & error vocabulary reused by #68, #59, #60
  ├─ #68 Cargo/foreground build and owned-child launch (independent)
  ├─ #59 OS process ownership + registered-attempt admission + Manager
  │     └─ protected Management commands: separate OS/controller authority,
  │        NOT dependent on an app-export protocol or runtime session epoch
  └─ #60 GUI event-loop / UX feasibility (independent)
```

These are **dependencies between guarantees**, not a blanket serialized project schedule. #68 tooling never owns runtime session generation; #59 Manager never resolves providers; #60 GUI never owns app domain state; #75 is not a daemon. #83 general execution scheduler remains a separate product/specification and does not alter these decisions.

## 5. Decision register — explicitly not yet accepted

| Owner gate | Recommended disposition | Acceptance blocker | What approval would authorize |
| --- | --- | --- | --- |
| #75 conceptual semantics | **ACCEPT** distinct authorities/identities, scoped observation, runtime-owned session generation, pre-dispatch fencing, unknown mutation outcomes and adapter independence as semantic requirements | Owner review of backward-compatibility and exact scope; no production security representation yet | Write a narrow architecture contract; later independently gate API and transport |
| #75 protected local authorization | **REVISE / defer concrete mechanism** | Decide trusted-user vs hostile-same-UID threat profile and verifiable server/client authority on supported OS | Research/contract for a secure adapter; **not** an authentication implementation yet |
| #68 workflow architecture | **ACCEPT** optional Cargo-aware orchestration plus SDK participation, headless plain Cargo preserved | Portable graceful stop, data-path precedence, opt-in asset handling and published participation shape | Create bounded T-A/T-B/T-C implementation Issues with relevant dependencies |
| #59 Manager owner target | **REVISE** toward immutable attempts + durable registration, first Linux OS-owned adapter | Trusted command authority, reconciliation and real Riesel #99 gate | Approve M-A/M-B/M-C only after guarantees and trust bounds are explicit |
| #60 native publication target | **ACCEPT direction; REVISE exporter mechanism** | Real principal proof, runtime epoch, full Counter/Todo mapping/forwarding comparison | Approve G-A/G-B after #75 boundaries, not general UI framework |
 
**Owner/maintainer action:** review each row against the full original product outcome, record explicit accept/revise/defer/reject on its existing specification Issue, and update the corresponding accepted architecture contract. **Then** create implementation Issues and milestones for accepted independent slices. Do not close the four specification Issues merely because research CI passed or a design is recommended.

**Current verification exception:** `main` at `439350f` has a [failed Linux Validate run 44895761588](https://github.com/aschenrot/riesel/actions/runs/44895761588) in the SQLite parent-symlink ownership test; later PR head validation is required. Independent runner-admission failures are tracked in [#100](https://github.com/aschenrot/riesel/issues/100). Neither unrelated CI state nor previously passing research runs may be presented as acceptance of new production APIs.
