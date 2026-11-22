# Riesel #226 — signal-activation readiness index (research only)

## Why this experiment exists

The verified dependency-ready index in draft PR #239 reduces the number of candidates scanned for large dependency graphs but still revisits **pending nodes whose prerequisites are satisfied and whose signal has not been fired**. One real 320-operation workload (256 gated nodes followed by 64 independent operations) required 16,960 admission visits with dependency indexing, despite only 320 actual callbacks. This is not a correctness failure; it is a remaining selection-cost case.

This research builds on the exact tested #239 comparator and separates two concepts:
1. **Dependencies met:** prerequisite completion admits eligibility to progress.
2. **Signal released:** a declared one-shot signal has actually fired; this is a persistent coalesced boolean within the run, **not** a counted event or recurring trigger.

## Research candidate

An immutable signal-subscriber index is derived at runner creation from operation declarations. When `RIESEL_RESEARCH_SIGNAL_ACTIVATION=1`, only dependency-ready nodes with immediate triggers enter the sorted candidate set initially. A fired declared signal activates matching pending subscribers whose dependency counts have reached zero. If a signal fired **before** the last prerequisite completed, that child becomes eligible when its final prerequisite completes, using the already-fired signal. Duplicate signals remain idempotent; they never enqueue duplicate attempts.

Every candidate still crosses the **existing** resource/domain admission, atomic publication-safe-point control, session-generation and effect-reporting checks. Declaration-index ordering is retained, and the original `GraphSignalSender` bounded validation/channel ownership is unchanged. No new queue, timer, fairness, remote authorization or ECS inner scheduler is introduced.

## Proof and comparison

Compare actual production `RunningApp::execution_graph` behavior in a common Rust test binary:
- Baseline dependency-ready index: `RIESEL_RESEARCH_READY_INDEX=1`.
- Additional signal activation: `RIESEL_RESEARCH_SIGNAL_ACTIVATION=1`.
- Original declaration scan with no flag for semantic reference.

Run one real-runner test of 256 signal-gated + 64 independent nodes with no signals initially, then release one signal after independent completion. Compare A1/B1/B2/A2 admission visits in optimized builds, and check no early gated invocation. Re-run the 256-writer resource-conflict case to demonstrate that signal activation **does not** solve independent resource admission contention. Repeat wide/forward/reverse DAG cases to identify regression overhead. Verify a signal fired twice while a prerequisite is still executing, with the child invoking only once on subsequent completion.

Run the SDK `execution_runner`, `execution_domains`, `execution_nested` acceptance suites with the candidate flag; always run `cargo validate --check` and `cargo smoke` on the exact PR candidate. Record raw diagnostics, CI IDs, and the latest `main` relationship in #226. These are **research-only** environment switches and counters, not a published scheduler policy.

## Limits and compatibility risks

- The candidate adds per-run subscriber metadata and activation bookkeeping. Repeated short simulation ticks may justify sharing immutable subscriber maps across exact immutable plan revisions, rather than rebuilding them for every run.
- A large set of resource-conflicting or paused/delegated candidates may still be inspected on every admission-release or safe-point wake. A resource-specific wait strategy is a **separate** investigation; it cannot invent a second `SessionGraphAdmission` authority or silently select FIFO fairness/priority.
- No guarantees about service jitter, latency deadlines, hard-real-time frame budget, starvation, cross-process authorization, persistent effects or durable retries are inferred.
- Do not merge any research source code from this draft PR to production `main`. Specification #226 remains open until the owner accepts a coherent outcome across the full intended workload scope.
