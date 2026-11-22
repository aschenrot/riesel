# #157 — Runtime session publication ingress: bounded implementation record

**Implementation slice:** [PR #162](https://github.com/aschenrot/riesel/pull/162), derived from the owner-accepted [#75 operational contract](../../docs/product/riesel/operational-contract.md). This is **internal feature-gated Riesel SDK participation** for validation; it is *not* the publicly accepted application exporter, authenticated client transport, Manager protected control or release.

## Compared approaches

| Candidate | Benefit | Why accepted/rejected for this bounded slice |
| --- | --- | --- |
| Per-adapter application-generated epoch (#92) | Simple test host, no core session changes | Rejected as the normative issuer: can diverge from the real Riesel runtime across same-process recovery |
| Synchronous read/write lock on runtime token (#95) | Direct proof that revocation and admission serialize | Rejected for async operations: an admitted non-cooperative callback can block the owner while `invalidate()` waits forever |
| **Runtime token + short ingress mutex / counted permits** | Existing runtime is sole session issuer; admission/revocation order is atomic; permit does not hold the lock through user code | **Selected for internal verification** with a finite 32 in-flight budget; revocation closes immediately, optional timeout-bounded asynchronous drain reports remaining operations honestly |
| Session-owned async actor/queue | Can carry explicit worker ownership, bounded work and cancellation policy | Still viable for the eventual #60 publisher; not introduced as a mandatory new scheduler/daemon in this narrow increment |
| Privileged OS broker | Separately protects server and can issue process-specific grants | Orthogonal: #59/#60 authorization decisions remain open; not an alternative source for Riesel runtime session identity |

## Exact ownership, lifecycle and limits

- `StartupState::new` creates each genuine Riesel `SessionToken`; under opt-in `internal-publication`, it also generates one UUID lifetime marker. Clones share the same generation and ingress state. A fresh runtime session after managed-process failure creates a different marker, even within the same application PID.
- `InternalPublicationSession::try_admit` validates only the expected runtime generation, current nonrevoked state and the finite ingress budget **inside one short mutex section**. It returns an RAII permit representing tracked admission; no mutex is held over the actual callback, thread or async future. Callers **must retain the permit through the full operation**.
- `SessionToken::invalidate` closes this admission gate before releasing it and invalidates existing typed capability handles. It never blocks on admitted work. A task holding a permit may continue after revocation; cancellation, lost reply and a timeout do not undo its effects.
- `drain_for(timeout)` is an **optional bounded observation** of tracked permits, not a forced join or cancellation mechanism. A timeout reports `TimedOut { in_flight }`; the response may be unknown and no automatic replay is permitted. Recheck after a last-permit/deadline race to avoid reporting unresolved work when none remains.
- **Important remaining ownership gap:** a detached, unregistered background worker can misuse/dismiss the permit or outlive the runtime provider's own teardown. This helper does **not** attach that worker to `AsyncTasks` or otherwise guarantee that all external callbacks drain before the provider is destroyed. A production exporter must own and join/cancel its accepted work under an independently verified contract, and must not expose an unguarded bypass route.
- The internal feature is explicitly opt-in and documented as unstable; it adds no supported public SDK capability export, no OS IPC, no client or server identity verifier, no grant issuer and no Manager command. The UUID is nonsecret correlation, not authorization.

## Reproducible evidence

The dedicated [Linux workflow](../../.github/workflows/internal-publication-ingress.yml) runs protocol tests for generation, clone identity, revocation-before-admission, 32-request bound, in-flight work across an async `await`, finite drain and a modeled lost reply. It then builds the **unchanged genuine Riesel managed Counter crash fixture** and its feature-gated SDK recovery client. [Stdlib probe](probe_internal_publication.py) injects the real managed crash and verifies same-process whole-session recovery: old publication is refused before changing the new Counter, while the new session admits typed reads and writes.

**First fully verified code head:** `6ddebaf0aae92d09598758199003854b8eec6bfd`; [research CI #44983762989](https://github.com/aschenrot/riesel/actions/runs/44983762989) and [standard Linux Validate + smoke #44983762983](https://github.com/aschenrot/riesel/actions/runs/44983762983) **passed**. Later documentation-only heads still need their own required-status check prior to merge. Linux is the only platform exercised; Windows/macOS Manager parity is unverified.

## Follow-on acceptance boundary

Keep [implementation #157](https://github.com/aschenrot/riesel/issues/157) open until the integration gate proves one application-owned **async publication/worker owner** can use this runtime ingress without early permit drop, detached worker, unbounded teardown or new-session resource lifetime confusion. Verify a real runtime async recovery and ordinary shutdown with a deliberately held admitted operation, and test authorized adapter withdrawal separately from whole-session invalidation.

[#60](https://github.com/aschenrot/riesel/issues/60) still owns the explicit published app-facing contract, independent GUI integration, transport, verifiable client principal and per-operation grants. [#59](https://github.com/aschenrot/riesel/issues/59) independently owns protected hostile-same-UID Manager control. [#158](https://github.com/aschenrot/riesel/issues/158) owns source-qualified observations and general unknown-result reporting. Neither a green Counter fixture nor an internal ingress token proves those products delivered.
