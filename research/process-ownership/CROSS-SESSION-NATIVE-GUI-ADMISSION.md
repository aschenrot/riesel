# #59 / #60 / #75 — Native GUI access across two real logind sessions

**Status:** bounded Linux integration/security research, **not yet executed**. This is **not** a production process-owner backend, chosen application-export API, user authorization, desktop login implementation or acceptance of a GUI framework.

Primary [Manager #59](https://github.com/aschenrot/riesel/issues/59), [native GUI #60](https://github.com/aschenrot/riesel/issues/60), coordination [shared operational identities #75](https://github.com/aschenrot/riesel/issues/75). Relevant independent sources: the real, unchanged SDK-resolved Counter + eframe GUI in [PR #90](https://github.com/aschenrot/riesel/pull/90), Linux protected OS-owned Counter and same-UID X11 injection in [PR #134](https://github.com/aschenrot/riesel/pull/134), actual two-login-session lifecycle and differently OS-owned Riesel services in [PR #145](https://github.com/aschenrot/riesel/pull/145), and polkit/input-origin research in [PR #142](https://github.com/aschenrot/riesel/pull/142). No duplicate native GUI or protocol is introduced.

## Product outcome and falsifiable question

Riesel already accepts a separate native GUI process for a headless application, and Manager requires explicit **user-session** versus **machine-service** lifecycle scopes while protecting privileged process controls from untrusted same-UID processes. The GUI itself may legitimately be low privilege, but the app's exported capabilities must have an explicit **access policy**; merely identifying an OS login session is not equivalent to authorizing a particular client or operation.

Can the **actual unmodified native GUI** run inside two different real Linux PAM/logind sessions of the **same account**, use actual X11 windows and typed Riesel RPC against one **separately OS-owned Counter bound to session A**, and preserve exact logout semantics? Does the current unauthenticated loopback transport accidentally allow a GUI in *unrelated* session B to mutate A's application **without any explicit grant**, despite correct process ownership and session association?

A second session's ability to call the Counter is an **admission counterexample for applications requiring same-session-only access**, not proof that all cross-session clients must be rejected. Sharing with other sessions can be an explicit legitimate application policy; no universally denied session-by-default public API is selected by this research.

## Experimental architecture and provenance

- Research itself runs from current main on disposable GitHub Ubuntu 24.04, real systemd PID 1, real PAM/logind and **Xvfb**. It verifies exact SHA of existing [PR #90](https://github.com/aschenrot/riesel/pull/90) at 5201e61cc74bab778dfc63a3788f1708b2530f21 and [PR #145](https://github.com/aschenrot/riesel/pull/145) at c59fb50b398b16cae47de4a616ca2fc486df38b3. It reuses their original source and verified logind helper **without edits**, building the actual SDK-resolved Counter and real eframe/winit native GUI.
- Create one disposable Linux login UID with no sudo or Linux capabilities, install a copy of the real Counter/GUI and a narrow [Python exec launcher](manager_cross_session_gui_launcher.py) into a root-owned, uniquely prefixed research-only location. No production Manager or application source is changed.
- Start an actual PAMName=login service A with the fixture launcher initially waiting for the backend endpoint, discover its real logind session ID, UID and scope. Once an OS-owned Riesel Counter is started with distinct DynamicUser principal and exact BindsTo/After dependency on A's scope, publish its deliberately **unauthenticated** TCP loopback endpoint into a disposable file readable by both GUIs. The launcher then execs the **original** native GUI in the same PAM session process, rather than spawning an unrelated process. Independently verify GUI PID/executable, kernel UID, actual logind cgroup, created native X11 window and initial typed Counter value.
- Drive Ctrl+I through the **actual X11 window** in A: Counter changes from 0 to 1 through original generated typed RPC, without moving the application runtime into the GUI.
- Start second independent PAMName=login service B under the **same OS UID**. Require distinct actual logind scope, its own unchanged native GUI executable/PID/cgroup and another real X11 window. Give it only the deliberate research endpoint address; **no app-issued client capability, credential, Manager credential or OS root permission is provided**. Drive Ctrl+I in B and verify both B and A observe authoritative value 2. The backend's separately owned process/OS InvocationID remains the same.
- Terminate **only session B** using a root **test** coordinator: GUI B must exit; GUI A and session-A-bound Counter must remain alive under unchanged PID/attempt. Drive Ctrl+I in GUI A again and observe value 3. Terminate **session A**: both the A GUI session process and its bound actual Counter must stop. No automatic replacement launch is allowed.
- Cleanup strictly the test-only account, two PAM sessions, uniquely named systemd units, root-owned disposable app directory and temporary private Xauthority file; no other system services or user accounts are modified.

The CI X11 server is owned by the GitHub runner and uses a test-only MIT-MAGIC-COOKIE. The disposable account cannot read the runner's original Xauthority file, so the fixture **provisions an independent, temporary cookie to that test UID** and proves a real X11 connection using xdpyinfo before native GUI startup. **Both test login sessions deliberately share this cookie**; it is an X11 display-access prerequisite, not an app-authentication credential or a trusted desktop isolation boundary. The two real PAM/logind sessions are **not** display-manager-authenticated graphical logins, physical seats or independent trusted Wayland compositors, even though each launches a genuine native window. Neither the X11 input events nor the root runner's privileged test actions prove a human approved anything.

## What the outcomes would mean

| Claim / gate | What this fixture can directly test | What it cannot establish |
| --- | --- | --- |
| Real independent GUI | Actual unchanged eframe/winit native window/event loop in each distinct PAM/logind session cgroup; keyboard produces genuine typed SDK calls | Desktop display manager, native toolkit selection or production GUI app |
| Protected app OS owner | Actual separately owned Counter binary, OS service InvocationID, exact login scope dependency | Manager human approval, protected exported capability IPC, database identity |
| Parallel-session policy | Session B GUI can mutate app A's RPC if only an endpoint address is known | A universal claim that every cross-session operation should be denied or that the attacker obtained an unseen address |
| Exact logout | B logout preserves A GUI and backend; A logout stops both A GUI and bound backend | Multiple seats/Wayland security, machine reboot, simultaneous logout races, power failure |
| Security | No root credential or privileged Manager control is needed to mutate app RPC in the expected **negative control** | Breaking into a correctly authenticated application channel; stopping the OS-owned app or bypassing an installed Manager |

**Architecture candidates (not accepted):** (A) all app exports always share same-UID loopback, simplest but cannot express same-session/explicit-client permission; (B) producer-owned versioned, granted capability export with explicit caller principal/session/operation and revocation, using OS-local IPC or another authenticated channel; (C) protected Manager intermediary for every GUI operation, potentially overcoupling app UI to an installed Manager and misplacing app-owned authorization; (D) per-user-session exports with owner-assigned scoped endpoints, which may improve isolation but require verified enforcement and a deliberate cross-session-sharing mechanism. Assess usability, principal attestation, client lifecycle, last-known session generation, migration/compatibility, revocation, platform equivalence and operational costs before selecting; do not hardcode an assumed exporter or transport in a research fixture.

**Critical independent obligations:** Distinguish (1) registered application identity, (2) OS executable/install and launch attempt, (3) login session scope, (4) actual Riesel runtime session generation, (5) graphical frontend process identity, and (6) a deliberately authorized exported capability grant. X11 input injection under [PR #134](https://github.com/aschenrot/riesel/pull/134) and polkit under [PR #142](https://github.com/aschenrot/riesel/pull/142) mean a mouse/keyboard event or password prompt inside an ordinary X11 session does **not** guarantee trusted human intent. Windows/macOS cannot be inferred from this Linux-only fixture. No app-level authorization API, protocol, public client grant or release promise follows automatically from this investigation.

**First executable fixture result (not a security finding):** [Ubuntu research run 44974110169](https://github.com/aschenrot/riesel/actions/runs/44974110169) built both unchanged Riesel binaries and observed an actual session A and separately owned Counter, but **FAILED before GUI startup**: the account had no Xauthority credential and Xvfb denied the GUI's connection. This was a **test-display setup failure**, not evidence for either cross-session admission policy. The fixture now provisions a disposable Xauthority cookie and requires a real xdpyinfo connection test before counting a GUI result. Do not infer a new result until rerun at a tested source revision.

## Reproducible Linux research

The dedicated [workflow](../../.github/workflows/manager-cross-session-native-gui-research.yml) builds both original pinned real binaries and executes [the main probe](manager_cross_session_native_gui_probe.py) and [the disposable PAM GUI launcher](manager_cross_session_gui_launcher.py) under actual Xvfb/systemd/logind. Execute only on disposable hosts with permitted root fixture setup and a trusted test X server.

Expected markers are **test assertions, not verified claims until CI passes**:

- RESEARCH_M59_PINNED_NATIVE_GUI_AND_SESSION_SOURCE_VERIFIED
- RESEARCH_M59_DISPOSABLE_X11_COOKIE_ADMISSION_VERIFIED
- RESEARCH_M59_REAL_GUI_PAM_SESSION_A_SCOPE
- RESEARCH_M59_REAL_COUNTER_PROTECTED_EXACT_GUI_SESSION_BOUND
- RESEARCH_M59_REAL_SESSION_A_NATIVE_GUI_MODIFIED_COUNTER
- RESEARCH_M59_CROSS_LOGIND_SESSION_UNAUTHENTICATED_GUI_MUTATED_APP
- RESEARCH_M59_UNRELATED_GUI_LOGOUT_PRESERVES_APP_A
- RESEARCH_M59_BOUND_GUI_LOGOUT_TERMINATES_EXACT_COUNTER_APP
- RESEARCH_M59_REAL_CROSS_SESSION_NATIVE_GUI_SECURITY_RESEARCH_PASS
