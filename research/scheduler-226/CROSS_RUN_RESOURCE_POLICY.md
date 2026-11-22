# Riesel #226 — Cross-run resource-admission policy probe

**Status:** research only; not a scheduler policy or public API proposal. Based on current protected `main` as of 22 November 2026, branch `research/226-cross-run-resource-policy`. Accepted graph runner and admission authority are unchanged.

## Product question

What should Riesel guarantee when two independent one-shot graph runs in the **same application session** compete for a scarce declared resource, while a third run has unrelated work? Can the existing opportunistic admission provide bounded waiting or only eventual progress when released and the competing workload is finite?

Existing `SessionGraphAdmission::reserve` grants capacity under one session mutex when requested access/domain constraints do not conflict and the 32-session-operation limit is not full. A released `GraphOperationLease` signals `Notify::notify_waiters()` to awaken interested run coordinators. The notification is **not** a reservation, per-run turn ticket or fairness order; awakened run futures compete again for the next slot. This behavior is source verified and consistent with prior hot/cold measurements, but no starvation guarantee was accepted.

## Test on the real runner

One application session owns three running graphs:

- Hot graph: **64** independent operations, all exclusive writers of `contested`; its first writer blocks on a controlled semaphore permit while owning the resource reservation. After the first dispatch, the hot graph has 63 pending writers. Each writer consumes one release permit, so release quantity is deterministic.
- Cold graph: one exclusive writer of `contested`, started after the hot first writer holds the reservation; its observer confirms `Running` before release.
- Independent graph: one exclusive writer of a separate key `unrelated`, started while the hot holds `contested` and cold awaits. It must finish before hot resource release, demonstrating that the competing writer does not cause head-of-line blocking of unrelated work.

After confirming independent completion and cold not yet dispatched, release all hot permits. Record **cold dispatch rank** among 64 hot + one cold callbacks and post-release elapsed microseconds, repeating the scenario 8 times in one current-thread Tokio test. Assert all three authoritative graph reports complete, 64 hot and one cold callbacks actually dispatched, no bypass of declared resource conflicts, and cold rank between 2 and 65. Do **not** assert a fairness rank threshold: the behavior under investigation is opportunistic and may vary across valid Tokio schedules.

Run this test both in development and optimized builds under PR-specific Linux CI. `cargo validate --check` and `cargo smoke` remain mandatory. Prior #228 research demonstrated an explicit **cooperative pause** of hot future admissions can allow cold progress without preempting an already-running writer; it did not establish general fairness.

## Mechanism comparison and questions remaining

1. Retain **opportunistic admission** with explicit no-fairness contract and a bounded workload envelope. Lowest surface but a constantly refilled hot producer may keep other runs waiting indefinitely.
2. Require **run-fairness** among waiting eligible requests (e.g. per-run fair turns or limited running slots), avoiding accidental head-of-line blocking across unrelated keys. Needs a precise fairness/liveness metric, pause/step handling and cancellation semantics.
3. Use **resource-key-aware waiting** integrated into the *existing* shared session admission authority; a release event wakes only relevant eligible candidates, while nonconflicting readers/writers/domains still proceed. Avoid two conflicting authorities; reject old-session waiters. A purely local ready set cannot know when another run releases a reservation without a session notification.
4. Let application/domain owners implement specialized policies on top of the shared safety limit while Riesel retains conservative access and run observation, if generic fairness would distort important async or simulation workloads.

For each: model resource read/write sharing, multi-resource requests (deadlock / priority inversion / head-of-line blocking), domain-owner authority, 32 capacity, paused/stepped run tokens, session shutdown, old-run removal, external effects, and bounded waiter memory. Compare latency of a cold run and impact on unrelated throughput under **sustained producers**, not merely finite one-shot graphs.

## Interpretation limitations

The controlled 64+1 case has a finite hot workload, so eventual completion is expected whenever the session remains alive and callback permits are provided; it **cannot prove starvation freedom under infinite or recurrent hot producers**. Callback order is diagnostic and **not** a portable contract across executors. One small CI host does not establish production jitter or real-time frame guarantees. No per-client external authorization, ECS inner scheduling, persistent replay, automatic recurring clock or new fairness API is in scope.

A durable #226 architecture decision must first define whether a platform-wide per-run fairness guarantee is part of the desired product, and how it coexists with shared resource safety and explicit domain tick owners. No new implementation issue should treat the fairness policy as accepted without the owner decision.

## Equal-size competitor control

The same exact research test binary also runs eight repetitions of **two equally sized 32-operation graph runs** competing for the same declared exclusive resource. The first run starts and its initial callback holds the resource behind a controlled permit. The second run enters the authoritative `Running` phase before any permit release, so it is a real waiting competitor. After release, both 32-operation plans must finish. The test records the position of the first callback from the second run and the number of transitions between first-run and second-run callbacks in the observed dispatch order.

This deliberately checks whether an apparently unfavorable single-cold rank could simply reflect a disproportionate 64-versus-1 workload. It does **not** impose an interleaving promise. Finite workloads, one Tokio executor, fixed start order and actual asynchronous completion are important limitations; runtime-level reservation fairness cannot be inferred from a small set of dispatch sequences.
