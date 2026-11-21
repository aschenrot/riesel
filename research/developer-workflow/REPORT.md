# Cross-project developer workflow — research and proposed specification (#68)

**Status:** Research-only; not an accepted CLI, SDK API, manifest, daemon or implementation.  
**Base:** main a35ac7c9ac9749ff023a72595c6851caee5f4859 (2026-11-21).  
**Owner direction:** [Issue #68](https://github.com/aschenrot/riesel/issues/68) records ACCEPTANCE of optional cross-project Cargo-aware developer tooling as a product outcome, **not** the command/participation mechanism.  
**Coordination:** [#75](https://github.com/aschenrot/riesel/issues/75) owns shared app/session/interface identity, authorization and readiness; [#59](https://github.com/aschenrot/riesel/issues/59) controls Manager process ownership; [#60](https://github.com/aschenrot/riesel/issues/60) controls native GUI. Independent scheduler research [#83](https://github.com/aschenrot/riesel/issues/83) is not a prerequisite.

## 1. Baseline audit and reusable evidence

- The existing [SDK application CLI](../../crates/riesel/src/application/cli.rs) already handles deployment/provider selection, definition and plan inspection, help and application-argument forwarding. Reimplementing its grammar or preflight would introduce a second authority.
- [PR #79](https://github.com/aschenrot/riesel/pull/79) tested a genuine Cargo workspace materialized outside the checkout; built via a path SDK dependency; verified that executable inspection runs user setup code but not the handler. Reuse that evidence and preserve the distinction between executable inspection and passive, provenance-validated metadata reading.
- The real [Todo HTTP host](../../examples/todo/src/bin/todo_http.rs) requires an explicit DB, binds an OS-assigned loopback port, optionally serves static assets, and emits TODO_HTTP_READY. [The server](../../examples/todo/src/http/mod.rs) awaits Ctrl-C and drains with a five-second HTTP deadline. It does not use managed-provider RIESEL_SHUTDOWN.
- [Todo HTTP integration tests](../../examples/todo/tests/http.rs) cover SQLite restart, Unix Ctrl-C, invalid assets, occupied bind, security and static serving with synthetic assets. [The browser E2E runner](../../examples/todo/web/e2e/run.mjs) covers actual Chromium and backend but uses a disposable test DB, deliberately unsuitable as a normal developer-data policy.
- [Repository Cargo aliases](../../.cargo/config.toml) and [xtask](../../tools/xtask/src/main.rs) are checkout-scoped. They are not automatically available to independent projects.
- Research [#85](https://github.com/aschenrot/riesel/pull/85) is merged. Architectural direction [#86](https://github.com/aschenrot/riesel/pull/86) is a separate draft, not a merged implementation contract at the verified baseline. [PR #87](https://github.com/aschenrot/riesel/pull/87) studies stale external bindings/uncertain mutations under #75 and should not be duplicated.

### New reproducible experiments

The files in this research directory are **not a shipped launcher**. The dedicated [Linux research workflow](../../.github/workflows/research-68.yml) explicitly installs Rust 1.100, Node 22, locked npm dependencies and Chromium before executing them. The existing Validate workflow separately runs Linux validation and smoke against the same pull-request head.

1. **Headless independent Cargo:** The runner materializes headless/ in an OS temporary directory **outside the Riesel checkout**, substitutes the checked-out SDK's absolute path, verifies Cargo workspace membership without xtask, builds offline, inspects SDK-derived definition, observes setup side effects, rejects bad args/deployment, launches in foreground, stops via this example's **app-specific** stdin command and separately sends SIGINT. This does not prove a general app-level graceful-stop protocol.
2. **Genuine persistent browser Todo:** Build static SvelteKit assets and the real Rust HTTP binary, use an explicit DB path reused across starts, verify a dynamically announced endpoint with a real HTTP request, open/close Chromium without stopping backend, create a record, SIGINT/await a graceful exit, restart and read the same record. An attempted simultaneous same-DB launch is logged as an **observation**, not silently converted into a general duplicate-session policy.
3. **Negative and false-readiness probes:** Missing DB argument, nonexistent DB parent/assets and occupied port must fail without a ready announcement; synthetic early exit, oversized output, timeout and a fake ready announcement without an HTTP listener test the runner's fail-closed behavior. A fake child is **not** evidence about the existing real app's identity protocol.

**Evidence rule:** Test code is not a passing test. Record the exact CI run/head SHA and outcome after the workflow completes. Do not infer Windows support, orphan-free cleanup, authenticated identity or tool delivery from Linux passing.

Focused reproduction from repository root (with tools explicitly installed):

    cargo validate --check
    cargo smoke
    cargo build --locked -p todo --features http --bin todo_http
    cd examples/todo/web
    npm ci --no-audit --no-fund
    npm run check && npm test && npm run build
    npx playwright install --with-deps --only-shell chromium
    cd ../../..
    node research/developer-workflow/probe.mjs

The runner deletes only its own disposable research files. Its temporary Cargo lockfile is acceptable solely for the experiment; independent published apps should commit their lockfiles. No test database path is recommended as a real app's default.

## 2. Alternative comparison against the same product outcome

Common dimensions: headless usability, independent Cargo workspaces, optional real interface, persistent state, readiness/failure correctness, cross-project boilerplate, ongoing platform ownership, dependency footprint, migration/reversibility, security and credible growth to non-Todo UIs.

| Alternative | Outcome coverage | Cost/risk | Conclusion |
| --- | --- | --- | --- |
| **A. Normal Cargo + app-scoped SDK CLI** | Already excellent for simple headless build/definition/run | Each persistent browser/native app reinvents interface start, readiness, DB policy, browser open and process cleanup | Preserve first-class baseline and escape hatch; insufficient for the accepted *shared* interface workflow |
| **B. Per-project xtask or shell script** | Can automate Todo and individual workspaces | Repeated cross-project logic, platform-specific cleanup drift, awkward later migration | Useful app adapter or short-term convenience, not platform-level cross-project solution |
| **C. Thin optional Cargo-aware tool** | One tool discovers Cargo targets and orchestrates app process and opt-in UI | Needs explicit trust, app participation/readiness and bounded ownership; wrapping cargo run alone is insufficient | **Preferred developer-facing orchestration layer**, not yet an accepted public CLI |
| **D. SDK-defined operational participation** | One shared identity/inspection/readiness semantics source usable by CLI/editor/Manager later | A public contract has compatibility and authority consequences; #75 must settle core semantics | **Preferred complement to C**, limited to actual app needs; avoid unsolicited exporter |
| **E. New universal manifest/plugin/build framework** | Could support more heterogenous build systems | Duplicated Cargo/SDK state, arbitrary script execution, greater migration and supply-chain exposure | Not justified by present outcomes; reconsider only on demonstrated adapter failure |
| **F. Manager-owned launcher** | Can coordinate retained processes eventually | Requires separate long-lived process, authorization/adoption and #59 design | Not an acceptable prerequisite for ordinary local Cargo development |

**Bias audit:** Low initial changes favoring A/B cannot erase cumulative duplication. A new named CLI or large manifest does not automatically improve longevity. C without D's verified app participation is superficial; D without shared orchestration leaves script duplication. C+D, with **clear ownership separation**, best matches the accepted long-term scope, subject to #75 semantics and negative executable evidence.

## 3. Developer experience — current versus proposed

**Current supported headless, in an independent workspace:**

    cargo run -- --print-definition     # trusted executable setup runs
    cargo run                          # app owns execution

Cargo's normal package/bin/feature flags remain authoritative. There is no application-wide generic ready or stop promise.

**Current supported persistent Todo, from the Riesel checkout:**

    (cd examples/todo/web && npm ci && npm run build)
    mkdir -p /explicit/developer-data
    cargo run --locked -p todo --features http --bin todo_http -- \
      --db /explicit/developer-data/tasks.db --port 0 \
      --assets /path/to/riesel/examples/todo/web/build

The backend announces its loopback address. Opening and closing that browser URL is independent of the backend. Repeating with the same explicit DB retains compatible state, not an automatic migration.

**Target (illustrative notation, no approved command spelling):**

    <optional-tool> inspect --package <app> --bin <bin> --execute-trusted
    <optional-tool> run --package <app> --bin <bin> --foreground
    <optional-tool> run --package todo --interface browser-static \
      --data /explicit/developer-data/tasks.db --no-open

The developer should see **Cargo building → app starting → app readiness known/unknown → interface verified/unverified → URL**. The tool must distinguish compile/preflight/handler failures, permit no-browser operation and not require Node/Manager/GUI in the headless path. Browser-open failure is nonfatal after backend readiness. There is no implicit process adoption.

## 4. Operational responsibility and authority

| Area | Authoritative owner | Optional tool responsibility and exclusion |
| --- | --- | --- |
| Package/bin/features/build | Cargo | Call Cargo/metadata; preserve errors; do not implement another build graph |
| App definition and providers | SDK/app runtime | Invoke existing inspection and pass configuration; do not duplicate schema or provider resolution |
| App session, provider generation and readiness semantics | Riesel runtime / #75 | Consume verified facts; do not invent session tokens or promote RIESEL_READY to whole-app readiness |
| Direct foreground child | Launching tool/process | Hold child handle, interrupt/reap only its child, bound stop; no unknown-PID kill or orphan-free claim |
| Persistent data and migrations | Application + Storage | Forward user-chosen stable path; never default to temp/target, silently migrate, delete data or fall back to memory |
| Public application interface | App-owned interface adapter / #75 | Verify actual endpoint and declared identity where supported; stdout+HTTP alone is not authentication |
| Frontend build | Opt-in, trusted app adapter | Run explicitly requested known steps, no hidden npm ci/download or arbitrary untrusted hook |
| Browser/native UI process | Independent UI owner | Optionally open/close; failed opener does not stop backend; no required GUI dependency |
| Retained/orphaned/Manager processes | Accepted #59 control plane when available | Not a developer CLI's automatic registry/adoption responsibility |

A graceful request must match the **actual app's** supported method. Todo uses Ctrl-C on Linux, the headless research app uses stdin, managed providers optionally use their separate stdin protocol. These must not be conflated. A launcher crash can orphan a child; process-tree cleanup is host-specific.

## 5. Required diagnostic/error model (category names illustrative)

| Situation | Required behavior |
| --- | --- |
| Missing Cargo/npm/frontend assets | Give explicit setup instructions, fail early when knowable; no surprise network installs |
| Cargo package/bin/feature error | Preserve selected target and Cargo diagnostics, do not launch a different binary |
| Binary inspection | Warn that executable setup runs; passive metadata must have independently verified provenance |
| Invalid config/SQLite schema or missing path | Preserve SDK/app cause, no temporary DB/memory fallback or silent upgrade |
| Child early exit/spawn error | Exit status/signal and bounded stderr, no fabricated readiness, owned child reaped |
| Timeout/oversized/invalid readiness | Bound wait and output, stop/reap attempted owned startup, clear not-ready result |
| False ready | Probe actual interface; remain unverified unless app/session identity and authority are known |
| Bind collision | Report app failure, prefer bind-to-zero over prebind/drop-race |
| Duplicate DB/instance | Reject or mark ambiguous without verified identity; never adopt/kill by guessed PID or port |
| Browser open failure | Keep running backend; emit URL and useful UI diagnostic |
| Graceful stop timeout | Escalate only under tested policy; record forced termination and uncertain in-flight work |
| Lost mutation reply | Outcome unknown; do not blindly retry external side effects |
| Launcher crash / escaped child | Report ownership uncertainty; cannot promise guaranteed transitive descendant cleanup |
| Credentials or external file content | Redact secrets from logs; local HTTP Host/Origin checks do not authenticate local OS clients |

## 6. Recommended implementation decomposition and acceptance

**Slice 1 — Shared participation and headless tooling:** decide minimal SDK-readable app/interface participation and how trusted inspection is authorized. Keep session generation, identity and authorization in #75; no CLI-specific public token. Then implement optional Cargo-aware foreground build/inspect/run/stop with external-workspace test. No manifest, daemon, Node or GUI prerequisite.

**Slice 2 — Optional persistent UI adapter:** explicit stable DB path, trustworthy static-build recipe and asset verification, OS-chosen port, checked interface readiness, optional browser opening and same-path restart. No source watcher or installed-user packaging in first slice. Another UI technology must be expressible by the participation boundary without copying Todo-specific orchestration.

**Slice 3 — Linux failure hardening and Windows verification:** owned child stop/reap, early exit, bounded stderr and timeouts, duplicate session ambiguity, shutdown escalation and uncertain mutations. Windows signal/control-event and process-tree behavior remain a *manual pre-release* gate, not implicitly covered by Linux CI.

Implementation-ready criteria proposed for owner acceptance:

- **T1 external Cargo:** separate workspace outside checkout, no copied xtask, normal Cargo usable, metadata vs trusted executable inspection distinguished; build/inspect/run/interrupt observable with no frontend dependencies.
- **T2 persistent browser:** explicit retained SQLite path, genuine static assets and Chromium, dynamic checked endpoint, browser-independent backend, create/stop/restart/read preserving record; reject misconfiguration without fallback.
- **T3 failures:** missing frontend dependency, invalid config/assets, early exit, timeout, malformed/oversized/false readiness, occupied port and ambiguous duplicate start are bounded and actionable. Only owned processes are affected.
- **T4 consistency and CI:** one SDK definition/provider resolver, no unaccepted public interface API, Linux cargo validate --check, cargo smoke, focused headless and browser tests on exact head; Windows manual before release.

Passing a research runner **does not** mean the unimplemented tool passes these future gates.

## 7. Still-open decisions and proposed resolution

| Question | Recommended disposition |
| --- | --- |
| Tool distribution and command spelling | Optional Cargo-discoverable wrapper; postpone spelling/SDK public signatures until participation experiment |
| Inspection | Explicitly trusted execution or provenance-checked passive artifact; #75 to settle identity |
| Data root | First slice **requires explicit persistent path**. Adopt a cross-platform OS state-dir default only after stable app identity and configuration precedence are accepted; never use cwd, temp or target silently |
| Frontend setup | Explicit opt-in static build and dependency setup, no automatic npm install, no hot reload supervisor |
| Duplicate start | Fail closed or describe ambiguity unless authenticated owner/session verification exists; SQLite lock is not universal ownership |
| Stop | Linux Todo SIGINT only where tested; require app-specific stop declaration or honest unsupported state for other apps; no managed-provider stdin assumption |
| Retained launch | Explicitly excluded from first tool; #59 owns later Manager/daemon integration |

**Research recommendation:** accept the **two-layer responsibility split** (thin optional Cargo-aware orchestrator + shared SDK/runtime operational participation), but **do not accept permanent CLI commands, public hooks, new manifests or a daemon yet**. The accepted product need is already in #68. #75 semantics must be reused where actually needed; Manager, GUI and scheduler implementation completion are not blanket blockers.

**Delivery discipline:** This PR is a reviewable research proposal. Do not merge, close #68, create implementation work, change the roadmap or milestones before explicit technical specification acceptance.
