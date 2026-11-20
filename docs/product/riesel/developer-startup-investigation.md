# Developer application launch — investigation

**Status:** research only; no accepted new CLI, implementation authorization or release claim.  
**Issue:** [#68](https://github.com/aschenrot/riesel/issues/68) · **Reviewed:** 2026-11-20 · **Base:** `main` at `70e751c8`.  
The Issue owns the requested outcome and acceptance checklist. This document records comparative evidence and a proposed decision under the [delivery workflow](../../project/delivery.md).

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

There are **three different startup products**:

1. **Developer inside this Riesel repository:** wants one command to prepare and launch the existing Todo R2 example, connect the browser and stop cleanly. This is the initial, bounded outcome.
2. **Independent Riesel developer in another Cargo workspace:** wants similarly low friction for their own application. That developer **does not inherit** this repository's `xtask`, Cargo alias or Todo-specific Node layout. A universal `riesel dev/run` command requires independent cross-workspace evidence; a Todo shortcut does not meet this wider outcome ([Cargo config hierarchy](https://doc.rust-lang.org/cargo/reference/config.html#hierarchical-structure)).
3. **Installed end user/operator:** wants launch from desktop or Manager, without building source. Packaging, persistent installation, updates, process registration and background service ownership are separate decisions; see [Manager #59](https://github.com/aschenrot/riesel/issues/59) and [native host #60](https://github.com/aschenrot/riesel/issues/60).

Headless Hello/Counter and application binaries must retain ordinary `cargo run` operation without Node, a GUI, always-running Manager, new manifest, or service installation.

## 3. Alternatives using common criteria

Criteria: one-command Todo use, existing SDK/Cargo reuse, cross-project value, maintenance cost, persistent data safety, startup/stop failure semantics and security.

| Candidate | Benefit | Main drawback | Disposition |
| --- | --- | --- | --- |
| **A. README + current Cargo commands** | Zero new code, debuggable steps | Still manual frontend build, dynamic port handling, browser opening, shutdown coordination | Keep as fallback, fails one-command outcome |
| **B. Existing workspace `xtask` + optional Cargo alias** | Uses installed Cargo/npm and existing repo structure; easy to test, no new public SDK API | Only solves **this workspace**; child ownership, setup, persistent path, platform signals need care | **Preferred first Todo slice** |
| **C. Todo-owned launcher executable** | Can eventually support distributed package | Couples production startup to npm/build/source tree unless packaging model changes; app-specific duplication | Defer until installed Todo requirements |
| **D. General `riesel dev/run/inspect` CLI and manifest** | Potentially reusable across independent projects | Needs project discovery, portable config/identity and versioning; duplicates current SDK flags unless proven useful | **Do not preselect**; assess with a real independent app |
| **E. Manager controls all starts** | Future centralized control | Depends on unresolved process-tree ownership, auth and Manager startup in [#59](https://github.com/aschenrot/riesel/issues/59) | Reject as a prerequisite |

**Review finding:** the initial proposal to create `riesel dev`, `riesel run`, `riesel inspect` and a possible new CLI crate was over-scoped. The minimum missing behavior is **local developer process orchestration around an existing application**, not reimplementing provider supervision or capability binding.

**Unresolved choice:** if developer ergonomics truly requires **hot reloading** rather than straightforward start, the runner additionally owns a Vite process, passes the dynamically discovered `TODO_API_ORIGIN`, retains Vite's tested same-origin proxy policy and coordinates two shutdowns. Do not implement that cost without its own acceptance criterion.

## 4. Proposed bounded first-delivery behavior

*Illustrative command only:* `cargo todo-web` as a repository alias of an `xtask` action (or direct `cargo run -p xtask -- todo-web`). Neither spelling nor implementation is accepted.

1. **Preflight:** check known prerequisites and args before launching Todo. Use the existing Cargo and npm build commands, rather than a parallel Rust frontend builder. Do not automatically run `npm ci` or install dependencies from the network without a separate explicit setup action; show instructions for missing dependencies.
2. **Data:** use a specified developer-data root with an explicit path override. The same default must persist across ordinary restarts, and not depend on the caller's current working directory. **Never** use a temp E2E DB, `target/` (removed by Cargo clean), silent MemoryStore fallback or implicit SQLite migration. Choosing the exact Linux/Windows data root is an implementation/acceptance detail.
3. **Start:** build static assets and existing `todo_http`, pass `--port 0`, chosen `--db` and trusted `--assets`. Keep the child handle, read a **bounded** readiness announcement, and print exactly `http://127.0.0.1:PORT`. Do not confuse `spawn` with application readiness; include backend stderr diagnostics when it exits prematurely.
4. **UI:** optionally open the browser; if no browser is installed or it fails to open, print the URL and **keep the working backend running**. The browser does not own the backend lifecycle. No nonloopback binding, weaker Host/Origin validation, permissive CORS or automatic remote access.
5. **Stop:** remain a foreground launcher, deliver a documented graceful shutdown signal to its **owned** Rust child, wait and reap, report timeout/errors and apply only a tested platform-specific escalation policy. A launcher crash may leave a child running; parent exit **does not** imply transitive child cleanup. Do not kill unknown PIDs or adopt external processes.
6. **Repeat:** a second run against the same developer DB should preserve task state. For an existing running app, either explicitly attach under a verifiable identity/ownership policy or fail with an actionable message; do not promise automatic single-instance adoption or broad SQLite concurrency safety.
7. **Mode:** initial acceptance is **build-and-serve**, not an automatic HMR/watcher. Keep all app-specific business logic in the existing Todo domain and preserve application-scoped CLI/inspection behavior.

No new JSON deployment manifest, general process registry or independent GUI is required for this bounded workflow. An installed-user desktop launcher is a separate product slice; the developer tool cannot be presented as its substitute.

## 5. Decision-critical tests for a future implementation

| Test | Required observable result |
| --- | --- |
| Initial developer start | One invocation after explicit toolchain/dependency setup builds/starts the **real** Todo backend and static UI; correct discovered URL; no manual port copying |
| Persistence | Launch twice against the same **explicit fixture DB**, create a task in the first and read it in the second; data is not lost |
| Failure safety | Invalid DB/asset path, missing npm, invalid command, bad/oversized readiness, early backend exit and startup timeout produce actionable errors with no silently orphaned owned process |
| Foreground lifecycle | Ctrl-C and normal exit stop and reap the owned child; abnormal helper death is distinguished from controlled shutdown; no claim of universal descendant cleanup |
| Browser/security | `--no-open` works without a graphical environment; browser-open failure is nonfatal; existing Host/Origin/Fetch Metadata rejection tests remain green |
| Isolation | Headless examples still work with normal Cargo; existing Todo unit, HTTP and Playwright tests use isolated stores and remain independent |
| CI | Linux `cargo validate --check`, `cargo smoke`, focused launcher tests and real Todo browser E2E on the exact change; Windows manual pre-release validation |

A **future focused test** can borrow the E2E runner's *readiness parsing and child cleanup technique*, not its temporary-data default. There is **no claimed running launcher or passing implementation test** in this research PR.

## 6. Recommendation at the specification gate

**Recommend accepting a Todo-in-this-workspace launch outcome with minimal `xtask` implementation, while deferring a general Riesel CLI until an independent application demonstrates reusable needs.** Do not treat either outcome as accepted without the maintainer's explicit **accept / revise / defer / reject** decision on [#68](https://github.com/aschenrot/riesel/issues/68).

Before implementation authorization, the decision must fix the promised first-run/setup behavior, supported data-root semantics, browser-opening policy and the **limited** process shutdown guarantee; the exact command name and minor parser design can be left to implementation. If a broader cross-project capability is required immediately, **revise this scope rather than mislabel a workspace alias as general tooling**.

Only after acceptance: update applicable product/architecture contracts, create a single independently verifiable implementation Issue, test in a coherent PR and adjust the roadmap *only if an accepted stage changes*. Leave #59/#60 independent and do not create a milestone merely to schedule a helper.
