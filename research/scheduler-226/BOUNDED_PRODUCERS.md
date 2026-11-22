# Riesel #226 — bounded replenishing hot-run contention

**Status: research only.** This is not an accepted fairness policy, performance target, new component or a production implementation candidate. The branch is based on protected `main` at `e1a4bcacc08ea9c6833a49b090fceaf9e553af4a`, with **no changes to `ExecutionGraphRunner`, `SessionGraphAdmission`, plan contracts or app lifecycle**.

## Decision question

The production arbiter correctly rejects conflicting operations and enforces one shared session resource/capacity budget, but prior controlled tests found a single long hot graph repeatedly regained an exclusive resource, causing a cold waiting run to execute last. In contrast, 32 sequentially created one-operation hot runs allowed the pre-existing cold run to execute second. Does the cold run also obtain early service when the application **maintains multiple live, overlapping, replenished short runs**, rather than submitting the next only after all contenders have drained?

## Production-runner experiment

From the public Rust SDK create one real `RunningApp` session. Repeat four rounds per `backlog = 1, 4, 8`:

1. The first hot one-operation graph obtains exclusive `contested:Write` and blocks on an explicit semaphore. Observe its callback having started.
2. Start a cold one-operation graph requesting the same exclusive resource, verify its authoritative phase is `Running`, and confirm its callback has **not** started.
3. Start `backlog` additional hot one-operation graph runs from the same *immutable bound plan*, within the 16-live-run bound. They cannot dispatch while the first hot writer holds the resource.
4. Release the initial hot writer. Collect hot run completions using Tokio `JoinSet`; on each completion create one replacement one-operation hot run until exactly **96 hot callbacks** have executed. This maintains active producers as allowed by the executor, while never creating an unbounded live queue.
5. Await and verify the cold graph's authoritative report. All 96 hot and one cold callback must have executed exactly once, with successful operation states. Record the **cold callback rank among 97 starts** via shared atomic sequence. **Do not assert any expected fairness rank**: this tests current behavior, not a fairness contract.

This tests a bounded hot producer over multiple short Riesel graph-run identities. It is **not** a recurring Riesel runtime policy, an endless producer, or a proof of bounded starvation freedom. Current-thread Tokio, short side-effect-free callbacks, one Linux CI worker and deterministic creation order limit portability.

A second test starts a pending cold writer behind a held resource, aborts the cold runner future **before** it receives admission, then releases the resource and verifies a fresh writer can run. This confirms abandoned waiting does not leave a stale reservation or consume a run slot under the existing implementation. It does **not** prove that a new fairness waitlist would clean up automatically.

## Reproduction and evidence

```sh
cargo validate --check
cargo smoke
cargo test -p riesel --features async-tcp --test scheduler_continuous_producer_research -- --nocapture --test-threads=1
cargo test -p riesel --release --features async-tcp --test scheduler_continuous_producer_research -- --nocapture --test-threads=1
```

Branch-specific CI runs both profiles; verify exact PR commit, workspace validations and actual reported callback ranks before publishing findings.

## Architecture alternatives remain open

- **A — Safety-only, opportunistic admission:** preserve existing shared atomic reservations and document the absence of cross-run service guarantees.
- **B — Run-fair admission:** grant eligible resource requests service opportunities across run identities while preserving no head-of-line blocking on unrelated resources and retaining exactly one session resource authority. Requires precise definition of fairness, cancellation, safe-point pausing, read sharing, multi-key requests, and how much resource allocation a long graph may retain.
- **C — Fair request scheduling:** order waiting operation attempts per resource or resource-set rather than run. This may improve waiting-service bounds but can harm independent workloads, introduce writer/reader phase rules, and require bounded waiter state and strict lifecycle fencing.
- **D — Application policy above a conservative safety arbiter:** allow independent domains/hosts to manage their own service objective while preserving Riesel's shared resource correctness; may not help unrelated applications sharing one run session.

All alternatives should be assessed against the **same** product outcome, not chosen because the benchmark's particular creation order favors one mechanism. A new queue that tracks waiters outside the shared `SessionGraphAdmission` would duplicate the admission authority and is not acceptable without an explicit contract change.

**Delivery disposition:** #226 remains an open specification. This branch must stay draft/unmerged. A fairness implementation or public scheduling feature requires an owner-accepted product guarantee and independent production issue.
