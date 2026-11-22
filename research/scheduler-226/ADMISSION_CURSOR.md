# Riesel #226 — pending-prefix admission cursor differential (research only)

**Status:** unaccepted research on the existing session-owned execution runner. This document is not a production API, implementation permission, or long-term scheduling-policy decision.

## Why test this alternative?

The accepted runner admits one-shot, signal-gated and dependency-constrained operations within an actual `RunningApp` session. [#233](https://github.com/aschenrot/riesel/issues/233) removed redundant full dependency-blocking scans after successful progress. Admission still searches operations from declaration index zero on each coordinator wake. [#228](https://github.com/aschenrot/riesel/pull/228) established real-runner scan counts and CI samples, but its unmerged diagnostic path does not establish that any particular ready-index design is necessary.

A monotonic cursor is a low-complexity comparator to a full ready-node index. Once a node leaves `Pending` in an immutable one-shot plan, it cannot return. At each admission pass, move the cursor past the longest declaration prefix of non-`Pending` nodes, then use the **unchanged** eligibility/resource/control checks from that point onward. A pending node (even one blocked by a signal or resource) must stop this prefix advance. The candidate cannot skip an earlier pending node or substitute a second scheduler.

## Same-workload comparison

- Production reference: unmodified runner admission scan (A).
- Research candidate: only a private, opt-in prefix cursor (B), no other policy change.
- Actual Rust SDK `RunningApp::execution_graph`, immutable bound operations, current-thread Tokio executor, existing 32-slot session budget.
- Wide independent nodes, forward-declared dependency chains and reverse-declared dependency chains at 128, 512 and 1,024 operations. Five repeated execution samples for each shape/size in a Linux release-profile test, ordered A1 / B1 / B2 / A2. Count actual per-run admission passes and visited declaration nodes. Observe correct operation states and dispatch counts in all cases.
- An additional signal-gated node at index zero must remain pending without starving independent later nodes; a real signal later releases it.
- With B enabled, run the existing SDK `execution_runner`, `execution_domains` and `execution_nested` suites, not just the benchmark.

This is a **research-only** change to `crates/riesel_app/src/execution_runner.rs`. Private environment switches select the comparator and emit counters; they must never be exported as public/production features or merged into protected `main`.

## Expected discrimination, not assumed findings

- Wide workloads with completed leading nodes may benefit substantially.
- Forward chains may still visit a large suffix of ineligible nodes on every completion.
- Reverse-declared chains and earlier signal/resource-gated nodes may keep the cursor at zero indefinitely while other work executes.
- A full derived ready index (unmet dependency counts, reverse edges, signal subscribers) could avoid these scans but has substantially more bookkeeping and race/step correctness obligations.
- A domain-owned internal scheduler remains the authority for nested ECS/world work. Cross-run fairness and clocks are independent policy questions.

Do not accept or reject any architecture from this hypothesis alone. If evidence favors a larger index, demonstrate no changes to `GraphRunPublication::may_admit/admit` atomic safe-point semantics, shared `SessionGraphAdmission` resource/domain admission, terminal effects, session fencing, no-retry outcomes or observer truth. No benchmark is a real-time or production-latency SLA.

## Reproduction

From the repository root on the research branch:

```sh
cargo validate --check
cargo smoke
cargo test -p riesel --features async-tcp --test scheduler_admission_cursor_research -- --test-threads=1
RIESEL_RESEARCH_PENDING_CURSOR=1 cargo test -p riesel --features async-tcp --test execution_runner
RIESEL_RESEARCH_PENDING_CURSOR=1 cargo test -p riesel --features async-tcp --test execution_domains
RIESEL_RESEARCH_PENDING_CURSOR=1 cargo test -p riesel --features async-tcp --test execution_nested
RIESEL_RESEARCH_ADMISSION_COUNTERS=1 cargo test -p riesel --release --features async-tcp --test scheduler_admission_cursor_research cursor_differential_shapes -- --nocapture --test-threads=1
RIESEL_RESEARCH_ADMISSION_COUNTERS=1 RIESEL_RESEARCH_PENDING_CURSOR=1 cargo test -p riesel --release --features async-tcp --test scheduler_admission_cursor_research cursor_differential_shapes -- --nocapture --test-threads=1
```

The branch-limited GitHub Actions step repeats this comparison in A/B/B/A order for cross-order sanity. Never attribute differences to a single cause without verifying counted work and method limits. Preserve exact commit/CI metadata on [#226](https://github.com/aschenrot/riesel/issues/226); keep this draft PR unmerged.
