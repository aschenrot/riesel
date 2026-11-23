# Riesel #226 — instrumented cross-run successful-admission witness

**Status:** research-only. No B1 fairness mechanism, product acceptance, supported public API, credential or remote observation authority. Based on protected `main` `2b9b0b69c98345123988c1ed67e7d11abbd19365`, after the accepted A1 documentation correction #267 merged. The normal production build does not enable this probe.

## Why a separate measurement is required

Previous [real-runner multi-worker measurements](https://github.com/aschenrot/riesel/issues/226#issuecomment-7091495562) showed the long 64-reader graph's cold-writer **callback** began 65th/65 on current-thread Tokio but 2nd–5th/65 on four workers. The resource-free 32-slot contention case began the cold callback 65th/65 versus 33rd/65. Those counters live inside spawned callbacks, *after* the runner grants the session reservation and the authoritative `GraphRunPublication::admit` safe point. Such ranks **cannot prove admission order or any bounded fairness contract**.

The existing `GraphRunObserver` reports exact run-local running attempts and a bounded **per-run** transition sequence. Independent per-run sequence numbers cannot be compared as one session-global chronology. Altering ordinary read-only snapshots to invent global ordering would violate their contract.

## Research probe and limits

The opt-in `internal-admission-research` feature adds a **session-local, bounded** `AdmissionResearchLog` (max 1,024 records; explicit truncated flag) shared by existing graph runs in exactly one `RunningApp` generation. Within the authoritative `GraphRunPublication::admit` control mutex, only after all gate checks and the state transition to **Running**, a successful dispatch records its globally monotonic research sequence, `GraphRunIdentity`, operation ID and attempt. The sample is captured before spawning the callback. Failed resource reservations, pending/paused operations and rejected safe-point transitions do not record an admission. The fixture reads the research snapshot only after its graph futures complete; no active or unknown external effects are inferred.

The sequence represents the *instrumented post-control successful-admission witness*, not an existing public cross-run ordering API. The additional shared research lock can change runtime interleavings; therefore comparisons to earlier uninstrumented callback start ranks are **not** performance/fairness guarantees for production. The probe has no queue, no admission veto, no new scheduler, no remote transport, and no durable persistence. The explicit cap and truncation flag prevent silent unbounded research retention. It is feature-gated and **must not be merged as a production contract**.

## Reproducible workload and acceptance oracle

Use the real public `RunningApp::execution_graph` runner, rather than a model-only arbiter. Four test variants repeat each case four times in both debug and optimized builds using current-thread Tokio and a four-worker Tokio runtime:

- **Conflicting declared resource:** 64-read operation graph (run-local max 1), first read held; earlier-waiting exclusive writer. Both complete after the held read releases.
- **Capacity-only contention:** 64 resource-free operations (run-local max 32), first 32 held; one resource-free cold run waits for the shared 32-operation session budget. Both complete after release.
- **Safe-point proof:** an initially paused one-operation run produces **no** successful admission record. One permitted step dispatch creates exactly one run- and attempt-correlated record.

Each completed case checks: 65 and only 65 successful admission records, sequences `1..=65` without gaps or duplicate attempts, matching session generation, exactly one cold operation record, expected earliest possible cold ordinal, and callback count/report success. The test prints **both callback-start rank and instrumented successful-admission rank** and does not assert a new fairness bound.

Commands:

```sh
cargo validate --check
cargo smoke
cargo test --locked -p riesel --features internal-admission-research --test scheduler_admission_research -- --nocapture --test-threads=1
cargo test --locked --release -p riesel --features internal-admission-research --test scheduler_admission_research -- --nocapture --test-threads=1
```

The two dedicated CI steps are enabled **only** on this draft research PR branch, after ordinary workspace validation and smoke tests. Windows is separately gated by the existing release policy. Record the exact head and real output on [#226](https://github.com/aschenrot/riesel/issues/226).

## How to interpret the result

A correct instrumented order is needed before any proposal to cap the number of real conflicting admissions that overtake an eligible run. It **does not** accept F1, F2, capacity-only fairness, contested-demand accounting, local `max_in_flight` semantics or a numeric service bound. Those remain the owner's decisions in [#226](https://github.com/aschenrot/riesel/issues/226) / [draft contract #263](https://github.com/aschenrot/riesel/pull/263).

This probe intentionally measures the *current opportunistic baseline*, not a candidate F2 policy. A later mechanism comparison must also test reader/writer phase closure, multi-key all-or-nothing grants, independent progress, bounded claim cleanup, pause/cancel/generation fencing, and performance in the presence and absence of instrumentation.
