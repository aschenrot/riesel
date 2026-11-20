# Developer application launch — investigation

**Status:** research only; no accepted new CLI, implementation authorization or release claim.  
**Issue:** [#68](https://github.com/aschenrot/riesel/issues/68) · **Reviewed:** 2026-11-20 · **Base:** `main` at `70e751c8`.  
The Issue owns the requested outcome and acceptance checklist. This document records comparative evidence and a proposed decision under the [delivery workflow](../../project/delivery.md). Shared application identity, readiness, inspection and launch semantics are investigated in [#75](https://github.com/aschenrot/riesel/issues/75) / [operational boundary research](application-operational-boundary-investigation.md), without preapproving a new daemon, export API or manifest.

## 1. Verified baseline

| Current implementation | Consequence for developer launch |
| --- | --- |
| [`Application::run`](../../../crates/riesel/src/application/builder.rs), prepared handlers, and [`run_from_env`](../../../crates/riesel/src/application/cli.rs) already execute configured apps and support `--deployment`, `--print-definition`, `--inspect-deployment`, `--inspect-catalog` and provider selection | A new global `riesel run/inspect` command **must not** duplicate the existing runtime, inspection or application argument grammar |
| [Cargo aliases](../../../.cargo/config.toml) invoke existing [`xtask`](../../../tools/xtask/src/main.rs) for validation, smoke and shipping | Reusable **repository-local** task runner exists, but no application launch task |
| [Todo HTTP binary](../../../examples/todo/src/bin/todo_http.rs) takes `--db PATH [--port PORT] [--assets DIR]`; the [server](../../../examples/todo/src/http/mod.rs) binds `127.0.0.1`, prints `TODO_HTTP_READY` after bind, supports Ctrl-C | Backend and lifecycle already work; repeated setup arguments and discovered browser URL are manual |
| [Todo SvelteKit](../../../examples/todo/web/README.md) needs dependencies and a static build; [Vite](../../../examples/todo/web/vite.config.ts) uses dynamic `TODO_API_ORIGIN` | Production-like static serving and live-reload development are **different** launch modes; choose one initially |
| [Browser E2E runner](../../../examples/todo/web/e2e/run.mjs) starts the real backend, parses readiness, sets up **temporary** SQLite and reaps the child. [Todo web CI](../../../.github/workflows/todo-web.yml) builds Rust/UI before E2E | Proves launch mechanics can be scripted, **not** a safe persistent developer launcher; temp test data cannot become normal developer data |
| [Riesel product](riesel.md) and [Todo product](../todo/todo.md) distinguish SDK, app CLI, future Manager/native host and delivered local browser R2 | Developer launch convenience is an **additional requirement**, not a reason to reclassify R2 or implement a Manager |

The [Cargo Book](https://doc.rust-lang.org/cargo/reference/config.html#alias) specifies workspace Cargo aliases; [`cargo run`](https://doc.rust-lang.org/cargo/commands/cargo-run.html) already selects package/binary/features. An `xtask` can invoke that tooling without inventing a new general execution runtime. A [Cargo build script](https://doc.rust-lang.org/cargo/reference/build-scripts.html) is not appropriate for starting long-lived apps: building source code must not itself launch a persistent service or mutate user data.

## 2. Architectural separation

There are **three distinct scopes**. For Issue #68, the **cross-project developer outcome is the subject of the specification**, rather than a speculative feature that must first be proven by multiple duplicated launchers:

1. **Application developer in an independent Cargo workspace:** wants consistent, discoverable build, launch, inspection and error behavior without writing the same orchestration per app. The product owner must decide whether this shared developer outcome is accepted; it does not itself select a CLI.
2. **Todo developer inside this repository:** needs the real browser app, persistent SQLite and reliable dynamic URL with one convenient command. This is an important integration test, **not sufficient proof of platform-wide delivery**. Workspace aliases/`xtask` cannot be inherited by a separate Cargo workspace ([Cargo configuration hierarchy](https://doc.rust-lang.org/cargo/reference/config.html#hierarchical-structure)).
3. **Installed end user/operator:** wants launch from desktop or Manager, without building source. Packaging, persistent installation, updates, process registration and background service ownership are separate decisions; see [Manager #59](https://github.com/aschenrot/riesel/issues/59) and [native host #60](https://github.com/aschenrot/riesel/issues/60).

Headless Hello/Counter and application binaries must retain ordinary `cargo run` operation without Node, a GUI, always-running Manager, new manifest, or service installation.

## 3. Alternatives using common criteria

Criteria: the proposed cross-project user outcome, reuse of SDK/Cargo semantics, first-run and repeated-use friction, foreseeable app/UI variants, configuration/discovery, operational identity/readiness semantics, maintenance and migration cost, reversibility, persistent data, startup/stop failure semantics and security. Compare all candidates against the **same** product scope; distinguish current repo convenience from platform capability.

| Candidate | Benefit | Main drawback or unknown | Assessment |
| --- | --- | --- | --- |
| **A. Existing Cargo + SDK/application CLI** | No new general tool; direct execution and inspection already work for headless apps | Multi-process UI build/port/readiness differs between projects; repeated app-specific setup | Baseline; test as a legitimate full-workflow candidate |
| **B. Workspace `xtask` / project-local script** | Minimal change for Todo, can reuse existing Cargo/npm and test orchestration | No cross-project contract, potentially replicated scripts and future migration | Useful Todo convenience, **not** a complete shared platform solution |
| **C. Thin reusable Cargo subcommand / Riesel CLI** | Consistent workflow across Cargo projects; can delegate Rust execution and inspection rather than reimplementing runtime | Needs clear project selection, optional UI build integration, stable readiness and error semantics | **Strong candidate to prototype**, but not selected on name alone |
| **D. SDK launch/inspection boundary with optional front-end adapter** | Shared Rust-side metadata and lifecycle semantics can serve CLI, IDE tools and future Manager | New exported metadata/readiness hooks may have compatibility and security costs | Evaluate as composable foundation; do not assume an entirely new public exporter is required |
| **E. Universal CLI + new application manifest/plugin engine** | May support heterogeneous project/deployment systems | Second configuration authority, extensibility/plugin security and migration complexity | Require concrete unmet needs before adding these layers |
| **F. Manager controls all starts** | Future centralized operational control | Depends on open #59 process ownership/authentication decisions; not needed for an ordinary local developer command | Do not make it a prerequisite |

**Bias review:** the original research ranked workspace `xtask` as preferred because it minimized immediate work and bundled a general CLI with a **new manifest**, making the cross-project alternative appear unnecessarily complex. That comparison was asymmetric. Conversely, a general `riesel` CLI cannot be declared necessary or future-proof solely from the product name. Compare a *thin delegating CLI* and *SDK-supported reuse* independently of a new manifest, with explicit long-term adoption/migration cost.

**Unresolved choice:** if developer ergonomics truly requires **hot reloading** rather than straightforward start, the runner additionally owns a Vite process, passes the dynamically discovered `TODO_API_ORIGIN`, retains Vite's tested same-origin proxy policy and coordinates two shutdowns. Do not implement that cost without its own acceptance criterion.

## 4. Developer-facing semantics independent of command choice

Illustrative choices such as `cargo riesel dev`, `riesel run` or a Todo-specific workspace command are **unselected**. The first implementation should establish which actions are inherent to Riesel and which are application-specific. A candidate CLI should use Cargo and existing application entrypoints, not duplicate provider orchestration. Todo's static assets are one optional interface integration, not a universal Node dependency.

1. **Preflight:** check known prerequisites and args before launching Todo. Use the existing Cargo and npm build commands, rather than a parallel Rust frontend builder. Do not automatically run `npm ci` or install dependencies from the network without a separate explicit setup action; show instructions for missing dependencies.
2. **Data:** use a specified developer-data root with an explicit path override. The same default must persist across ordinary restarts, and not depend on the caller's current working directory. **Never** use a temp E2E DB, `target/` (removed by Cargo clean), silent MemoryStore fallback or implicit SQLite migration. Choosing the exact Linux/Windows data root is an implementation/acceptance detail.
3. **Start:** build static assets and existing `todo_http`, pass `--port 0`, chosen `--db` and trusted `--assets`. Keep the child handle, read a **bounded** readiness announcement, and print exactly `http://127.0.0.1:PORT`. Do not confuse `spawn` with application readiness; include backend stderr diagnostics when it exits prematurely.
4. **UI:** optionally open the browser; if no browser is installed or it fails to open, print the URL and **keep the working backend running**. The browser does not own the backend lifecycle. No nonloopback binding, weaker Host/Origin validation, permissive CORS or automatic remote access.
5. **Stop:** remain a foreground launcher, deliver a documented graceful shutdown signal to its **owned** Rust child, wait and reap, report timeout/errors and apply only a tested platform-specific escalation policy. A launcher crash may leave a child running; parent exit **does not** imply transitive child cleanup. Do not kill unknown PIDs or adopt external processes.
6. **Repeat:** a second run against the same developer DB should preserve task state. For an existing running app, either explicitly attach under a verifiable identity/ownership policy or fail with an actionable message; do not promise automatic single-instance adoption or broad SQLite concurrency safety.
7. **Modes and growth:** static build-and-serve can be a bounded first Todo scenario; separately evaluate whether a development workflow needs source watching or hot reload. Keep the backend runtime and application business logic authoritative. The chosen architecture should allow future frontend kinds and independent project workspaces without rewriting launch ownership.

Do not introduce a new JSON deployment manifest, second process registry or GUI toolkit by assumption. A generic launcher may need an opt-in app descriptor or readiness contract, but it must be assessed against SDK-derived metadata and existing app APIs. Launching an executable to inspect it executes code; do not mistake its output for passive, authenticated discovery. Installed-user desktop launch remains a separate product slice.

## 5. Decision-critical tests for a future implementation

| Test | Required observable result |
| --- | --- |
| Initial developer start | One invocation after explicit toolchain/dependency setup builds/starts the **real** Todo backend and static UI; correct discovered URL; no manual port copying |
| Persistence | Launch twice against the same **explicit fixture DB**, create a task in the first and read it in the second; data is not lost |
| Failure safety | Invalid DB/asset path, missing npm, invalid command, bad/oversized readiness, early backend exit and startup timeout produce actionable errors with no silently orphaned owned process |
| Foreground lifecycle | Ctrl-C and normal exit stop and reap the owned child; abnormal helper death is distinguished from controlled shutdown; no claim of universal descendant cleanup |
| Browser/security | `--no-open` works without a graphical environment; browser-open failure is nonfatal; existing Host/Origin/Fetch Metadata rejection tests remain green |
| Cross-project portability | An independent Cargo workspace with a small headless Riesel app can build, inspect and start using the proposed supported workflow, without copying Riesel-repository scripts; compare authoring/configuration boilerplate with Todo |
| Evolution | Document how another frontend type, app workspace and future Manager could reuse coherent app identity, readiness and interface semantics without a second schema/runtime; identify what must change to add them |
| Isolation | Headless examples still work with normal Cargo; existing Todo unit, HTTP and Playwright tests use isolated stores and remain independent |
| CI | Linux `cargo validate --check`, `cargo smoke`, focused launcher tests and real Todo browser E2E on the exact change; Windows manual pre-release validation |

A **future focused test** can borrow the E2E runner's *readiness parsing and child cleanup technique*, not its temporary-data default. There is **no claimed running launcher or passing implementation test** in this research PR.

### Long-term coordination boundary

A developer CLI that starts an application must **not** become a second provider resolver or authoritative application-session owner. Reuse current SDK definitions and runtime preflight. Distinguish the build tool's *process launch attempt*, the app runtime's *session generation*, the provider readiness handshake and any app-owned *public interface readiness*. This same distinction matters for Manager and native GUI but does not require the developer to install or start those products.

An independent project should not copy the Riesel repository's `xtask`. Compare a thin CLI/Cargo integration, a standardized SDK-authored description and a small transport-neutral operational boundary with their **real authoring costs in both Todo and another independent application**. A shared boundary is only justified if it avoids future duplicated identity/readiness/authorization behavior or costly migration; a universal discovery daemon or new application manifest is not assumed.

## 6. Recommendation at the specification gate

**Recommended Gate B direction: first decide whether Riesel should support a coherent cross-project developer workflow, then select a reusable, evolvable architecture against its full long-term outcome, allowing greater initial investment when justified.** The most relevant technical candidates are a thin reusable CLI/Cargo integration and SDK-supported launch metadata, potentially implemented together; repository `xtask` is a narrower fallback or reference, not the selected platform design. The owner must explicitly **accept / revise / defer / reject** the scope and mechanism on [#68](https://github.com/aschenrot/riesel/issues/68). A second application is a **validation case**, not a prerequisite for recognizing the proposed platform need.

Before implementation authorization, decide what operations are genuinely shared across projects, who owns builds vs application execution, canonical discovery and readiness semantics, setup/data-root policy, browser-opening and platform-specific shutdown guarantees. Explicitly reuse or reconcile the cross-product distinctions under [#75](https://github.com/aschenrot/riesel/issues/75) rather than creating a CLI-specific application identity or readiness signal. Avoid locking CLI spelling or manifests unless they affect compatibility. If an incremental Todo-only launcher is delivered first, identify it explicitly as an **interim** integration rather than fulfillment of cross-project tooling.

Only after acceptance: update applicable product/architecture contracts, create independently verifiable implementation Issues only where boundaries require them, test in coherent PRs and adjust the roadmap *only if an accepted stage changes*. Manager #59 and native host #60 remain separate product decisions; link to #75 only for genuinely shared semantic invariants, not as a blanket prerequisite.
