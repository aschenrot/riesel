# #59 M-S1b — Genuine Riesel Counter native GUI under separate Linux system-service owner

**Date:** 2026-11-22. **Status:** reproducible **research only**, not a protected Manager service, accepted OS adapter, authorization protocol or release. [Manager #59](https://github.com/aschenrot/riesel/issues/59), [GUI #60](https://github.com/aschenrot/riesel/issues/60), [shared application operational contract #75](https://github.com/aschenrot/riesel/issues/75).

## Why this investigation is not another GUI implementation

[#90](https://github.com/aschenrot/riesel/pull/90) already proved an **actual** headless Riesel Counter application with SDK-resolved `AsyncCounterContract` and an independent native eframe GUI process under Linux Xvfb on **exact source `5201e61cc74bab778dfc63a3788f1708b2530f21`** ([native GUI CI #44877911293](https://github.com/aschenrot/riesel/actions/runs/44877911293)). That proof uses **unprotected same-user processes and unauthenticated loopback RPC**. [#123](https://github.com/aschenrot/riesel/pull/123) independently proved a different-UID transient Linux system-service target can resist unprivileged process signals/OS control, but its app was the stock `sleep` executable, not a real Riesel GUI-driven application.

The present gap is whether a **real SDK-resolved Riesel app** remains callable by an **actual independent native GUI process** while OS process ownership lives under a different, protected app principal, and how far the already-accepted same-user threat boundary still fails.

### Baseline and repeatability

The path-scoped [CI workflow](../../.github/workflows/manager-native-gui-os-owner-research.yml) uses **Ubuntu 24.04** with real systemd PID 1 and a real X11/Mesa Xvfb display. It fetches **GitHub PR #90 head** and hard-checks the fetched full Git SHA against `5201e61cc74bab778dfc63a3788f1708b2530f21`. It adds that source as a **detached worktree**, never merges it into the current research PR or `main`, builds the unchanged original `counter_resolved_facade_host` and the independent eframe GUI, and runs the original `research/native-gui-host/exercise.py` unchanged as a baseline.

Then [the independent probe](manager_native_gui_os_owner_probe.py) runs **the same native app + GUI binaries** in a separate protected OS-owner scenario. This preserves the existing code as the comparator, not a second GUI/framework/RPC implementation.

## Disposable protected owner fixture

1. A Linux CI coordinator `sudo -n` installs **only a copy of the real built Counter host binary** into a randomly named root-owned temporary `/tmp/riesel-m59-native-owner-<12hex>` directory, with ownership/mode checks. No Riesel binary is modified.
2. A separate short-lived launcher uses the **real OS system manager**, `systemd-run --system`, to start a transient service under a **distinct temporary `DynamicUser=yes` principal**. It then exits abruptly with code 23. The test controller independently checks `ActiveState`, `MainPID`, `InvocationID`, actual kernel UID and actual Counter app PID. The user-session GUI remains under the CI runner's desktop UID.
3. **Existing fixture shutdown limitation:** the exact pinned #90 Counter host reads its `STOP` command from stdin, and EOF immediately fails its handler. For this narrow test, an OS-owned shell pipeline `sleep 600 | counter_resolved_facade_host` **keeps stdin open** after the initiating controller dies. **This deliberately does not test graceful app drain, production Riesel shutdown/lifetime semantics or independent management authorization**. The final test-only systemd `stop` terminates the entire service cgroup. The observed systemd `MainPID` is a shell wrapper; the probe therefore also identifies the actual Rust app process independently.
4. A non-root process under the GUI's same OS UID, with zero effective capabilities, attempts real `SIGTERM` directly against the Counter PID and actual `systemctl --system stop` **without sudo**, which must fail and leave `MainPID`, `InvocationID`, `ActiveState` and actual app PID unchanged. Since the coordinator account has **ambient `sudo -n`**, another process under that account can also invoke `sudo`; [#123](https://github.com/aschenrot/riesel/pull/123) already tested and proved that bypass. Thus denial without sudo is **not hostile-same-UID-proof privileged Manager authorization**.
5. In the same Xvfb display, the **original #90 eframe UI** opens, receives actual X11 `Ctrl+I` keyboard input, and increments the system-owned Counter through SDK-resolved typed RPC (0→1→2→3); it closes without ending the system service.
6. A second independent **ordinary same-UID** GUI process opens with the *same learned loopback endpoint*, obtains authoritative Counter value 3 and increments 4→5. This is a **negative exported-capability authorization test**: the app-facing TCP transport has **no caller admission or session attestation** and can be used by arbitrary code that knows the address. It does **not** mean the second GUI gained privileged OS process ownership or the ability to change Manager registration.
7. All processes and OS objects remain temporary. The probe cleans up the exactly named transient service and root-owned fixture, not a real Manager, installed package, storage, OS account or user capability. The workflow keeps independent Linux `cargo validate --check` and `cargo smoke` as a **separate exact-head PR gate**.

## Evidence markers — **not verified until exact-head CI executes**

- `RESEARCH_M59_PINNED_NATIVE_GUI_SOURCE_EXACT_HEAD`: #90 source fetched and SHA checked, rather than reconstructed or silently modified
- `NATIVE_GUI_RESEARCH_XVFB_PASS` and `RESEARCH_M59_REUSED_NATIVE_GUI_UNMODIFIED_BASELINE_PASS`: actual original eframe GUI/Counter benchmark
- `RESEARCH_M59_REAL_RIESEL_APP_SYSTEM_OWNER_SURVIVED_LAUNCHER_EXIT`: real Counter app under independent system service after launcher exits
- `RESEARCH_M59_REAL_COUNTER_APP_PID=...`: verify kernel principal of **actual Rust app** rather than shell wrapper
- `RESEARCH_M59_SAME_DESKTOP_ACCOUNT_DIRECT_APP_SIGTERM_DENIED` and `RESEARCH_M59_SAME_DESKTOP_ACCOUNT_SYSTEM_SERVICE_STOP_DENIED`: Linux OS target isolation against two direct no-sudo operations, not broad same-UID `sudo` bypass
- `RESEARCH_M59_REAL_NATIVE_GUI_KEYBOARD_RPC_TO_SYSTEM_OWNER_PASS` and `RESEARCH_M59_NATIVE_GUI_CLOSE_PRESERVES_SYSTEM_OWNER_PASS`: real native UI interaction with a distinct-UID app
- `RESEARCH_M59_UNAUTHENTICATED_SAME_UID_GUI_CAN_MUTATE_COUNTER`: deliberate negative app-exporter call admission proof
- `RESEARCH_M59_PROTECTED_SYSTEM_APP_NATIVE_GUI_INTEGRATION_PASS`: **bounded OS-owner/UI interop** test only

## Strict acceptance limits

| Intended first-release property | What this specific fixture can establish | Still open |
| --- | --- | --- |
| Independent real native GUI | Pinned real #90 GUI/Counter keyboard behavior | Wayland, macOS/Windows UI and accessible AT-SPI/UIA |
| Process-owner independence | Real transient `systemd --system` service outlives initiating launcher | Controller reconciliation and durable registration/unknown outcome; app-level graceful stop |
| OS process target isolation | Different-UID app rejects direct unprivileged user signal and no-sudo system-service command | **Same-operator-UID `sudo` privilege bypass**, OS-admin attack, installer/policy authority |
| User-session application ownership | GUI can run in user session and interact with a **machine-level** owned app | A genuine **user-session-scoped managed app**, logout/login semantics, Wayland session/DBus/GUI notifications, desktop ownership permissions |
| App capability publication | Existing SDK-resolved Counter façade accessible across OS UIDs over localhost | **Authenticated app identity, exact runtime generation/ready state, authorized attachment, revocation and lost-reply mutation** |
| Protected Manager API | Nothing: probe uses privileged CI commands to create and stop synthetic systemd test unit | **Trusted human intent**, local-browser origin/CSRF, legitimate vs malicious same-UID management, explicit per-operation target/attempt, replay and unattended policy |
| Cross-platform | Ubuntu 24.04 systemd + Xvfb only | Real manual Windows/macOS OS-owner + GUI/consent release tests |

**Critical security distinction:** UI callability is **not** UI authorization. The earlier app exporter #60/#75 needs independently protected application/session/capability attachment; the Manager's privileged OS registration/control contract is *separately* #59. The unauthenticated loopback address is a deliberately insecure reference fixture, not a recommended production transport.

## Decision-critical next gates

- **M-S1b genuine user-session app mode:** use the actual app and GUI with an independently protected **user-session** owner, enforce logout/session-lifetime semantics and desktop resource access without exposing owner control to arbitrary same-user processes. Compare separate protected principal with OS MAC confinement and user-session delegation. Verify with real X11/Wayland/DBus variants and practical login, reboot and recovery.
- **M-S2 protected operator authority:** actual protected OS mechanism must grant one intended and deny one malicious **same-UID** CLI/browser command for the **same** app/action/launch attempt, including executing the trusted binary, stolen/shared UI data, unconfined attacker access, polkit broad allow, spoofed prompts, and replay.
- **M-S3 actual attempt fencing:** OS owner survives controller crash; after exact-attempt reconnection, stale stop and lost-acknowledgment cannot terminate a newer app launch.
- **#60/#75 exporter:** match actual app/session/version identity, explicit callable readiness, authenticated attach, explicit capabilities, unknown mutation outcomes and revocation without sharing the Manager's privileged OS-control authority.
- **Platforms:** no first-release guarantee until Windows and macOS native host, protected owner and authorization are tested on real OSes; Linux remains routine CI.

**Disposition:** no production backend, runtime API, security architecture, GUI framework choice or permission downgrade is accepted. Report the actual CI result and any fixture-specific limitations. Keep #59/#60/#75 open, and the research PR draft until reviewed.
