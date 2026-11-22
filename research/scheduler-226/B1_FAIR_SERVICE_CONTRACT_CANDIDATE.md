# Riesel #226 — B1 fair-service contract candidate

**Decision status:** owner accepted B1's **product outcome** on 22 November 2026; neither F1 nor F2 below, the fairness accounting unit, the bound, a queue, nor a public API has been accepted. This is a **reviewable specification candidate** attached to existing [#226](https://github.com/aschenrot/riesel/issues/226), not a new accepted architecture or a production implementation. Source base: protected `main` `0138ebd9698b4989ae1f4b7403be410404bb89f9`. A1 prerequisite/signal indexing is **already merged** as [#253/#255](https://github.com/aschenrot/riesel/pull/255). This document changes no code or current authority.

## Goal and scope

Within one trusted Riesel application **session**, provide a bounded opportunity for a **live, continuously eligible graph run** to admit its next operation despite competing live runs that request conflicting declared resources or Riesel-owned domain admission. The user-visible improvement is meaningful forward progress for eligible runs, not global execution order or elapsed-time scheduling.

Preserve: one session-wide `SessionGraphAdmission` authority, separate `GraphRunPublication` cooperative safe-point owner, immutable per-run graph revision and attempts, existing 16 live-run/32 active-operation budget, application/domain clocks, caller/delegated domain ownership, and no automatic retry or claim about unconfirmed effects. Fairness must not create an extra runtime, process scheduler, priority API, external credential, global FIFO or preempt opaque callbacks.

The current `SessionGraphAdmission::reserve` receives only resource accesses and a domain boundary; it does **not** receive a run identity or a registered waiting intent. `SessionGraphAdmission` owns the session generation and `GraphRunLease` stores the monotonic run sequence; `GraphRunPublication::may_admit/admit` separately owns live pause/step eligibility. A B1 candidate therefore requires internal, session-fenced contender tracking and a cancellation-safe bridge between these existing authorities. Simply replacing `Notify::notify_waiters` with a different wake primitive cannot create a fairness guarantee.

## Distinguish eligibility from availability

A proposed operation is **logically eligible (E)** only while all are true:

1. Its exact application session generation and run are live; its immutable node is still `Pending`.
2. Every dependency succeeded and any one-shot declared signal has fired, without retrying a failed, cancelled or outcome-unknown ancestor.
3. Its run, operation and (if applicable) Riesel-owned outer-domain safe points are in `Running` or have the permitted step credit. An unsupported delegated-inner domain control never creates eligibility.
4. Its declared resource/domain requirements are validated and its run has not been cancelled, stopped or replaced.

An E operation is **physically grantable (G)** at one instant only when all declared resources and the shared 32-operation budget allow one **atomic**, all-or-nothing reservation. E can be true while G is false. A *writer waiting behind overlapping read leases* remains E even while no exclusive G instant exists; this difference defines the key owner decision. Declared conflicting domain-owner *modes* are rejected at whole-run claim time and **are not queued as B1 requests**.

A runnable graph with several E nodes must not obtain many independent fairness turns merely by declaring many ready nodes. In the accepted B1 outcome the accounting subject is a **live competing run**, while the exact selection and bound per resource set remain to be chosen. Original **declaration-order preference** continues to select equally eligible independent nodes *within* a run; fairness may determine which run gets the next conflicting admission opportunity.

## Owner decision: scope under continuous reader arrivals

| | **F1 — Availability-only fair opportunities** | **F2 — Drain-aware bounded read/write phases** |
| --- | --- | --- |
| Eligible waiting claimant | Only receives fair credit when its complete request can already be reserved (G), even if E held earlier | Can register E waiting intent despite G=false; the authoritative arbiter eventually protects a turn by declining **new** conflicting admissions |
| Continuous read overlap while writer waits | Permits newly compatible reads; writer can remain ungrantable indefinitely despite every individual reader eventually releasing | After a finite bounded read phase, stops/limits **new** contested readers; already admitted readers finish; writer gets its exclusive turn |
| Unrelated work and existing readers | Proceed as before | Proceed as before; the policy cannot revoke or pause existing callbacks or stop work using only disjoint keys |
| Potential benefit | Less wait bookkeeping and no intentional compatible-read throttling | Liveness for writers and multi-key conflicts under sustained compatible arrivals |
| Cost/risk | Narrower than usual interpretation of B1; continuously E writer may get no opportunities | Additional session-owned contender state; reduced read throughput/order during contention; deadlock and abandoned-waiter risk if poorly designed |
| Reversibility | Can later extend to F2 if the compatibility contract permits | Can later optimize mechanism without weakening the accepted liveness contract; downgrading to F1 would change admitted-service guarantees |

