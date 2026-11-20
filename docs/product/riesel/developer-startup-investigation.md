# Developer application startup — bounded investigation

**Status:** research proposal; **not** accepted architecture, implementation authorization, or a released `riesel` CLI.  
**Specification:** [#68](https://github.com/aschenrot/riesel/issues/68) · **Reviewed:** 2026-11-20 · **Git baseline:** `main` at `70e751c8` for this research branch.  
**Decision owner:** project maintainer, under the [delivery model](../../project/delivery.md). Research evidence can be merged without accepting the implementation.

## 1. Product outcome and correction

**Problem:** a developer using Todo R2 should not need to copy the backend's ephemeral port, manually select a SQLite path on each run, build frontend assets in a separate terminal, and then open the browser manually every time. At the same time, Riesel applications that are naturally headless must stay runnable as ordinary Rust binaries, without a frontend toolchain or always-running Manager.

**Correction to the initial proposal:** `riesel dev`, `riesel run` and `riesel inspect` are only suggested command names, **not accepted APIs**. Riesel already has a runtime and application-scoped CLI. A new cross-project tool, manifest format or runtime supervisor is a material architectural change that needs evidence beyond one Todo example.

Scope two distinct experiences:

| Experience | User outcome | Scope status |
| --- | --- | --- |
| **Local developer launch** | One command after documented toolchain/dependency setup: prepare, start and observe Todo's existing Rust app and web assets, with stable developer data and clear exit | **This investigation** |
| **Installed end-user launch** | Launch from desktop/app menu without Cargo, npm or a checked-out source repository; package dependencies and store data safely | **Separate future product requirement**, not included in Todo R2 or this scope |
| **Manager administration** | Register, inspect and control multiple independently installed applications, including those not yet running | [Manager #59](https://github.com/aschenrot/riesel/issues/59); neither required nor preapproved here |
| **Independent native GUI** | Attach a separate native OS-process interface to headless backend | [Native host #60](https://github.com/aschenrot/riesel/issues/60); not required for Todo's existing browser UI |

Existing [Riesel product](riesel.md) and [Todo product](../todo/todo.md) definitions are authoritative. Delivered Todo R2 remains delivered in its accepted reference scope; this investigation proposes **additional startup ergonomics**, not a retroactive reclassification of R2.

## 2. Verified implementation evidence

| Area | Observed implementation | Gap for normal developer workflow |
| --- | --- | --- |
| SDK lifecycle | [`Application::run`](../../../crates/riesel/src/application/builder.rs), prepared handlers and [application CLI](../../../crates/riesel/src/application/cli.rs) already handle application dependencies, selected providers, `--print-definition`, `--inspect-deployment`, `--inspect-catalog`, `--deployment` and explicit argument forwarding | No workspace-wide builder/launcher; a separate `riesel run` could redundantly wrap existing `cargo run` |
| Existing development tooling | [`.cargo/config.toml`](../../../.cargo/config.toml) aliases `validate`, `smoke`, `ship` to [`xtask`](../../../tools/xtask/src/main.rs). `xtask` already invokes Cargo and owns workspace root detection | No `launch`/Todo convenience command |
| Todo backend | [`todo_http`](../../../examples/todo/src/bin/todo_http.rs) accepts `--db PATH [--port PORT] [--assets DIR]`; [host](../../../examples/todo/src/http/mod.rs) binds loopback, emits `TODO_HTTP_READY 127.0.0.1:PORT`, handles Ctrl-C gracefully | Repeated manual arguments, build coordination and browser open |
| Todo static UI | [SvelteKit workspace](../../../examples/todo/web/README.md) requires `npm ci`, `npm run build`; Rust serves static `build/` with API same-origin | Initial dependency setup and asset build are separate actions |
| Vite hot reload | [Vite config](../../../examples/todo/web/vite.config.ts) accepts `TODO_API_ORIGIN`; local security gate rewrites same-origin proxy requests | Dynamic backend port must be conveyed to Vite; editing/restarting Vite state is not an application-runtime concern |
| Tested orchestration | [Playwright launcher](../../../examples/todo/web/e2e/run.mjs) spawns `todo_http`, parses readiness and terminates child; [Todo web CI](../../../.github/workflows/todo-web.yml) builds Rust and static UI before E2E | Uses **ephemeral SQLite in tempdir**; unsuitable as normal persistent development data, and the Node runner is test-only |
| Repository data hygiene | [`.gitignore`](../../../.gitignore) excludes Cargo output and editor artifacts; no accepted persistent developer-data directory convention | A new default data path requires explicit cleanup/retention policy; `target/` must **not** be the default for persistent tasks |

The **Cargo Book** documents [workspace Cargo aliases](https://doc.rust-lang.org/cargo/reference/config.html#alias) and [`cargo run`'s existing package/bin/features targeting](https://doc.rust-lang.org/cargo/commands/cargo-run.html). These can improve invoking a workspace helper without introducing a separate `cargo-riesel` installation. Cargo's [external subcommand support](https://doc.rust-lang.org/cargo/reference/external-tools.html) exists, but by itself does not supply Riesel process management or app discovery.

## 3. Requirements that actually affect the choice

**R1 — Bounded developer execution.** From a checked-out workspace with prerequisites installed, one documented command prepares the existing Todo frontend/backend, starts exactly one local backend, observes the runtime's readiness announcement and prints a clickable correct `http://127.0.0.1:PORT`. It optionally opens the default browser but must work on headless Linux without an installed browser. Do not hardcode a port, infer readiness from process spawn or bind external interfaces.

**R2 — Persistent and explicit state.** A normal repeated development launch reopens the **same SQLite database** unless the developer overrides its path. The default location should be an agreed user-data or deliberately configured developer-data directory, **not a temporary test directory or Cargo `target/`**. Do not silently overwrite existing data, fall back to in-memory Storage, run migrations or change the owner of a file. Honor explicit `--db` or documented config override, with path errors visible.

**R3 — Single ownership.** The foreground launcher owns the one Todo process it starts; on Ctrl-C it requests the existing graceful shutdown, waits a bounded interval and reports escalation/failure. On failed frontend build, invalid DB path, bind error, missing readiness or application exit, do not leave a second unmanaged backend. Abrupt launcher death **cannot** be claimed to guarantee transitive OS process cleanup without a tested containment mechanism. Stop semantics are only as strong as the actual OS process tree controls; any exceptional leftovers are observable.

**R4 — Developer ergonomics with correct security.** Distinguish `run` (build and serve a current static frontend) from optional `dev-watch` (Vite live reload, two supervised child processes) if both are justified. The first slice needs **one reliable mode**, not hot reload, a new JS frontend builder, or an always-on service. Preserve Todo's exact Host/Origin/Fetch Metadata checks; the runner must not create a permissive proxy or expose service beyond loopback. A UI/browser close must not kill the headless backend automatically.

**R5 — Preserve platform API.** Any generic launcher must re-use SDK entrypoints and definitions rather than duplicating `Application::run_from_env`, inspection flags, provider resolution, or the domain's task logic. It must not infer arbitrary binaries as safe to execute merely because Cargo metadata lists them. An application with no UI should continue to run with plain `cargo run` and existing SDK CLI, without installing Node.

**R6 — Developer error contracts.** Invalid arguments, missing `npm`, missing node dependencies, frontend compile failure, backend premature exit, readiness timeout, occupied resources, browser-open failure, concurrent instance and Ctrl-C each have a documented observable result. Browser-open failure should display the URL but not destroy a healthy backend. Failures have nonzero exit status where applicable and do not leak secrets. Avoid automatic network dependency installation unless the explicit operation documents and authorizes it.

## 4. Alternatives assessed on common criteria

| Candidate | One-command Todo | Reuse / authoring overhead | Operational / security consequences | Recommendation |
| --- | --- | --- | --- | --- |
| **A — README and Cargo commands only** | No: still requires npm/build/backend/browser coordination | Smallest changes; no additional code | Manual error/port/data orchestration, hard to test consistently | Keep as escape hatch, not target developer UX |
| **B — Workspace `xtask` runner + optional Cargo alias** | Yes: can call existing Cargo and npm, parse bounded readiness, manage one child, print URL | Reuses existing tooling and one repository root; no new Riesel public API or manifest | Must implement foreground cancellation, platform errors, stable data choice; not an installed application launcher | **Preferred first slice** for existing repository |
| **C — Todo-specific launcher binary** | Yes; close to application packaging | Can place defaults near app but risks mixing Node/frontend build and source paths into product runtime | Installation semantics, data path and source-tree dependencies become entangled; still needs UI asset packaging | Consider for independently distributed Todo, not first repository developer mode |
| **D — New general `riesel dev/run/inspect` CLI + manifest** | Potentially | New cross-project discovery, names, configuration and duplication of existing SDK CLI; large API surface for one example | More behavior to secure, version and maintain; requires at least Todo and headless Counter/Hello evidence | **Defer** until two substantively different apps prove `xtask` inadequate |
| **E — Manager-controlled app launch** | Potentially | Reuses eventual operator control but introduces #59 ownership and authorization dependencies | A Manager that itself needs bootstrapping cannot be the basic developer-launch prerequisite | **Reject as default prerequisite** |

**Recommendation is conditional, not approval:** choose a **workspace-scoped, foreground `xtask` Todo launch task** as the minimal first implementation. A short Cargo alias may be added, but the alias must be explicitly selected only after confirming consistent command ergonomics from workspace root. **This does not deliver the generalized developer-startup experience for a third-party Riesel application**; Cargo aliases and repository `xtask` code belong to the current workspace, not every downstream app's checkout. Do not claim a universal `riesel dev` program exists. If independently authored applications prove repeated tooling needs, evaluate a reusable SDK-facing/CLI entrypoint against the simpler `cargo run` and explicit app-level configuration.

**Why not a Cargo build script?** An ordinary Cargo build script is executed as part of building a crate, not an interactive persistent app launcher; mixing application startup and durable data creation with Rust compile-time steps violates a clear side-effect boundary ([Cargo build script contract](https://doc.rust-lang.org/cargo/reference/build-scripts.html)). An `xtask` is an explicit operation invoked by the developer instead.

### Cross-repository developer experience is a separate acceptance boundary

A contributor starting Todo **inside `aschenrot/riesel`** can use that workspace's Cargo aliases and `xtask`. A developer building `my_app` in **another Cargo workspace** does not inherit this repository's alias, runner, Todo-specific binary or Node layout. The initial one-command Todo result therefore proves a *repository-level* development convenience, **not** a product-wide “Riesel apps start with one command” capability. The [Cargo workspace configuration rules](https://doc.rust-lang.org/cargo/reference/config.html#hierarchical-structure) support this distinction.

For a later reusable tooling decision, construct an independent miniature Rust application *outside the Riesel workspace*, using published/path Riesel SDK APIs and its normal Cargo metadata. Measure how much manual startup plumbing remains after normal `cargo run`; only if it is repeated across at least two real application shapes should a generic launcher, app manifest or `riesel dev` API be accepted. Ordinary headless apps may not need a dedicated launcher at all.

This is a **separate future generalization gate**. Do not expand the first `xtask` Todo implementation into a general discovery/installer tool simply to claim cross-project coverage. Conversely, do not close a broad developer-experience claim on the strength of a Todo-only shortcut.

## 5. Proposed first operator/developer contract for review

**Illustrative command, not accepted naming:** `cargo todo-web` → an alias invoking `xtask`'s Todo launch action. A direct invocation such as `cargo run --package xtask -- todo-web` may remain supported to avoid alias lock-in. No new package or app manifest should be necessary.

Proposed bounded behavior:

1. Validate arguments and availability of the expected pinned Rust and supported Node/npm toolchains; no backend process or DB mutation before validation. For missing JavaScript dependencies either emit an actionable `npm ci` setup instruction **or** provide a separately explicit `--setup` action. Do not silently download dependencies on a normal run.
2. Build the SvelteKit static app and the existing `todo_http` Cargo target, reusing current build outputs. Preflight the intended persistent DB path and trusted asset directory; avoid writing or resolving data paths relative to the arbitrary caller working directory. Do not use a Playwright temp DB.
3. Choose `--port 0`, spawn the Rust backend with configured DB path and static assets and retain the real process handle. Read a **bounded** readiness line, reject malformed/unexpected announcements and early exit, distinguish TCP readiness from browser connectivity and avoid losing child stderr diagnostics.
4. Print the exact local URL and stop instructions; optionally attempt browser-open through an explicitly selected OS integration without forcing browser presence. The browser is **not** a child whose lifetime controls the application.
5. Remain attached as a foreground launcher. Ctrl-C requests the already implemented Todo HTTP graceful shutdown; wait for process exit, report failures/timeout, and apply a documented platform-tested escalation policy. Never assume a kill signal on the parent implies all descendants were cleaned up.
6. Repeated run with the same configured data root must retain tasks; an incompatible persisted model fails closed and tells the operator what to do. Do not invoke offline migration as a convenience side effect.
7. Make duplicate-application protection narrowly scoped: if a previous app holds the selected DB/endpoint and cannot be safely attached, fail clearly rather than killing unknown PIDs or silently starting a duplicate. The **actual** SQLite concurrency semantics and process lock policy are implementation/test decisions, not facts inferred from an open DB.

**No-launch options:** exact flags and configuration file are **not** selected here. A simple `--db PATH` override and `--no-open` option are illustrative. Reject unknown flags, avoid shell-quoted argument injection, preserve `OsString` paths and do not introduce a second global Riesel argument grammar without need.

**Separation of run/watch:** one-command *static frontend build + backend serve* is sufficient to close the initial developer-launch outcome. Vite HMR requires coordinating a **second process** with dynamic `TODO_API_ORIGIN`, fail-fast startup, same-origin proxy security and shared shutdown; treat as a separate follow-on only if developers need a live reload experience. Calling a nonwatching command `dev` may mislead developers; select command naming accordingly.

## 6. Failure traces / verification gates

| Case | Expected evidence |
| --- | --- |
| First valid run | One invocation after explicit toolchain+dependency setup builds UI/backend, obtains OS-assigned port and prints a usable URL; no hardcoded port or extra terminal |
| Repeated run | Task created in run 1 is visible in run 2; DB location stable and documented; no missing-data fallback |
| Headless | Same run with browser-open disabled succeeds in CI; Hello/Counter continue with plain Cargo, no Node runtime |
| Missing setup | Missing npm, modules, assets, unavailable browser and incompatible toolchain each produce distinct useful errors; no surviving unexpected Todo process |
| Backend error | Invalid/unwritable DB, backend exit before readiness, malformed/oversized readiness and bind failure are reported without an orphaned owned child |
| Shutdown | Ctrl-C, normal exit and forced termination are distinguished; launcher-owned app is signalled, waited and reaped; no automatic mutation retries |
| Browser security | Existing host/origin/Fetch Metadata rejections still hold when launched through helper; browser-open failure preserves healthy backend |
| Concurrency | Two launchers targeting same default data path do not claim exclusive ownership without evidence; either clearly reject or specify safe, tested sharing |
| Isolation | `npm test`, `npm run build`, `npm run test:e2e` and backend integration remain runnable independently; test temp DB remains isolated |
| Quality gates | Linux `cargo validate --check`, `cargo smoke`, dedicated launch integration and Todo browser E2E; Windows manual release validation; exact commit recorded |

A **future test fixture** should invoke the launch command with a disposable *explicit* DB override, capture readiness and interrupt it without opening a real browser, then run again against the same DB. This verifies persistence without contaminating a developer's default data directory. The fixture is not claimed to exist.

## 7. Decision and delivery boundary

**Research recommendation for Gate B: ACCEPT a *bounded Todo-in-Riesel-workspace developer-launch outcome*, but do not yet approve a full platform CLI or claim downstream projects are served.** Choose existing `xtask` plus a workspace Cargo alias as the initial implementation candidate, with one-command Todo static UI+backend and stable explicit data semantics. A later generalized app-developer workflow needs separate cross-repository evidence. This is a recommendation awaiting the accountable maintainer's **accept / revise / defer / reject** decision; documentation is not proof of a working command.

**Resolved for planning if accepted:** no Manager dependency, no new process supervisor, no generic manifest, no native UI dependency, no auto migration, no remote bind, no downloaded dependencies without explicit setup, no claim that source-code launch equals installed-user packaging.

**Not resolved without prototype evidence:** command spelling/flags, platform-specific DB root, browser opener, behavior after launcher crash, duplicate instance policy and exact Windows signal/cleanup semantics. These can be implementation details **only if** the owner explicitly accepts the limited guaranteed behavior and Linux-first acceptance, with Windows manual gate. If guarantees are materially stronger, investigate and test before authorization.

**Next workflow steps, only after decision:** amend [Riesel product](riesel.md) for accepted developer workflow (and Todo product only if adding accepted Todo-specific outcome), specify small durable boundaries where needed, open **one bounded implementation Issue** for the launch task, add tests and docs in a coherent PR, and update [implementation-roadmap.md](implementation-roadmap.md) only if a real accepted stage changes. A GitHub Milestone is optional and should not be created merely to label a single small task.

[Manager #59](https://github.com/aschenrot/riesel/issues/59) and [native host #60](https://github.com/aschenrot/riesel/issues/60) remain independently gated. **Research status is not implementation progress.**
