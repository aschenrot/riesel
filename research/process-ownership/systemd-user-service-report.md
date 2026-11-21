# Real Linux systemd user-service owner experiment — #59

**Status:** Verified narrow research result, **not** adoption of a Manager process-owner API, an installed service, an accepted security model or an implementation issue.

- Source branch: [draft PR #94](https://github.com/aschenrot/riesel/pull/94), based on `main` at `a35ac7c9ac9749ff023a72595c6851caee5f4859`.
- Research harness: [systemd_user_service_probe.py](systemd_user_service_probe.py). It creates only random ephemeral `riesel-research-59-*.service` transient **user** units.
- First verified executable code revision: `cb183e89159ccf31754e2213e5986006a6576ebe`, [GitHub Actions 44882989009](https://github.com/aschenrot/riesel/actions/runs/44882989009) **PASSED** its dedicated real-systemd job with `Ran 1 test in 2.717s / OK` and the `RESEARCH_REAL_SYSTEMD_USER_SERVICE_OWNER_PASS` marker.
- Environment: hosted Ubuntu 24.04, systemd **255 (255.4-1ubuntu8.18)**, PID 1 **systemd**, unprivileged runner UID **1001**, existing `/run/user/1001` and active **systemd user manager**; linger was already enabled. This is an **actual systemd user service**, not a subprocess simulator, mocked D-Bus or merely a shell command that claims the service launched.

## What was actually demonstrated

| Case | Actual OS observation | Scope |
| --- | --- | --- |
| Real transient owner | `systemd-run --user` created `Type=exec` transient `.service`, `KillMode=control-group`, `TimeoutStopSec=2s`, `Restart=no`; active `MainPID`, `ExecStart`, `ControlGroup`, `InvocationID` observed | One systemd 255 user instance, synthetic Python application |
| Controller crash | Controller submitted service start then abruptly exited `os._exit(23)`; service and child survived | Manager *process* death only; does not cover user-manager crash, logout, linger-off or reboot |
| Independent reconciliation | New external controller obtained the **same actual** unit, `MainPID`, invocation and cgroup through `systemctl --user show` | Real owner observation; no durable registration/provenance or privileged command API |
| Duplicate start | Starting an already active unit under its exact existing name failed; original invocation remained unchanged | Unit namespace reuse is not itself an application registration/duplicate policy |
| New attempt identity | `systemctl --user restart` rotated `InvocationID`; old main and `setsid`-descendant process incarnations were no longer alive | OS owner restart, **not** Riesel same-PID whole-session recovery |
| Stale stop refused | Research `stop_if_current(unit, old_invocation)` rejected the previous invocation and did not stop replacement; matching invocation could request stop | **Only a local compare-then-stop model**; systemd `StopUnit(name, mode)` has no atomic expected-invocation parameter in the tested API |
| Descendant cleanup | A grandchild intentionally called `setsid`, ignored SIGTERM and remained inside inherited service cgroup; `KillMode=control-group`/timeout stopped the unit and the tests observed all recorded parent/child process births gone | Bounded cooperative parent and stubborn descendant; not hostile cgroup migration or user-manager death |

Commands are bounded and clean up only the randomly named research unit; no service or unit belonging to a real Riesel installation is touched. Ordinary Riesel workspace validation is an independent CI gate.

## Architecture implications

The [previous #59 report](../../docs/product/riesel/manager-control-plane-investigation.md) compared direct-child, OS-owned user services and one-owner hybrid. This real experiment **supports** a systemd user `.service` as a credible first *installed Linux* owner, provided the Manager is an **authorized observer/controller** while systemd is the **actual process owner**. It does **not** establish that this mode has been technically accepted.

The experiment also exposes the most important remaining correctness question: a preliminary `InvocationID` check before `StopUnit(unit)` has a **time-of-check/time-of-use race**. Another actor may restart the same unit between the comparison and stop request, causing the stop to affect a fresh attempt. The public `org.freedesktop.systemd1.Manager.StopUnit` call accepts a unit name and job mode but not an expected invocation ID: [systemd 255 D-Bus API](https://www.freedesktop.org/software/systemd/man/255/org.freedesktop.systemd1.html).

Before accepting a Manager stop contract, compare:

1. **Serialized trusted single-controller mutation** around a stable per-registration unit name, with a clearly defined foreign-actor/race threat boundary and reconciliation behavior. A local mutex cannot protect against out-of-band systemd user-bus controls or an unrelated same-UID controller.
2. **Unique owner unit identity per launch attempt**, immutable registration-to-attempt mapping and authorized registry admission; stale commands name old attempts and cannot directly address the replacement. This moves complexity into durable registration/admission/cleanup and requires a separate duplicate-start/race proof.
3. **OS-supported conditional/fenced management** if a verified systemd mechanism can enforce the expected attempt under concurrent mutation; do not assume this exists merely from observing `InvocationID`.

The correct contract should refuse uncertainty rather than kill the process identified by a guessed PID or silently accept a new attempt.

## Security and responsibility limits

- This research **does not run a Riesel executable** and never proves SDK app-session identity or authenticated callable readiness (#75). A systemd unit can be active while a published app interface is unavailable.
- The user D-Bus manager is reachable by the executing user. Any same-UID adversary or another trusted controller able to invoke systemd directly can interfere with a unit. Strong same-UID isolation requires a separate **real** trust/authority boundary; unit names, `InvocationID` and `SO_PEERCRED` are not credentials for distinct same-UID callers.
- Executable provenance is only checked as a test marker/command-line observation. Real trusted installed artifact identity must verify immutable build, configuration and ownership before launch or control.
- The test did not exercise Riesel application-level graceful drain, service disappearance, logout without linger, manager command journal crash windows, cross-controller start/stop races, service-manager crash, reboot or Windows/macOS OS ownership.
- A normal user-manager may be unavailable on many supported developer machines. Return explicit `owner_unavailable`, and do **not** silently replace a registered service owner with direct-child launch.
- `systemd-run --user` is a **research command**; selecting actual systemd D-Bus Rust adapter, installer, service policy or Manager API is separately reviewable.

## Remaining decision gates

- **M1:** accept/revise/reject Linux transient systemd user `.service` as the initial installed process owner, with precisely bounded survival/cleanup availability assumptions; distinguish this from opt-in foreground developer process ownership (#68).
- **M2:** choose and adversarially test a race-safe registration/attempt ownership strategy before claiming a stale stop cannot affect a newly started app.
- **M3:** decide first trusted-operator threat profile and its compatibility with #75 privileged client attachment; explicitly exclude hostile same-UID callers *only if the owner chooses that narrower boundary*.
- **M4:** specify Manager command journal retention and outcome uncertainty, actual Riesel graceful stop/readiness linkage, and CLI/browser authorization tests.

**Recommendation:** Accept the **bounded executed evidence** as replacing the earlier lack of a real systemd user-service probe, but **do not accept the final Manager technical specification** until the stale-stop admission race and local trust profile are settled. Keep [#59](https://github.com/aschenrot/riesel/issues/59) open and [#94](https://github.com/aschenrot/riesel/pull/94) draft. No implementation issue, public API, new crate, milestone, release claim or merge is authorized.