**Recommendation for a *product* decision: F2**, with symmetric protection for reader runs against a permanent stream of writers, and **without** deciding FIFO/round-robin/per-key data structures at this gate. The evidence makes F1 insufficient **if** B1's accepted “continuously eligible” includes E claims that never see a G moment. If the owner only wants F1, explicitly narrow B1's existing product language: neither “no indefinitely overtaken E run” nor writer progress under continuous read churn can be promised. Do not portray F1 as equivalent to F2.

Evidence:
- [#245 real-runner CI](https://github.com/aschenrot/riesel/actions/runs/45049376681) observed cold writer 65th/65 and equal-size second run 33rd/64 after long hot graphs repeatedly reacquired an exclusive key.
- [#249](https://github.com/aschenrot/riesel/pull/249) verified no speculative partial reservation for a multi-key request, concurrent compatible reads, unrelated progress and pause/step release safety.
- [#251](https://github.com/aschenrot/riesel/pull/251) verified bounded repeated **distinct** short hot runs served cold second and abandoned waiting runs left no current resource reservation.
- [#260 exact-head CI](https://github.com/aschenrot/riesel/actions/runs/45062782452) on the A1 runner: 64-reader long graph admitted waiting writer 65th/65; **eight controlled overlapping read handoffs** kept one compatible read lease active throughout, with writer undispatched until final release. This finite test establishes a possible ungrantable state under continual read arrivals, **not measured infinite starvation or the correctness of a proposed F2 algorithm**.

## Fairness measure — proposed, not finalized

Measure **successful conflicting operation admissions to other live runs** while one claimant remains E, rather than wall-clock milliseconds, task-completion order or number of resource notifications. Count one *actual* admission at the shared arbiter + cooperative safe point, not a speculative reservation later rejected by a paused controller.

A bounded service objective must specify an overtaking budget as a function of the number of **currently active contending runs** (at most 16) and how the budget is preserved across multiple declared resource sets. It must also define when the budget is reset on loss of E, when a conflicting holder fails to release, and how resource-phase closure forces a G opportunity. Do **not** yet assert “within N milliseconds”, “at most N−1 admissions” for arbitrary multi-key sets, global FIFO order, or hard real-time behavior; these are unverified and can be impossible under external noncooperative holders.

A candidate service rule worth testing: when a run remains E and some requested resources are contested, it eventually receives one protected admission opportunity **after existing conflicting leases cooperatively drain**, even if other runs keep offering new conflicting work. A protected opportunity is withdrawn when that run loses E; it may not reserve any subset of the requested resource set. “Eventually” must become a precise bounded admission-count test before production implementation, with a stated multi-key contention condition.

## Mechanism comparison: preserve a single authority

| Internal approach | Addresses long-run repeated reacquisition? | Addresses continuous overlapping readers? | Critical failure mode |
| --- | --- | --- | --- |
| Opportunistic `reserve` + broad `Notify` (current) | No service bound verified | No | Tokio polling order, busy run wins |
| Only targeted wake / voluntary yield | May improve observed distribution | No bounded guarantee | Wake is not reservation; next poll can be overtaken |
| Per-operation global FIFO | May appear to, but one long graph can register many tickets ahead of peer | Writer preference possible but risks global blocking | Unrelated keys head-of-line blocked; long plans monopolize tickets |
| Run-accounted, **resource-aware** fair admission inside existing `SessionGraphAdmission` | Plausible, requires proof | Possible with bounded reader/writer phases | Multi-key scheduling, stale run credit, pause race, throughput cost |
| Domain/application-only external scheduler | Only with cooperation from all competing runs | Cannot enforce on independently owned runs sharing one session | Duplicates or bypasses the common authority |

A chosen F2 implementation is free to use an internal queue, ticket, read phase, grant reservation or alternative bookkeeping **provided the observable bounded-service rule is met** and only the existing session arbiter actually owns resources. Run-local A1 readiness remains **candidate selection**, never a competing resource authority.

## Gate N contract precision — evidence, not accepted policy

[Real-runner research #260](https://github.com/aschenrot/riesel/pull/260) at exact head `c0e8dee92dee32be2a086c5847c9fbfd7a694eca` passed [Linux workspace/smoke and five debug/release tests](https://github.com/aschenrot/riesel/actions/runs/45067525794); the detailed source-qualified record is [#226 Gate N](https://github.com/aschenrot/riesel/issues/226#issuecomment-7085467199). Two added tests change the fairness contract's required discrimination:

- **Contested demand versus unrelated run progress:** with a held `alpha:Read` and an eligible cold `alpha:Write`, eight independent `gamma:Write` operations of that same cold run completed while the writer remained pending in all eight debug/release rounds. A scheduler that clears the cold run's fairness debt on *any* successful callback would incorrectly count `gamma` as service for `alpha`. Thus **run-level anti-monopoly identity alone is insufficient** for the stronger F2 promise unless the specific contested demand remains owed. This is a counterexample to a proposed accounting rule, not evidence of a fairness ledger implemented today. A possible representation is session-fenced `(run, eligible contested operation, complete demand)` plus run-normalized turns; the representation is **not selected**.
- **Capacity-only contention:** with no declared resource keys, 32 held hot callbacks saturated the session budget, then the waiting cold callback started **65th/65** in all eight debug/release rounds. This shows the existing opportunistic arbiter can favor a hot graph solely through session capacity, without a resource-access conflict. The accepted B1 goal refers to shared session-owned resources; **explicitly decide** whether its fairness guarantee includes the shared 32-operation budget. A capacity-fair rule may use the existing budget as one contested admission dimension, but must not invent a second resource owner or impose a global FIFO.

**Evidence qualification — callback order is not admission order.** The reported "65th/65" for the historical long graph and the capacity-only case is a **handler callback-start rank**, not an instrumented session reservation/`GraphRunPublication::admit` rank. The runner checks/records authoritative admission *before* spawning the future, so executor scheduling can reorder callback entry. Controlled resource holds still demonstrate safety and delayed progress, but these numbers alone must **not** be used as proof of an admission-overtake bound. Before production B1, add a source-owned successful-admission witness (session/run/operation/attempt scoped and emitted only after the safe-point gate) to compare fair mechanisms. A per-run observer revision or handler timestamp is not a session-global admission sequence.

### Admission distinctions that the test oracle must maintain

- **E — logical eligibility:** exact session/run/operation, dependencies and trigger, controls and pending state permit the candidate to request admission.
- **G — physical grantability:** at this instant the complete declared access/domain request *and* operation budget can be reserved atomically, without considering prospective fairness preference.
- **P — policy permission:** if F2 is accepted, an otherwise physically compatible newly arriving read may be deferred to protect a continuously eligible exclusive contender. Therefore **G does not imply P** under F2. In F1, service is considered only at G opportunities.
- **A — actual dispatch admission:** only the authoritative post-reservation `GraphRunPublication::admit` transition starts an attempt and consumes any step credit. A speculative reservation later rejected by pause, cancellation or a stale generation **must not** be recorded as A or consume final fair-service credit.

A service oracle shall count conflicting successful **A** admissions overtaking a continuously E contender; it shall separately record G/P when interpreting intentional reader-phase closure, session capacity limits and multi-key availability. It must never substitute callback completion, `Notify` wake order, release count, a `Pending` flag alone, or unrelated admissions of the same run as proof of service.

### Required scope and mechanism questions

| Question | Required owner-level outcome | What research must establish before production |
| --- | --- | --- |
| **Read/write liveness** | Choose F1 grantable-only or F2 drain-aware reader/writer phases | Falsifiable admission bound under cooperative releases and rolling read/write arrivals |
| **Fairness unit** | Run-level non-monopoly with **contested-demand claims** if F2 protects an eligible writer even while its run executes disjoint work | Claims move among pending nodes without registration explosion, loss of older claims or repeated unfair credit |
| **Capacity-only admission** | Explicitly include in B1, or defer as a stated limitation | Resource-free saturated competing runs; bounded service after cooperative capacity releases |
| **Multi-key atomicity** | Full-set all-or-nothing reservation and unrelated-key progress | Continuous `{alpha,beta}` claims against separate `alpha`/`beta` contenders; no deadlock, partial lease or global head-of-line blocking |
| **Safe-point and lifecycle** | Existing control/session/domain authority; never reserve or age a paused/cancelled/stale contender | Pause/step racing reservation, abandonment, generation replacement; earned turn only after A |
| **Executor independence** | No fairness guarantee based on current-thread ordering alone | Compare current-thread/four-worker Tokio; count actual admissions and preserve safety |

No numerical overtaking bound or specific FIFO/run-queue policy is accepted by this research update. If B1 excludes capacity-only contention, the contract must say so plainly; otherwise the proposed general-purpose session fairness may fail for pure asynchronous workloads without resource annotations. Neither F1 nor F2 is implementation authorization.

## Mandatory risk and acceptance matrix

Each test must execute **real `RunningApp::execution_graph` callbacks**, not only a stand-in simulation; define oracle/counters before changing the code. Include bounded deterministic tests, debug and optimized stress where useful, and the full graph/session regressions.

| Workload | Contract to verify | Safety or liveness risk |
| --- | --- | --- |
| 64 exclusive `shared:Write` operations in hot graph and one cold waiting writer; also 32/32 peers | An E competing run receives a bounded protected turn under repeated cooperative releases | Reacquisition monopolization and operation-level ticket gaming |
| Held reader, waiting writer and new read arrivals; controlled overlapping handoffs | If F2 selected: reader phase eventually closes, all existing readers drain, waiting writer gets service despite ongoing arrivals | Writer indefinite E-but-not-G state |
| Writer flood followed by waiting read-only run | Reader run also gets a bounded turn, without dropping writer correctness | Asymmetric writer preference can starve readers |
| Multiple read-only runs with **no** waiting writer | Compatible reads may overlap as before; do not serialize gratuitously | Throughput regression |
| `{alpha:Write,beta:Write}` denied while `alpha` held, plus independent `beta` and `gamma` work | No partial lease, no deadlock, unrelated `gamma` progresses; define policy explicitly when fair protection of alpha+beta must throttle new beta work | Accidental global FIFO or perpetual multi-key starvation |
| 16 live runs, 32 active operations, different resource sets | Never exceed admitted limits; fairness claims remain bounded by real live runners and their eligible operations | Queue/memory growth, slot leakage |
| Signal before prerequisite, duplicate signals, failed/unknown ancestor | Only E eligible nodes can claim a turn; no extra one-shot executions or effect replays | Phantom waiters and unsafe retries |
| Run/operation/domain pause and single step during resource wait | Paused claim yields; exactly one step is consumed **only by successful atomic admission**; another run can progress | Stale fairness priority, lost step credit |
| Cancel/abort/expire a waiting run; session generation replacement | Remove or fence waiting claim and notify; it cannot later receive a lease; old generation cannot alias new run | Stale waiter, use-after-close or resource pinning |
| Domain boundary and delegated inner scheduling | Fair only across Riesel-authoritative outer operation/tick admission; domain mode conflict remains fail-closed | Duplicate ECS/native admission authority |
| Multi-thread Tokio and multiple contention patterns | Outcomes hold independent of deterministic current-thread polling and exact creation order | Test-only illusion of fairness |
| Repeated short-run tick/frame workloads and large plans | Measure wait registration/churn, heap per pending eligible claim, resource-release overhead and latency distribution; no unbounded cache | Per-wake scans and per-run allocation blowup |

**Important concurrency design check:** `may_admit` is only a precheck, while `publication.admit` atomically consumes step credit **after** `SessionGraphAdmission::reserve`. A new fairness credit **must not be permanently consumed by a speculative reservation rejected after a simultaneous pause/cancel**. Do not acquire the observation/control mutex inside the session resource mutex without auditing lock order. Session generation + run sequence + operation/attempt identity must fence every waiting intent; use RAII/explicit cleanup on cancellation and failed admission. Release notifications alone are not tickets. A protected turn cannot be claimed by another run's node, replay effects, or retain capacity through an abandoned graph.

## Delivery and migration

- **Before implementation:** owner selects F1 or F2, together with whether fairness is **per run** (recommended for B1), and accepts the measurable overtake/reader-phase conditions. Record those invariants in the existing [execution-plan contract](../../docs/product/riesel/execution-plan-contract.md) and product definition. Leave mechanism details open until the same workloads and teardown tests discriminate the alternatives.
- **After acceptance:** one implementation issue for the coherent shared admission/lifecycle behavior, unless independent verification truly requires splitting by separate ownership. Use a clean branch from then-current protected `main` with permanent tests and no research-only knobs, and validate exact combined candidate before protected merge.
- **Compatibility:** no new plan encoding or authoring schema is needed merely to improve session fairness; the change may alter execution interleavings and resource admission ordering, so document behavior for clients that previously observed opportunistic ordering. Never infer exact remote authorization or durable fairness from an in-process session. Existing Rust-first, editor-safe S2 and optional S3 plan evolution remain independent.
- **Deferred:** priority/deadline/QoS, hard real time, automatic recurrence/catch-up/unknown-effect retry, global service scheduling, nested ECS-system control. Applications continue providing their own clock/tick proposals. No silent change to their domain authority.

**Disposition:** proposal only; retain [#226](https://github.com/aschenrot/riesel/issues/226) as the single open specification and preserve research [#260](https://github.com/aschenrot/riesel/pull/260) as unmerged evidence. This document/PR is not authorization to implement or merge B1.
