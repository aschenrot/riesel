# #59/#75 — Durable unknown OS mutation outcome, real Riesel Counter

**Status:** bounded research-only fixture, pending actual GitHub Actions execution at a verified commit. This is not a production Manager architecture, a selected ledger schema, accepted authorization mechanism or release guarantee. Parent [#59](https://github.com/aschenrot/riesel/issues/59), shared outcome terminology [#75](https://github.com/aschenrot/riesel/issues/75), draft proof gates [#86](https://github.com/aschenrot/riesel/pull/86).

## Original requirement and gap

The first installed Manager must preserve independently owned app lifetimes when the controller crashes, reconcile exact OS launch attempts after reconnect and represent **uncertain** destructive command results without silently issuing a second mutation. It must reject untrusted same-UID clients and support both user-session and machine-service scope on Linux, Windows and macOS. The previously green real OS owner/root test authority [PR #139](https://github.com/aschenrot/riesel/pull/139) allocated approvals and receipts **in RAM**. Its exact-bound stop did not test loss of a reply or durable observations.

**Bounded test question:** Can a root-owned research journal preserve an exact attempted stop on an actual Riesel process when a protected test controller dies before the effect, or dies after the effect but before a response? Can a newly started root observer distinguish an unchanged running old attempt, a newer attempt, and externally observed stopped state, without replaying any stop?

## Scope and execution

- Disposable GitHub Ubuntu 24.04 runner, actual systemd PID 1, Rust 1.100.0, and unmodified pinned real Riesel Counter at PR #90 SHA 5201e61cc74bab778dfc63a3788f1708b2530f21.
- A uniquely named transient system service uses DynamicUser=yes and Restart=no. Its real Counter PID, /proc process start tick, separate OS UID and systemd InvocationID are observed. No application runtime issued epoch or GUI/HTTP readiness is inferred.
- A root-owned 0700 research directory contains a protected SQLite ledger. The trusted root *test* worker precommits a submitted intent with the OS attempt, Counter PID/start tick, executable digest and verb, then exits using os._exit at one of two deterministic points.
- **Crash before effect:** app and attempt remain active; new observer classifies original operation as unknown/active, **not authorized to automatically retry**.
- **Superseding attempt:** a separate root *test* controller intentionally restarts the real systemd unit, creating a different InvocationID. Reconciliation of the old stop must classify it superseded without killing the replacement.
- **Crash after effect:** a fresh submitted record is committed, actual systemctl stop of the new Counter attempt succeeds, then worker exits before recording a completed response. A new observer confirms absence of the original process and externally observed inactive state, retains the historical submitted phase and does not claim a success reply from the crashed worker.
- **Repeat reconciliation:** another independent observer makes no OS mutations; appended evidence is still available with both committed operations.
- **Hostile real separate client:** UID/GID 60090 with zero effective capabilities is denied direct SIGTERM, systemctl stop, sudo, journal read and journal writes. The normal CI runner itself has ambient sudo, so its UID is **not** the hostile test principal.
- Cleanup affects only the random transient systemd unit, /opt research install directory and root-owned temporary journal directory. No existing application or real Manager data is altered.

The source deliberately has **no production policy issuance, broker, user consent, browser endpoint, app exporter or service install**. A root test authority is not independently trusted operator input. The original hostile same-UID first-release approval requirement stays OPEN.

## Falsifiable expected evidence

| Case | Requirement | Honest outcome |
| --- | --- | --- |
| Real OS owner | Separate actual Counter principal, observed process ID/start tick and InvocationID | A real Linux machine service, not proof of desktop scope |
| Pre-effect worker crash | Durable submitted record but original app remains running | Unknown/active; **no automatic retry** |
| New OS invocation | systemd restart changes InvocationID; old command remains in journal | Superseded old operation; replacement stays active |
| Post-effect worker crash | Real Counter exits and unit is observed inactive, despite no completion receipt | Externally observed effect; not necessarily causally attributable from observation alone |
| Repeat observers | Submitted records and observations survive process exits | Journal state persists across **process** crashes |
| Hostile UID | All tested root journal/OS access paths refused | Conditional on no attacker ambient elevation |
| Cleanup | Only uniquely named research resources removed | No permanent install or Manager source changes |

The dedicated workflow is [here](../../.github/workflows/manager-durable-unknown-outcome-research.yml) and [the probe](manager_durable_unknown_outcome_probe.py) must be run only on a disposable Ubuntu real-systemd host. **No CI outcome is claimed until the workflow has completed.**

## Architecture alternatives and bias audit

**RAM-only pending requests:** cannot recover from controller crash. A client might reissue a destructive stop against a new app attempt. Not acceptable as the sole production outcome source.

**Protected durable command journal plus OS observation:** promises compatible transport-neutral semantics (command identity, submitted versus effect observed, OS attempt and uncertainty), but the experiment's SQLite schema and protected root test process are just fixtures. Compare SQLite versus append-only fsynced event log versus OS job journals before choosing technology. Consider concurrency, retention, schema migration, data corruption, machine power interruption and security of recovery state.

**Native OS conditional mutation / operation identity:** might provide stronger atomicity, but Linux systemctl's check of InvocationID followed by stop is **not atomic** with respect to privileged concurrent replacement. Actual Windows/macOS owner API semantics must be investigated; never invent a shared atomic compare-and-stop guarantee.

**Blind retries after lost response:** unsafe for stop, update, removal and other operations with uncertain side effects. Default to status-query by the original command ID, not newly issued destructive requests.

**Preapproved unattended policy:** useful for the owner-approved opt-in bounded crash recovery/boot/update policies, but does not itself authenticate policy issuance, revocation or interactive privileged operations. This experiment does not implement automatic policy-driven recovery.

**Important durability limitation:** the fixture uses SQLite rollback-journal DELETE mode with synchronous=FULL, and terminates processes deliberately. It does **not** test power failure/kernel crash durability. SQLite documents stronger directory sync for synchronous=EXTRA in DELETE mode: [SQLite PRAGMA](https://www.sqlite.org/pragma.html#pragma_synchronous). Host filesystem and power-fail guarantees require a separate real evaluation.

**Lifecycle limitation:** systemd Type=exec and running Counter PID do not prove app protocol readiness; stopping the process does not prove a graceful Riesel runtime drain. [systemd service reference](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html). The user-session case and actual Windows/macOS behavior remain untested.

## Further acceptance gates

A production Manager requires protected CLI and local browser authorization against hostile same-user programs (PR #142 exposed cached polkit grants and X11 spoofable input), durable command admission and result reconciliation across revocation and controller loss, runtime session generation separate from OS attempts, verified app readiness, safe policy approval/installation and both execution scopes. The product owner has not selected one root helper, OS mechanism or database schema. **Do not close #59 or #75 from this fixture.**

## Reproduction and success markers

From a disposable Ubuntu systemd runner after building the pinned unchanged Counter, execute:

    python3 -m py_compile research/process-ownership/manager_durable_unknown_outcome_probe.py
    timeout 160 python3 research/process-ownership/manager_durable_unknown_outcome_probe.py --exercise "$PINNED_COUNTER_ROOT"

Expected, **not yet verified**, markers:

- RESEARCH_M59_UNKNOWN_REAL_RIESEL_OS_OWNER_VERIFIED
- RESEARCH_M59_UNKNOWN_OUTCOME_UNTRUSTED_OS_AND_JOURNAL_DENIED
- RESEARCH_M59_UNKNOWN_BEFORE_EFFECT_DURABLE_UNCERTAIN_NO_RETRY
- RESEARCH_M59_UNKNOWN_RECONCILE_FENCES_NEW_OS_INVOCATION
- RESEARCH_M59_UNKNOWN_AFTER_EFFECT_REAL_RIESEL_STOP_RECONCILED
- RESEARCH_M59_UNKNOWN_DURABLE_RECEIPTS_SURVIVE_CONTROLLERS
- RESEARCH_M59_UNKNOWN_EFFECT_CRASH_RESEARCH_PASS
