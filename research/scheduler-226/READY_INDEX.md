# Riesel #226 — dependency-aware ready-index differential (research only)

## Scope and provenance

This is a **research-only comparator**, built on protected `main` after the completed #224/#233 optimizations. The candidate remains in an open draft PR and is **not** an accepted scheduler architecture, public API, release benchmark or implementation grant. It extends the earlier #237 declaration/topological-cursor research with one derived readiness index. Existing plan validation, operation binding, session admission and run observation are unchanged.

## Proposed research mechanism

The immutable, already validated operation graph has declaration-indexed nodes and dependency IDs. During graph-run creation, derive:

- `unmet[node]`: count of prerequisites not yet completed successfully
- `dependents[parent]`: children affected when a prerequisite completes
- `ready`: declaration-index-ordered set of pending nodes with zero unmet prerequisites

A successful completion decrements only direct dependents and inserts newly eligible pending children. A non-success terminal completion leaves prerequisite counts unsatisfied and uses the **existing** topological blocked propagation and observer transitions; blocked candidates are removed from the derived set. The ordinary run still owns the same one-shot `JoinSet`, 16-run/32-session operation budgets, `GraphRunPublication` control gates, `SessionGraphAdmission` resource/domain locks and attempt-scoped effect recorder. Every candidate is **rechecked** for current state, signal trigger, pause/step eligibility and shared admission authority before dispatch.

A candidate blocked on a resource, signal or safe point stays eligible in the set; other candidates may still run, and notifications wake the same outer coordinator. This is a **single-run readiness representation**, not cross-run fairness, a timer, priority queue, delegated ECS scheduler, persistent retry ledger or new external authority.

The candidate uses a sorted set keyed by original declaration index to preserve the baseline's selection preference among currently eligible nodes. A topological cursor is independently known to change some independent-operation tie-breaks; the index must not. Adding ready-set memory and per-completion bookkeeping has costs, so reduced scan visits alone do not prove it is worth adopting.

## Research verification

Against the actual public Rust SDK `RunningApp::execution_graph`, compare `RIESEL_RESEARCH_READY_INDEX=1` against no flag:

- 128, 512 and 1,024 nodes, independent wide graphs, forward-declared chains and reverse-declared chains, five repetitions each
- Baseline A1 / candidate B1 / candidate B2 / baseline A2 within one CI job, Rust release-profile tests
- Actual coordinator admission visits and index setup cost, not invented theoretical throughput
- A front-of-graph pending signal must not suppress independent work; after the signal, the blocked operation completes
- An explicitly shared-signal, two-branch test verifies original declaration-order tie-breaking rather than topological-order preference
- Public SDK `execution_runner`, `execution_domains`, `execution_nested` suites run under candidate selection, including unknown/failure chains, resource conflicts, cooperative controls and session invalidation
- Workspace `cargo validate --check` and `cargo smoke` still run

All private flags, diagnostic counters, research fixtures and branch-specific CI steps are **unmergeable-by-design**; do not cherry-pick them as a production patch. If an index is eventually selected through #226's explicit decision workflow, implement it independently, without research toggles, with separately verified acceptance gates on current main.

## Limits and next decision criteria

The sorted ready set is not an indexed signal subscriber map or per-resource wait queue. On every wake it still examines pending dependency-ready but signal/resource/step-blocked candidates; under sustained event bursts or many blocked ready nodes, total visits may remain large. Index creation is O(nodes + edges) apart from ordered-set insertion, and retains edges and ready metadata for each live run. Measure initialization, memory growth and real handler costs before accepting an implementation.

A meaningful decision compares this candidate against retaining a documented graph envelope and against a more complete event-driven candidate with indexed signal/resource readiness. The accepted external resource and safe-point gates cannot be moved or bypassed. Clock/recurrence, admission fairness, domain-owned nested scheduling and durable effects remain separate #226 questions.

## Negative-workload verification and session ownership

The research branch also includes real-runner checks for 256 signal-gated ready nodes with independent eligible nodes; 256 operations contending on one declared write resource alongside independent operations; and a reverse-declared fan-out/fan-in DAG. Baseline and indexed variants must both execute these tests before interpreting their visit counts. A dependency-ready candidate blocked on a signal, resource or pause still requires a fresh admission check, so the ready index is not claimed to eliminate all repeated scanning.

An early CI attempt at these added fixtures failed the baseline `report.succeeded()` assertions because two fixtures had constructed a graph from a **temporary** `owner().execution_graph(...)`. The actual `RunningApp` implementation calls `shutdown()` in its `Drop` handler (`crates/riesel_app/src/runtime/running.rs`), which expires the session before the graph future can run. The fixtures were corrected to retain an explicitly named application owner through the awaited report. This is a fixture lifecycle defect, not evidence of a scheduler-index correctness problem. Its fix requires a **fresh exact-head CI run** of both variants before any negative-workload conclusion.
