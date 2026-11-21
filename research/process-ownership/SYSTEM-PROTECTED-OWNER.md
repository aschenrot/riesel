# #59 — Linux protected **system** owner vs same-user OS bypass

**Date:** 2026-11-21  
**Status:** isolated, reproducible **research only**. The experiment is **not** a production Riesel Manager, a new system daemon, an accepted OS backend, an authorized browser/CLI channel, or Windows/macOS evidence.  
**Baseline:** latest `main` at branch creation. Parent [#59](https://github.com/aschenrot/riesel/issues/59), identity/security coordination [#75](https://github.com/aschenrot/riesel/issues/75).

## Original requirement and why this experiment matters

The owner requires the first installed Manager release to manage **user-session applications and machine background services** on Linux/Windows/macOS, with Manager-controller crash independence, durable exact-attempt reconciliation and rejection of untrusted same-OS-user management commands. A single protected command broker alone is insufficient when the running app is an ordinary target which a hostile same-UID process can terminate directly.

- [#116](https://github.com/aschenrot/riesel/pull/116) **executed** actual Linux `SIGTERM`: a same-UID unprivileged process could terminate a same-user target, while a different unprivileged UID was denied.
- [#96](https://github.com/aschenrot/riesel/pull/96) **executed** `SO_PEERCRED` and isolated-owner authentication research: a distinct server UID protects server secrets but cannot distinguish two clients with one allowed UID.
- [#94](https://github.com/aschenrot/riesel/pull/94) and [#99](https://github.com/aschenrot/riesel/pull/99) exercised **user** systemd services, bounded controller survival/attempts and real Todo behavior; their same-user user manager is **not** a hostile-same-UID protected process owner. #99 does **not** prove generic graceful app drain.

This research now tests the contrasting **system** service with a **separate app OS principal**, not just a system service wrapping an app still owned by the hostile desktop account.

## Scope and real OS fixture

[Script](system_protected_owner_probe.py), executed on an ephemeral Ubuntu 24.04 runner with actual systemd PID 1:

1. An unprivileged CI coordinator uses disposable runner-only `sudo -n` to submit an actual `systemd-run --system` **transient Type=exec unit**. The unit runs `/usr/bin/sleep 100` under **numeric service UID/GID 60081**. No permanent service or OS account is installed. Its unit name includes a random 12-digit hex attempt identifier under an exact test prefix.
2. The initiating controller calls `os._exit(23)`. A separate controller subsequently observes systemd's `MainPID`, `InvocationID`, `ActiveState`, and kernel-reported process UID.
3. **Two independent untrusted clients running under the same numeric UID/GID 60090**, each with zero effective capabilities and no passwordless sudo, try: direct `SIGTERM` to that exact PID, actual **system-manager** `systemctl stop/restart/kill` without interactive password prompting, and writes to an explicitly protected **root-owned 0700/0600 temporary registration**.
4. Every forbidden effect must be denied; confirm the exact PID, `InvocationID` and liveness **remain unchanged** after *each* client. This must not depend solely on nonzero return codes.
5. As a **separate privileged positive control**, the CI coordinator uses `sudo systemctl --system stop` and confirms the managed process/unit ceases running.
6. `finally` stops/resets **only** the named test unit and deletes **only** its temporary root-owned registration fixture. No service files, user accounts, real app assets, root application code, or persistent Manager installation.

Reproduce only in a throwaway Ubuntu/Linux machine with real systemd, `setpriv` and `sudo -n` for the CI coordinator:

```sh
python3 -m py_compile research/process-ownership/system_protected_owner_probe.py
python3 research/process-ownership/system_protected_owner_probe.py
```

The [path-scoped research workflow](../../.github/workflows/manager-system-protected-owner-research.yml) runs this one experiment under a time limit. Normal Linux `cargo validate --check` and `cargo smoke` remain separate required PR checks. **Before an actual green exact-head CI result, the markers below are expected outputs, not verified findings.**

Expected evidence markers:

- `RESEARCH_MANAGER_SYSTEM_OWNER_SURVIVES_CONTROLLER_EXIT`
- `RESEARCH_MANAGER_SYSTEM_OWNER_DENIES_SAME_UID_UNTRUSTED_CALLERS`
- `RESEARCH_MANAGER_ROOT_OWNED_REGISTRY_UNMODIFIED`
- `RESEARCH_MANAGER_PRIVILEGED_SYSTEM_OWNER_STOP_CONFIRMED`
- `RESEARCH_MANAGER_SYSTEM_OWNER_S1_DIFFERENTIAL_PASS`

## Strict interpretation and threat-model gaps

**If passing, what it proves narrowly:** on the tested Linux runner, OS-enforced separate app owner/UID, real system-service control and root-owned artifacts can protect **the tested machine-service target** from **the tested unprivileged user**; the real system owner outlives the initiating controller; an OS-authorized coordinator can stop the exact observed unit.

**What it does NOT prove:**

1. **M-S2 legitimate operator authorization:** The two UID-60090 clients are both untrusted and both denied. No independently authenticated *legitimate* GUI/CLI/automation actor under UID 60090 is admitted by this fixture. The CI runner's `sudo -n` is an **ambient privilege of that runner account**, which any hostile program under the runner's same UID may reuse. The positive privileged stop is therefore **not a solution** for the owner's accepted same-UID caller authorization requirement.
2. **User-session app interoperability:** `sleep` does not need Wayland/X11, login/session D-Bus, notifications, local files or GUI input. A user-session app isolated under a different UID may lose desktop access, and sharing the original session bus could reintroduce same-UID-like bypasses. This release-critical gate requires a separate real GUI/session fixture.
3. **Other OS authority:** Linux `systemd --system` is one candidate for machine services, not an implementation for Windows Service DACLs, macOS launchd/XPC or all Linux distributions. Actual Windows/macOS protected-owner and authorization tests remain mandatory **manual release** evidence; Linux alone does not establish parity.
4. **Manager completeness:** no Riesel runtime, provider/session generation, authenticated app readiness, stop protocol, durable registry/transaction, competing-controller admission, permission-grant UX, portable installer, update path, recovery policy or arbitrary executable provenance is verified. `InvocationID` observation does not make a conditional stop atomic.
5. **General immunity:** root/administrator/kernel attacks, service-file substitution by a privileged actor, ptrace/cgroup/namespace and local GUI input forgery, system-manager crash/reboot and cross-user data-sharing have not been modeled comprehensively. The fixture tests precisely the listed negative operations and a single app process, not all OS bypasses.

**Decision rule:** accepting a protected `systemd --system` machine-service *candidate* requires an independently protected client principal and safe OS owner whose control cannot be bypassed by the human account; if that would block intended user-session GUI apps, **document the trade-off, compare independently enforced per-app confinement and request owner review before relaxing the security requirement**. No silent downgrade to `systemd --user`, UID-only peer auth or readable same-user bearer secrets.

## Next decisive experiments

| Gate | Alternative/hypothesis to compare | Sufficient falsifiable experiment |
| --- | --- | --- |
| **M-S1a** Separate OS owner for machine service | This system-service differential vs privileged broker with same-user app owner | Attacker direct SIGTERM/service call/registry writes denied; privileged owner still operates; controller crash survival |
| **M-S1b** User-session GUI under protected owner | Separate UID owner + narrowly delegated desktop access vs actual OS-enforced confinement/sandbox | Genuine user-session GUI interaction works, attacker same-user PID/service/injection/asset modification denied |
| **M-S2** Legitimate CLI/browser authorization | OS-approved per-operation human consent, separately signed/attested client, or bound privileged client principal; compare unattended automation | Legitimate and hostile **same-UID** clients behave differently under actual OS enforcement; stolen/replayed proofs and browser CSRF/origin cannot bypass |
| **M-S3** Durable launch admission | Reuse #94 unique attempt + transaction concept inside protected owner, with ambiguous OS effect and competing controllers | No duplicate after lost outcome; old stop cannot target new attempt, even under concurrent mutation |
| **Portability** | Native Windows Service/Process DACL and macOS launchd/SMAppService/XPC ownership | Repeat real OS-level negative and positive checks manually, documenting exact OS/configuration/installer permissions |

Do **not** equate test fixture success, a technical preference, and approved first-release architecture. #59/#75 remain open, draft research stays unmerged, and no public Manager API or implementation issue is authorized by this test.
