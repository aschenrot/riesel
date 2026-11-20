# Riesel independent native window hosts — integration investigation

**Status: research proposal; NOT an accepted architecture, toolkit decision or implementation authorization.**  
**Specification:** [#60](https://github.com/aschenrot/riesel/issues/60) · **Evidence date:** 2026-11-20 · **Repository baseline:** `b2702907` (`main`).  
**Decision required:** maintainer accept / revise / defer / reject under [delivery.md](../../project/delivery.md). The accepted *product goal* of separate native processes already exists; the delivery mechanism below does not. Shared application identity/readiness/export semantics are being compared in [#75](https://github.com/aschenrot/riesel/issues/75) / [operational boundary research](application-operational-boundary-investigation.md); that research does not preapprove a shared exporter.

## 1. Product boundary and verified implementation

[Riesel's product definition](riesel.md) accepts an **independent native window host in a separate OS process**; closing a GUI must not automatically destroy backend application logic. It does *not* require Riesel to build a widget toolkit or screen-description language. A custom GUI framework and visual builder remain **exploratory**.

Audit of [application authoring](high-level-app.md), [API authoring](api-authoring-v3.md), [runtime lifecycle](runtime-lifecycle.md), [async execution](async-execution-contract.md), [provider catalog](provider-catalog.md) and [Todo R2](../todo/todo.md):

| Existing mechanism | Can reuse | Missing / risk |
| --- | --- | --- |
| Typed `#[capability]` and `#[component]` contracts; generated adapters | Versioned domain operations, local/managed provider binding and errors | There is **no general application-owned GUI endpoint**: `Application::handler` is not automatically an RPC service callable by an external window process |
| Managed provider host and loopback RPC | Framed requests, typed generated contracts, validation against declared provider metadata | Provider RPC is plaintext/unauthenticated; using it as a UI control plane needs new trust and export/lifecycle policy |
| `RunningApp`/session-bound work | Cancellation, ownership, typed stale-handle rejection, cooperative teardown | A GUI detached from the app has a separate process/session; backend restart does not rebind an old generated handle |
| Todo R2 app-specific Axum + SvelteKit | Proven backend-independent browser lifecycle, Rust domain authority, same-origin security boundary | Browser and app-specific HTTP are **not** a reusable native host contract or a generated front-door for arbitrary app capabilities |
| `riesel_manager::ProcessRegistry` | Child-process ownership when explicitly managed | Does not automatically own arbitrary windows; Manager [#59](https://github.com/aschenrot/riesel/issues/59) need not exist for app/GUI separation |

**Key gap:** a GUI needs an **explicit application-facing interface**. This is a product boundary, not proof that Riesel must implement a new generic `Application::expose` API. An application-specific endpoint (as in Todo), a deliberately public facade component that reuses generated capability hosting, and a new app-level exporter are **different implementation options**. Directly exposing arbitrary internal providers would bypass application ownership and authorization; a facade deliberately selected and governed by the app is not equivalent to exposing arbitrary internals.

## 2. Minimal proposed user outcome

A developer can write normal Rust domain/application code, optionally launch an independently built native GUI process, and reuse the same versioned application-facing operations without rewriting business logic. The app runs headlessly when no GUI is connected. The UI owns its event loop, view and transient presentation state; the backend remains authoritative for mutable domain state.

| Capability | First delivery candidate | Not implied |
| --- | --- | --- |
| Headless operation | App starts and works with no UI executable | GUI always required at startup |
| Native attachment | Explicit supported local connection of one independent GUI client | Remote UI discovery, implicit global bind |
| Rendering | One tested native toolkit adapter *or* reference host; toolkit remains replaceable | Riesel-owned widget framework, DOM/render protocol |
| Commands | Versioned typed app-facing capability, concrete success/error semantics; no implicit duplicate submission | Automatic retries/exactly-once side effects |
| Refresh | Initial snapshot and explicit re-read after command/reconnect | Required reactive streaming/subscriptions in the first iteration |
| Lifecycle | UI close leaves app alive; app exit/restart invalidates connection; GUI displays disconnected/reconnect state | Transparent rebinding of stale session handles |
| Interaction | A small accessible example with keyboard/focus/error feedback | Platform-wide accessibility certification or custom UI designer |

A single native GUI can be a **first verification scenario**, but may not prove a reusable Riesel native-host integration. Gate B must distinguish: (i) application-specific GUI composition, (ii) an extensible app-facing interface connection model, and (iii) a universal rendering/widget framework. The first scenario can be delivered incrementally, but the accepted **target** must address the intended platform reuse boundary and identify a credible evolution path without repeating per-app export/discovery/security boilerplate.

## 3. Architecture candidates against common criteria

| Architecture | Ownership / runtime fit | API and trust | Cost / extensibility | Initial assessment |
| --- | --- | --- | --- | --- |
| **A. Separate native GUI binary + explicit app-facing capability boundary** | GUI runs toolkit loop in its process; backend continues headlessly | Reuses versioned Rust domain contract; needs a deliberate export/connection and authorization layer | Toolkit replaceable; app-level endpoint lifecycle/identity must be specified | Plausible **target boundary**, without preselecting how it is implemented |
| **B. Generic Riesel native host consuming serialized widget/layout trees** | Renderer in separate process | New widget schema, input/events, diffing, accessibility, synchronization and graphics protocol | Could serve many apps but high maintenance cost; duplicates toolkit architecture | Defer until multiple concrete products demand it |
| **C. Embedded GUI in app process** | Straightforward direct calls; event-loop integration may compete with app runtime | No IPC boundary | Violates already accepted separate-OS-process requirement | Reject for this product milestone; may exist as external, independently scoped pattern |
| **D. Independent webview/browser GUI attached to app** | Can be a separate client/process; Todo pattern already exists | HTTP/webview bridge and browser-origin/command permission model needed | Efficient for web reuse, but is not necessarily a native toolkit experience | Viable *adapter*, not an automatic replacement for native-window-host requirement |
| **E. Native GUI directly connected to internal managed providers** | Component may be owned by app and restarted with it | Exposes internal providers to arbitrary UI, loses app-level mediation | Accidental shadow API and capability topology leak | Reject as the default application integration boundary |
| **F. App-declared public facade component** | Generated component/contract adapters already exist; app explicitly selects facade as allowed external surface | Still needs discovery, trusted client authorization, readiness/session mapping and export policy | May avoid a second schema and reduce implementation work; needs proof of reusable authoring, not just a working demo | Evaluate equally with app-owned and generic exporters |
| **G. Explicit reusable app-owned export contract** | SDK exposes only intentionally public facade capabilities, with lifecycle/identity managed by current app session | Requires new public SDK surface or adapter and explicit authorization; must not duplicate provider resolution | Higher initial cost may simplify multiple GUI/frontend projects, session versioning and later event extensions | **First-class candidate**; do not exclude simply because a smaller adapter can work |

**Lifecycle rationale:** winit's supported desktop event loop is normally created on the **main thread** and is not a general async executor; a separate GUI process lets Riesel's existing session executor run independently ([winit event loop](https://docs.rs/winit/latest/winit/event_loop/struct.EventLoop.html)). This is supporting evidence for the already accepted separation, **not** proof of complete cross-platform behavior.

## 4. Toolkit / renderer discovery (not framework selection)

Toolkit evaluation is separate from choosing a Riesel connection protocol. The ability to create a window does not imply an application-level GUI library.

| Candidate | Current relevant properties | Tradeoffs / tests required |
| --- | --- | --- |
| **winit + renderer of choice** | Portable window creation, events and native handles; deliberately **no widgets or drawing** ([winit](https://docs.rs/winit/latest/winit/)) | Most flexible, greatest rendering/input/accessibility effort; useful as host boundary only if custom rendering actually needed |
| **egui/eframe** | Rust immediate-mode widget UI with native host/event loop and web option ([eframe](https://docs.rs/eframe/latest/eframe/)) | Rapid inspector/tool prototype; validate keyboard accessibility, large-list performance, async channel integration and packaging |
| **Iced** | Rust declarative `update/view` and subscriptions ([Iced](https://docs.iced.rs/)) | Natural state/event flow; validate maturity, keyboard/assistive support, pinned Rust version and reconnect handling |
| **Slint** | Compiled declarative UI, Rust bindings, desktop/platform backends ([Slint Rust API](https://docs.slint.dev/latest/docs/rust/slint/)) | Separate .slint UI language/build step and **licensing choices** need explicit review ([license options](https://slint.dev/pricing)); validate toolchain and API boundary |
| **Tauri 2** | Desktop webview + Rust native host, multi-process webview model ([architecture](https://v2.tauri.app/concept/architecture/), [process model](https://v2.tauri.app/concept/process-model/)); frontend permission model ([capabilities](https://v2.tauri.app/security/capabilities/)) | Strong web UI reuse but JS/webview dependency and separate frontend security/IPC review; webview subprocess is **not by itself** separation of Riesel application business logic |
| **Other licensed/native binding** | GTK, Qt or platform-specific host may serve real user requirements | Packaging, licenses, OS behavior, FFI and maintenance cost require the same evidence gates |

**Finding:** host lifecycle/export semantics should be evaluated separately from toolkit choice. A dedicated GUI toolkit evaluation is **not automatically a blocker to accepting the host architecture**; choosing one adapter is a later integration gate. No toolkit uniquely satisfies the Riesel application boundary. **Propose a toolkit-neutral connection/lifecycle contract**, with one verified first adapter and a documented portability path; compare additional toolkits only if they expose a material compatibility or UX risk. Do not mandate a scored framework ranking or create one crate per renderer absent an actual ownership boundary.

## 5. Export/IPC and event delivery options

| Option | Existing fit | Trust / lifecycle | Assessment |
| --- | --- | --- | --- |
| **Explicit versioned app-facing capabilities over an approved transport** | Reuse Rust contracts and generated types instead of handwritten parallel JSON schema | Requires app-controlled export point, session-scoped binding, authorization and readiness; current provider TCP does not automatically provide it | Preferred **contract model**, pending validation of export adapter |
| Application-specific HTTP, as Todo R2 | Already exercised and convenient for browser/webview | Extra manually maintained transport/route models unless generated/adapter design is accepted; strict Host/Origin insufficient as OS-user auth | Valid simple app adapter, but not the only or automatically general path |
| Unix domain sockets (Linux), Windows named pipes | Local transport with optional filesystem/peer-identity and ACL enforcement ([Tokio UnixStream](https://docs.rs/tokio/latest/tokio/net/struct.UnixStream.html), [Windows named pipe options](https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/struct.ServerOptions.html)) | Requires OS-specific connection/security policies and separate Windows release evidence | Candidate for trusted local native-to-native IPC; not approved solely on API availability |
| Current plaintext loopback TCP | Existing generated async RPC transport, bounded framing | Metadata verification is not authentication; any local program may attempt to connect; not a secure default for sensitive GUI mutations | May be suitable only under an explicitly accepted trusted-local-client threat model |
| General event broker/subscriptions | Future incremental updates possible | Delivery order, dropped events, catch-up snapshots, backpressure, session renewal and data authority need new contracts | Defer until a measured workflow demands them |

**Proposed first protocol semantics (conceptual, not an accepted wire API):**
1. GUI requests an explicit **versioned application capability** from a known trusted application connection/launch descriptor, never an arbitrary provider catalog entry. The backend controls what it exports.
2. On connection, validate capability/version, endpoint identity and **application session generation** under the agreed trust model. Unknown contract, wrong app, stale session and unavailable endpoint fail visibly.
3. Render a **read-only snapshot** in the GUI; send user commands with application-domain revisions where relevant. Successful calls update view via reply/refetch; rejected commands present typed errors.
4. If a response is lost, its command may already have committed. Do not retry automatically. Refresh domain state and let operator/user decide.
5. On backend restart, discard session-scoped bindings and reconnect/re-negotiate with a **new generation**; never present old handles as valid. UI-only exit does not invoke backend shutdown.

**Not decided by evidence alone:** must the app own the listener, expose a dedicated component, or provide a generated adapter in its existing session? How does the native process locate/authenticate the app without handwritten ports or a general discovery daemon? One short-lived launch descriptor/token with explicit lifetime is a possible research candidate, not an accepted security model.

## 6. Lifecycle and failure traces

### Headless start and attach

- Start the Riesel application without creating any native window. Its existing dependency preflight, provider ownership and session lifecycle remain unchanged.
- An explicitly authorized exporter advertises a bounded, verifiable *app-facing* connection descriptor. Starting that exporter must be an **accepted app operation**, not an inspection/preflight side effect.
- Launch a GUI process independently (manual or explicit launcher; not assumed Riesel Manager-owned). Main thread runs the chosen toolkit loop; async RPC/IPC stays off the render thread and posts bounded UI messages.
- Connect, validate expected application/contract/session, obtain current state, display; close GUI and confirm that backend continues serving headless clients.

### Backend crash/restart

- GUI detects connection loss and moves to a disconnected state; no detached worker or UI reconnect command may continue using stale session handles.
- Riesel app failure tears down its own provider session under the [runtime recovery contract](runtime-lifecycle.md). Any opt-in whole-session restart recreates app export state; a **new** session must be validated before fresh calls.
- In-flight commands have an **unknown outcome** after disconnection until authoritative state is read. No automatic write replay and no UI claim of exactly-once semantics.

### GUI crash / reconnect

- Native GUI process can crash or exit without automatically cancelling the Riesel application. Backend may release any GUI-scoped resources and must not depend on a browser/window connection to remain alive.
- New GUI process obtains a fresh authorized descriptor, re-connects and snapshots; no assumption that local UI state persists across GUI crash.

### Unavailable/unauthorized client

- A malicious or unrelated local client is not automatically excluded by a loopback address or metadata pins. Any accepted privileged export requires specified authentication/authorization or a deliberately constrained threat model, enforced before dispatch.
- Disallowed contract/version, resource exhaustion and malformed IPC frames fail closed without executing application commands.

## 7. Verification matrix and decision-critical probes

| Gate | Evidence needed before accepting implementation as delivered |
| --- | --- |
| Product boundary | A single headless domain app works identically without a GUI; no business logic copied into widget callbacks |
| Separate process | Real distinct OS PIDs; GUI-only close/crash leaves backend alive; app exit leaves GUI able to report/disconnect |
| Developer API | Typed `RieselModel`/`#[capability]` where applicable, no duplicate manual schema or hardcoded port in sample authoring |
| Export ownership | UI can access only explicitly exported app capability; cannot implicitly call internal providers; inspect/preflight has no host/window side effects |
| Security | Wrong endpoint/app identity, stale session, unknown contract, unauthorized local client and malformed/over-budget frame rejected |
| Failures | Network drop during completed/ambiguous mutation, backend crash, opt-in session restart, GUI restart; no implicit retry/rebinding |
| Event handling | Toolkit loop not blocked by synchronous IO; bounded channel/backpressure; prompt disconnect and reliable UI dispatch |
| UI accessibility | Keyboard focus/navigation, state/error announcements and basic assistive access tested for selected renderer; claims bounded to tested platform |
| Cross-platform | Linux Rust 1.100 `cargo validate --check` and `cargo smoke`, explicit integration tests, manual Windows release validation; other OS claims only with evidence |

**Focused probes before choosing an adapter/export contract:**
1. Build a tiny two-process fixture using *one real generated capability* and a currently supported app-owned hosting path if one exists. Confirm whether a new exporter is unavoidable; record paths and failures, not a bespoke network stack.
2. Evaluate one or more representative toolkit adapters **only when the first reference app's real UX/platform constraints are known**. A second toolkit spike is justified only if the first cannot answer a decision-critical question; avoid gratuitous benchmarks or unrelated framework selection during the IPC contract gate.
3. Fault inject GUI disconnect and backend crash with a revisioned mutation; prove no silent replay and distinct session identity, including under current reference recovery.
4. Test chosen local IPC trust on Linux and explicit Windows equivalents before claiming cross-platform authorization or an authorized remote host.

### Concrete minimum contract proposal (explicit decision requested)

The following is a bounded **proposed** integration contract, not a currently supported SDK API:

- **Export is opt-in and app-owned:** application author declares an *application-facing* versioned capability contract and deliberately publishes that specific interface. Internal requirement aliases and provider instances are never automatically exposed. **Compare three mechanisms on equal terms:** (A) explicitly published facade component with existing generated bindings/host, (B) app-owned Todo-style adapter, and (C) reusable application-export API. Test developer boilerplate across two application shapes, lifecycle/security correctness, future adapters and migration costs; do **not** accept or reject C merely because A or B can technically function.
- **Listener lifecycle:** exporter starts only after successful application/provider startup, belongs to the current application session and stops with it. No socket, directory, process or GUI is created by schema generation, inspection or preflight. App start must remain headless by default.
- **Local bootstrap:** for a first trusted Linux user deployment, evaluate a runtime-generated Unix-domain socket within a private operator-owned runtime directory (for example a verified `XDG_RUNTIME_DIR` location) and an explicitly selected application identity. The independent GUI obtains a fresh bounded descriptor via a trusted launch/attach operation; **never** hardcodes a port or relies on ambient network scanning. Check permissions, aliases and peer credentials, not just a socket pathname ([Linux `unix(7)`](https://man7.org/linux/man-pages/man7/unix.7.html)).
- **Handshake and binding:** verify expected application identity, declared capability/version and current session generation; the local OS credential/access policy must authorize the peer independently of advertised metadata. `SessionToken` is an in-process cancellation object, **not** a cross-process credential or serialized session ID; a new externally represented generation/epoch requires a specified mapping and rejection policy. Wrong identity, stale descriptor or unsupported version must reject before dispatch. Same-user malicious clients and remote access are not covered unless a stronger explicit policy is later adopted.
- **GUI responsibilities:** one separate native OS process owns its toolkit, main-thread event loop, windows, rendering and transient view state. It performs transport on a worker/async scope, hands bounded messages to the UI loop, and keeps domain state authoritative in the application.
- **Failure semantics:** backend restart requires a new descriptor/session handshake; GUI-only close does not stop the app. Requests have a distinct `outcome unknown` case on lost reply, no automatic replay. First slice uses snapshot-on-connect and explicit refresh; subscriptions are deferred unless an example demonstrates a measurable requirement.
- **Platforms:** Linux acceptance first; Windows pipe ACL, process/event-loop and GUI toolkit behavior remain separate manual **pre-release** requirements if Windows support is claimed. A CI-green Linux experiment does not establish OS-local authentication parity.

**Proposed developer outcome:** a small headless domain application plus one independent GUI binary that reads and writes through one typed exported capability, without handwritten duplicate schemas, fixed ports, or shared business state. This is neither a generic window renderer nor a universal Riesel UI framework.

This design still needs a focused **two-process feasibility probe** to confirm the export path can reuse generated adapters without breaking session ownership; and one selected toolkit must pass a real window/event-loop/accessibility test before delivery. If either fails, return to #60's decision gate rather than quietly broadening the core runtime.

## 8. Dependencies, uncertainties and proposed slicing

**Decision-critical unknowns requiring specification acceptance:**
- Is a **single app-facing exported capability** sufficient for the first native application, or is a generic host registration/launch contract a required product outcome?
- Who owns and authorizes the export endpoint, how is it discovered without hardcoded ports, and what is the supported threat model? How does it differ from provider placement and ordinary app inspection?
- Which UX requires reactive subscriptions rather than explicit reload/refetch? A general broker is not justified without such evidence.
- Which OS/platform and input-accessibility obligations are release gates? A Linux smoke test does not prove Windows native-loop parity.
- How is an independent GUI launched/restarted if the Riesel Manager is not installed? Explicit developer/operator startup is a viable first-slice candidate.

**Possible implementation order after an accepted architecture:**
1. Bounded application-owned, explicitly exported **typed host-facing contract** with lifecycle/session identity and permitted local attachment; preserve preflight purity.
2. A test-only **two-process** client/host that exercises the real generated facade (or identifies exactly why it cannot work), session generation, failure, authorization and no-retry tests.
3. One **chosen** toolkit-backed independent GUI example, keyboard and accessibility review; test GUI-only exit/reconnect.
4. Optional discovery/launcher integration and expanded UI adapters **only** when separately accepted.

This order does not make a native UI host a required Stage-6 network feature, nor does it make Manager [#59](https://github.com/aschenrot/riesel/issues/59) a prerequisite. Export security *is* a real prerequisite for any sensitive UI calls. Do not create a crate merely because a product concept exists.

## 9. Critical review findings and feasibility gates

**Review outcome (2026-11-20): REVISE before Gate B.** Earlier drafts alternated between assuming a new application exporter was inevitable and requiring existing facades to demonstrably fail before considering one. **Both are selection biases.** A cross-project capability-export requirement warrants evaluation of long-term developer experience, security ownership, compatibility and adoption costs alongside immediate technical feasibility. Toolkit prototype count remains driven by actual decision-critical gaps.

**Repository evidence:** [`Context`](../../../crates/riesel/src/application/context.rs) obtains application-selected typed capability handles *by requirement alias*, but is not itself a network server. [`PreparedAsyncApplication::run`](../../../crates/riesel/src/application/async_handler.rs) executes a handler in a session-owned Tokio scope and does not expose the handler as an RPC endpoint. [`serve_router` / `run_async_tcp_rpc_host`](../../../crates/riesel_protocol/src/host.rs) host **provider/component** routes with provider readiness. Therefore a public facade component and an application-owned export bridge are not proven equivalent; they must be compared with tests around scope and identity. Provider readiness remains distinct from readiness of the application's public interface.

**Platform/transport evidence:** recent [winit event-loop documentation](https://docs.rs/winit/latest/winit/event_loop/struct.EventLoop.html) states its event loop is `!Send`/`!Sync` and offers a cross-thread `EventLoopProxy`, confirming a dedicated GUI-thread/event-loop boundary is necessary for that adapter. Linux [unix(7)](https://man7.org/linux/man-pages/man7/unix.7.html) and Rust [UnixStream peer credentials](https://doc.rust-lang.org/std/os/unix/net/struct.UnixStream.html#method.peer_cred) support OS-local identity checks where available, but **same-UID malicious processes are not excluded by peer UID**. These facts do not prove native GUI behavior, a secure bootstrap protocol, Windows pipe-ACL equivalence or a working Riesel two-process application export.

**Decision-critical probes, before implementation authorization:**

1. Build a minimal Riesel headless app with one typed public-facing facade component and an independent *headless test client*; verify that it can be explicitly exported **without exposing unrelated providers**. Compare the same test with an app-owned adapter and a bounded reusable app-export prototype if needed to measure authoring/maintenance cost, rather than using facade viability as automatic rejection of a shared contract.
2. Compare the resulting developer code and error/inspection behavior against an app-specific Todo-style endpoint. Record whether a new generic host/export API would actually remove repeated boilerplate or instead add a redundant third model.
3. Define the application generation representation across processes, bootstrap authorization, and stale/broken connection handling; verify a lost mutation response is **never replayed automatically**.
4. Only then implement **one** native window adapter and test OS process independence, event-loop responsiveness, keyboard/focus/accessibility and backend reconnect. A multi-toolkit comparison is optional if it can change the decision.
5. Keep Manager [#59](https://github.com/aschenrot/riesel/issues/59) optional. A GUI host must work with explicit independent startup/attachment; automatic discovery/installation is separate.

**Outstanding:** no real Rust two-process fixture or native GUI test has yet been executed in this investigation. Documentation and OS API availability alone are insufficient to declare the export/lifecycle design verified.

### Architecture-first addendum — reusable interfaces and evolution (2026-11-20)

**Long-term product test:** a second independently authored application should be able to expose a deliberately selected capability to another UI technology without replicating JSON schemas, lifecycle wiring, endpoint discovery, connection authorization and session/reconnect logic. Assess total cost of the repeated adapter versus a purpose-built shared boundary; neither is preselected. Todo-style HTTP is evidence of app separation, not proof that each application should handcraft its own frontend API.

**Future event-driven interfaces:** an exported capability may eventually need snapshot-plus-incremental updates and subscriptions. The initial protocol need only preserve **stable versioned interface identity**, session epochs, typed failure/unknown-outcome semantics and a path to add stream endpoints with bounded delivery. Evaluate compatibility of request/reply today with cursor/resnapshot, slow-consumer backpressure and reconnect later; **do not implement** an event bus, stream broker or exactly-once delivery in this first contract without an accepted use case.

**Cross-product alignment:** [#75](https://github.com/aschenrot/riesel/issues/75) examines identity, readiness and interface exposure used by CLI, Manager and GUI; #60 continues to own OS-process separation, event loops, native accessibility, security of GUI attachment and renderer integration. The GUI must work without a persistent Manager. Record which invariants truly depend on shared acceptance and which can be tested independently.

## 10. Proposed Gate B outcome — pending owner review

**Recommend REVISE before unconditional Gate B acceptance.** Preserve the separate-process native GUI target, but compare an explicit facade component, app-owned adapter and reusable exporter against **the intended multi-application integration outcome** rather than technical viability alone. A coherent reusable export boundary may justify higher initial investment. **Do not infer** a universal window host, widget protocol, renderer, event broker or particular toolkit from that decision.

Acceptance must be **conditional** on resolving the exported endpoint's authority/identity, the trust/IPC contract, toolkit/platform verification expectations, and the first example's observable user experience. If those questions remain material, keep [#60](https://github.com/aschenrot/riesel/issues/60) open rather than treating the proposal as acceptance.

The maintainer records **accept / revise / defer / reject** with evidence. Only then update [riesel.md](riesel.md), durable architecture documentation and [implementation-roadmap.md](implementation-roadmap.md), create implementation Issues and optionally a GitHub Milestone. This draft is not such authorization.
