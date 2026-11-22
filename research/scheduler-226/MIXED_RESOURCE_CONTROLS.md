# Riesel #226 — mixed-resource and control admission (research only)

## Purpose and verified baseline

This experiment investigates **current** session-owned resource admission using the production Riesel public SDK, without modifying `SessionGraphAdmission` or selecting a fairness policy. Base revision: protected `main` `e1a4bcacc08ea9c6833a49b090fceaf9e553af4a` on 22 November 2026. Evidence on [#245](https://github.com/aschenrot/riesel/pull/245) already shows repeatable batch-shaped service ordering in finite single-key workloads, alongside correct unrelated resource progress; it is not proof of infinite starvation.

The source's single lock checks all requested resource keys **atomically** before inserting a reservation. A read/write conflict or full 32-operation capacity denies admission, but does not create a waiter reservation, FIFO turn, priority token or promise of fairness. Wakes from released leases are notifications, not grants.

## Exact public-SDK test cases

1. **Multi-key no hoarding.** Hold an exclusive writer on `alpha`. A second graph requests exclusive `alpha` **and** `beta` and remains pending. An independent exclusive writer on `beta` must complete before `alpha` is released; the denied two-key request cannot hold `beta` speculatively. After releasing `alpha`, the two-key request completes once.
2. **Read sharing versus waiting write.** Hold a read lease on `shared`. Start a waiting exclusive writer and confirm it has not dispatched. A second reader on `shared` must be admitted concurrently with the first reader, before its release, while the writer remains waiting. After release the writer completes. This is consistent with no head-of-line restriction on compatible readers; it could permit writer starvation under an indefinitely renewed reader stream, which this finite fixture does **not** establish.
3. **Paused waiting run.** Hold an exclusive writer on `shared`. Start another waiting writer, pause its authoritative run admission before release, and free the resource. The waiting callback must not run until an explicit `GraphControlTarget::Run` step. One step admits exactly one callback, without a second resource reservation authority or retroactive preemption.

Each case is bounded by explicit test timeouts. Resource safety, authoritative run completion and callback counts are assertions; any elapsed timings or ordering variation are diagnostic only. The application owner is retained for the entire run, because dropping `RunningApp` revokes the session.

## Method and limits

```sh
cargo validate --check
cargo smoke
cargo test -p riesel --features async-tcp --test scheduler_mixed_resource_research -- --nocapture --test-threads=1
cargo test -p riesel --release --features async-tcp --test scheduler_mixed_resource_research -- --nocapture --test-threads=1
```

The branch-limited Linux CI step runs both profiles and ordinary validate/smoke. Results must be recorded against the exact head and merge candidate. This branch intentionally contains a research fixture and draft-only workflow change, **not** production code. Keep this PR draft and unmerged.

## Decision questions deliberately left open

- **Fairness scope:** per run, per eligible operation, per resource key or intentional no-guarantee. Resource safety alone is not a fair scheduling contract.
- **Multi-key wait requests:** fair reservation must atomically consider all keys and must not block unrelated work behind a request waiting on another key.
- **Reader/writer liveness:** allowing new readers while a writer waits maximizes compatible progress but may delay the writer indefinitely; writer preference could introduce its own priority inversion and throughput cost.
- **Cooperative controls:** a paused or step-armed run must not consume scarce capacity before the accepted safe-point gate admits work.
- **Sustained load:** these finite cases cannot establish service bounds under unbounded hot/reader producers. A separate controlled sustained-arrival experiment and explicit service objective are required before adopting a fairness mechanism.
- **Lifecycle:** a later production design needs cancellation, stale-session fencing, run-limit, multi-domain and memory-bound waiter tests under one session authority.

Any future ready-index or resource-aware wake mechanism must preserve the same authoritative reservation semantics and uncertainty behavior. Do not infer scheduler-owned clocks, durable retries or nested ECS/world preemption.
