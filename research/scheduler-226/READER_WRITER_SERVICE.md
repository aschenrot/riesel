# Riesel #226 — Reader/writer service-order research on the A1 runner

**Status:** research only; not an accepted B1 fairness algorithm, QoS metric, admission API or production code. Based on protected `main` `d1354560eef3aea6b1e038a57877e1407a4b9689`, which includes delivered A1 dependency/signal indexing (merged #253/#255). This keeps the research current with the **real production** scheduler, not older pre-index research branches.

## Original product question

The owner accepted B1: bounded fair **admission opportunities among continuously eligible live graph runs** competing for session-owned resources, without promising elapsed-time deadlines or preempting opaque callbacks. The exact service bound, fairness accounting unit and read/write/multi-key behavior are **not accepted**. B1 may not replace the existing single `SessionGraphAdmission` reservation authority, `GraphRunPublication` safe-point authority, immutable authoring semantics or domain-owned inner scheduling.

Earlier #245 used long exclusive writers and demonstrated one run repeatedly reacquiring a resource ahead of another; #249/#251 found different results for distinct short live runs. This experiment exercises the delivered A1 code through reader/writer and admission-capacity probes, without treating an unaccepted scheduling policy as a test expectation.

## Fixtures and expected observables

**Case R — Long reader plan vs waiting writer (eight rounds, debug and release).** One Riesel application session owns a 64-operation plan where every operation declares `shared:Read`, and max in-flight is one. The first reader holds its read reservation behind an explicit semaphore. A second graph requests `shared:Write` and enters authoritative `GraphRunPhase::Running` without dispatch. A third graph writes a disjoint `unrelated` resource and must finish while the first reader is still held. Release the first reader and let both conflicting runs finish. Count actual callback start order, including the writer's rank among **65** callbacks. Do **not** assert any rank bound—current behavior is being measured. All reports must succeed, with 64 reader and one writer callbacks and no incompatible simultaneous access.

**Case S — Later compatible reader runs can pass a waiting writer (eight rounds, debug and release).** One held `shared:Read` lease blocks the writer requesting `shared:Write`. After confirming the writer is waiting, independently create and complete eight new one-operation `shared:Read` runs, before releasing the original held reader. All eight compatible readers must complete, the writer must not dispatch early, and the writer must finish after final held read release.

**Interpretation limit for S:** The original read lease remains held throughout the entire overtaking window. Thus the writer was not grantable during that window. The experiment **does not demonstrate that the newly admitted readers caused any additional delay after the original reader's release**, nor does it prove infinite writer starvation. It proves the current scheduler does not put new compatible reads behind a previously waiting writer and that a future writer-fair policy could change existing permissive read-sharing. A stronger, bounded overlapping read-arrival/service experiment would be needed to quantify actual writer delay beyond necessary held leases.

**Case T — Continuously overlapping reader handoffs (four rounds, debug and release).** Start one reader holding `shared:Read`, then an exclusive writer waiting on `shared:Write`. For eight successive handoffs, start a new reader and verify its callback has acquired a read lease **before** releasing the previous one. Await the previous read completion while the new read remains held. The exclusive writer must remain undispatched throughout because the resource is never fully grantable; when the final read releases, the writer completes. All readers and the writer must report success, with bounded live runs.

**Interpretation limit for T:** This shows that the current shared-resource conflict semantics permit a deliberately overlapping finite reader stream to keep a writer ungrantable, *despite each individual reader eventually finishing*. It does **not prove indefinite starvation** or measure the implementation of any alternative fairness policy. It isolates the need for a deliberate reader-admission phase if the accepted B1 product goal is intended to promise writer progress under a continuously replenished reader stream.

**Case U — Mixed contested and independent demand (four rounds, debug and release).** A held `alpha:Read` lease prevents one cold graph's `alpha:Write` operation from being grantable. That cold graph also contains eight independently ready `gamma:Write` operations with max one in flight. Verify all eight `gamma` callbacks complete while the cold writer remains `Pending`; the writer may execute only after the held read releases. This is a **baseline counterexample to treating any successful callback in the cold run as service of its contested writer demand**. It tests no actual fairness-credit ledger, since B1 is not implemented. Acceptance of F2 would require an additional candidate-side oracle proving that independent progress cannot indefinitely reset the contested writer's fair claim.

**Case V — Session-capacity-only contention (four rounds, debug and release).** One graph starts 64 independent resource-free callbacks with run parallelism 32; the first 32 hold their leases on an explicit semaphore, saturating the shared session's 32-operation budget. A second graph has one resource-free callback and remains undispatched until at least one hot lease releases. Release the hot semaphore and record the cold callback's actual admission rank among 65 starts. Test successful completion without asserting a specific fairness rank. This isolates a **capacity bottleneck without a declared read/write/domain conflict**. #226 still requires an explicit product-scope choice about whether B1 protects capacity-only competing runs. An ordinary (resource-free) callback competing for session capacity is not proof that the current scheduler violates an unaccepted capacity fairness guarantee.

## Why these cases matter for B1

- **Run-fair turns:** preventing an active long graph from repeatedly reacquiring the same resource when a competing run is *grantable* is a narrower objective than requiring every newly arriving compatible reader to stop the moment a writer waits.
- **Writer progress:** with continually overlapping readers, an exclusive writer may never see the entire resource become free even if individual readers eventually finish. If B1 promises fair progress for writers, the mechanism may need to temporarily hold new readers once a writer is waiting. This explicitly trades away some read throughput and is **a policy decision**.
- **Atomic multi-key demands:** fairness cannot reserve part of `{alpha:Write,beta:Write}` while the full set is unavailable, or globally block unrelated `gamma` work. The accepted baseline already passes the multi-key/no-hoarding and pause/step gates in [#249](https://github.com/aschenrot/riesel/pull/249).
- **Session/lifecycle:** a run that is paused, signal/dependency blocked, cancelled or expired cannot retain a stale fairness claim. Notification is not a resource grant. A fair waiting state must be bounded by the 16 live runs and pending eligible operations, and owned/fenced by the same authority that grants reservations.
- **No wall-clock/real-time guarantee:** opaque callback durations, native domain owners, executor scheduling and process effects are outside the scheduler's ability to preempt. B1 must express service assumptions in *eligible contested admission opportunities* rather than universal deadlines.

## Reproduction

```sh
cargo validate --check
cargo smoke
cargo test --locked -p riesel --features async-tcp --test scheduler_reader_fairness_research -- --nocapture --test-threads=1
cargo test --locked --release -p riesel --features async-tcp --test scheduler_reader_fairness_research -- --nocapture --test-threads=1
```

Cases U and V also run through the same debug and release commands; a green result means they exercised the current runner, **not** that a B1 fairness policy passed. The temporary workflow steps are **strictly gated** to the draft research branch. No changes to `crates/riesel_app/src/execution_runner.rs`, shared resource admission, public API, product contract or standard validation path are permitted in this PR.

## Decision disposition

This is an evidentiary gate for accepted B1's **precise semantics**, not an implementation acceptance. Fairness accounting between runs and specific read/write ordering are independent but interacting product questions. Compare run-level fair turns, resource/request-aware age with bounded reader phases, and a safety-only opportunistic fallback against the same capacity, multi-resource, cancellation, domain and independent-progress constraints; document any unavoidable loss of compatible-reader throughput. Record the owner's accepted scope before opening production implementation work. Leave the spec #226 open until exact decision and recurrence disposition are recorded.
