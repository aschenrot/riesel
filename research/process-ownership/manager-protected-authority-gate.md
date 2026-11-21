# #59 — Protected Manager authority and direct OS stop-bypass research

**Date:** 2026-11-21  
**Status:** decision-critical, independently reproducible research; **not** an accepted operating-system backend, privilege broker, public Manager API, or implementation authorization.  
**Integration baseline:** `main` at `8b6780436d6a6ce4b508f4ff9bc82310c05f2309`. Parent specification: [#59](https://github.com/aschenrot/riesel/issues/59). Shared operational coordination: [#75](https://github.com/aschenrot/riesel/issues/75).

## 1. The actual accepted security requirement

The owner [explicitly chose](https://github.com/aschenrot/riesel/issues/59#issuecomment-7062536105) **protection of privileged Manager commands against untrusted programs even when they run under the same OS user**. This includes preventing an unauthorized program from bypassing the Manager to stop/restart protected application units. [Other accepted decisions](https://github.com/aschenrot/riesel/issues/59) require **two application execution scopes** (user-session apps and machine services), a **disposable Manager controller**, CLI and local browser clients of **one protected management contract**, and first-release Linux/Windows/macOS parity (Linux routine CI only).

This creates **two independent security questions**:

1. **Command admission:** can an untrusted same-user process invoke a privileged Manager operation, impersonate an authorized caller, modify the durable registry, or reach the OS owner directly?
2. **Effect/target protection:** even if the Manager denies a request, can the same-user process **directly kill, replace, debug or modify** the managed application or its protected executable/configuration using ordinary operating-system rights?

A broker with secure command admission can satisfy question 1 while failing question 2 if the actual application still runs as an ordinary process owned by the attacker's OS account. **Do not label such an arrangement compliant with the full accepted requirement.**

Scope assumption for research: attackers have arbitrary unprivileged code execution as the desktop/user-session OS account, including local socket access, command execution and same-UID process APIs; they are **not root, the OS kernel, the installed protected broker principal or an OS administrator overriding enforcement**. Whether attacker control of the user's interactive display, signed executable directory, authentication agent or desktop session is in scope must be made explicit in the production threat model. Do not confuse same-UID protection with immunity to a fully privileged OS administrator.

## 2. Existing repo evidence — precise limits

| Source | Verified result | Still unproven |
| --- | --- | --- |
| [#94 real-systemd research](https://github.com/aschenrot/riesel/pull/94), [executed admission CI](https://github.com/aschenrot/riesel/actions/runs/44884335628) | `systemd --user` transient services survive controller exit, can be observed across controllers; an immutable per-attempt unit name prevents a stale **name-based** stop of its replacement; durable cooperative SQLite admission can suppress duplicates | Hostile same-UID programs can invoke user-manager methods or access a same-UID registry. No protected installed broker, no true cross-principal OS owner, no verified Riesel app readiness |
| [#96 real Linux peer/principal differential](https://github.com/aschenrot/riesel/pull/96), [executed CI](https://github.com/aschenrot/riesel/actions/runs/44886275238) | `SO_PEERCRED` identifies kernel peer UID; two independent processes with the same allowed UID both pass a UID-only policy. A distinct OS principal can protect service-owned state from a separate UID | An isolated server UID **does not authenticate which of two same-UID user clients is allowed** |
| [#99 real Riesel Todo OS-owner fixture](https://github.com/aschenrot/riesel/pull/99) | A research-only integration test exists on a stacked draft branch | The existing report does **not** establish a completed exact-head real-Riesel/systemd CI passing outcome. Do not assume app-level graceful stop or readiness from synthetic #94 results |
| **This differential**, [script](manager_direct_stop_principal_probe.py) | Tests actual Linux `SIGTERM` from independent unprivileged processes with distinct UIDs; one matching the victim's UID, one not | The local preliminary result must be reproduced by **exact-head repository CI** before it is counted as PR-verified evidence; no Riesel binary, browser, broker, AppArmor policy, Windows or macOS test is included |

### Direct-stop test

The script starts one **disposable isolated victim process** under numeric UID/GID `60081`. A foreign unprivileged process under UID `60082` sends `SIGTERM` and is denied with `EPERM`; an independent unprivileged process under UID `60081` sends `SIGTERM` and the victim **terminates without involving any Manager command endpoint**. Attacker subprocesses must have no effective Linux capabilities. The test creates **no permanent user accounts**, OS units, installed services, app data or privileged production code and targets **only its own subprocess**.

Reproduce on a disposable Linux runner with `setpriv` and either root or `sudo -n`:

```sh
python3 -m py_compile research/process-ownership/manager_direct_stop_principal_probe.py
python3 research/process-ownership/manager_direct_stop_principal_probe.py
```

Expected markers, *not a declaration of CI success*:

- `RESEARCH_MANAGER_CROSS_UID_DIRECT_SIGTERM_DENIED`
- `RESEARCH_MANAGER_SAME_UID_DIRECT_SIGTERM_BYPASSES_MANAGER`
- `RESEARCH_MANAGER_OS_SIGNAL_PRINCIPAL_DIFFERENTIAL_PASS`

The dedicated, path-scoped [Linux workflow](../../.github/workflows/manager-protected-owner-research.yml) runs on Ubuntu 24.04 without routine Windows/macOS jobs. It uses a bounded timeout and disposable processes only.

Linux's documented `kill(2)` permission rule allows signals when matching real/effective UID and grants elevated `CAP_KILL` authority under defined conditions: [Linux manual](https://man7.org/linux/man-pages/man2/kill.2.html). Therefore ordinary same-UID OS-owned apps are **not protected from arbitrary same-UID signal interference just because a Manager broker protects its own IPC**.

## 3. Platform OS authority — sources and unknowns

These are **documented API capabilities, not implementation equivalence or tested Riesel solutions**.

| Platform | Credible OS building blocks | Essential limit / real verification needed |
| --- | --- | --- |
| **Linux** | A root/system-manager-owned service/broker may launch explicitly configured lower-privilege per-app OS principals; systemd's system and user managers are distinct, and `User=` for system services is documented. polkit may mediate authorization of *privileged mechanisms*. See [systemd.exec](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec), [polkit](https://www.freedesktop.org/software/polkit/docs/latest/polkit.8.html), [pkcheck](https://www.freedesktop.org/software/polkit/docs/latest/pkcheck.1.html). | **User-owned** systemd services and `0600` files do not stop a same-UID attacker. An independently owned broker alone does not distinguish legitimate same-UID CLI/GUI callers; each privileged action needs defensible operator authorization. For *user-session apps*, prove that separate OS identity/confinement remains compatible with user desktop/session APIs. |
| **Windows** | Windows Service Control Manager service-object security descriptors and process-object DACLs control service actions and `PROCESS_TERMINATE`. An installed service with independently protected principal can mediate commands. See [service security](https://learn.microsoft.com/en-us/windows/win32/services/service-security-and-access-rights), [process security](https://learn.microsoft.com/en-us/windows/win32/procthread/process-security-and-access-rights), [service SID configuration](https://learn.microsoft.com/en-us/windows/desktop/api/winsvc/ns-winsvc-service_sid_info), [named-pipe client impersonation](https://learn.microsoft.com/en-us/windows/win32/ipc/impersonating-a-named-pipe-client). | A named-pipe client from the same Windows user security token is not automatically a trusted Manager operator. Verify **both** the service DACL **and** app process/file rights against same-user termination and replacement, installation/elevation policy, job/service lifetime, GUI session access, and exact-launch attempt fencing. A Windows job by itself does not establish crash survival or authorized command admission. |
| **macOS** | Service Management supports login items, launch agents and launch daemons, including `SMAppService` registration; launchd/XPC and Authorization Services can support privileged helpers. XPC has peer **code-signing requirement** APIs. See [Service Management](https://developer.apple.com/documentation/servicemanagement), [SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice), [XPC connections](https://developer.apple.com/documentation/xpc/xpc-connections), [Authorization Services](https://developer.apple.com/documentation/security/authorization-services), [Apple permissions](https://developer.apple.com/library/archive/documentation/Security/Conceptual/AuthenticationAndAuthorizationGuide/Permissions/Permissions.html). | Ordinary same-EUID processes can generally send signals; a privileged launch daemon does not automatically protect a same-user app process. Signing requirements protect a particular XPC peer only under properly enforced OS code identity and helper installation; a web browser frontend cannot automatically inherit that identity. Verify launchd domains, helpers, signed-client trust, per-op authorization and user-session restrictions on real macOS. |

## 4. Matched candidate comparison

| Candidate | Same-user command admission | Direct stop/restart bypass | Controller crash/reconcile | Cross-platform and operational cost | Disposition |
| --- | --- | --- | --- | --- | --- |
| **A. User-owned process + local socket, peer UID, `systemd --user` or readable token** | **Fails**: malicious user process can share caller UID/secrets | **Fails** when OS grants same user signals/service API | #94 proves bounded synthetic controller survival where user manager is available | Easy foreground/dev baseline; cannot satisfy accepted first-release security | **Reject as full protected installed mode**; may remain explicitly unprotected developer mode |
| **B. Protected broker / OS service but app runs as attacker UID** | Potentially, with **additional** real user/client authorization; a broker alone is insufficient | **Fails** unless further OS restrictions prevent same-user direct effects | Promising if app owner and durable attempts survive controller | Medium-high; broker installation/auth and still unsolved direct app protection | **Reject as complete solution**; useful component only |
| **C. Protected authority + independently protected app/service principal(s)** | Needs OS-backed operator reauthentication, isolated authorized client principal or equivalent verified identity; separate server UID alone is insufficient | Credible when attackers cannot act as app principal or control its service owner | OS-managed owner + immutable attempts + durable admission/reconciliation are compatible candidates | High initial cost: safe install/elevation, app data and GUI-session interoperability, cross-platform adapters | **Preferred research candidate**, NOT accepted implementation; must test user-session GUI feasibility and every OS |
| **D. Protected authority + OS-enforced per-app confinement/security domains under one human account** | Requires trustworthy process identity / enforcement policy beyond UID | Could be credible **only if** confinement blocks direct signals, process injection, files and service APIs | Similar durable owner and admission still required | High deployment and portability risk; LSM/sandbox/OS policy parity unverified | **Parallel comparator**, do not claim equivalent to C without a real test |
| **E. Prompt for operator approval at every privileged operation but app remains same UID** | May prevent *silent* commands if each prompt is OS-verified, though any same-user program may attempt to trigger it | **Fails direct-stop bypass** without protection of the target process | No independent process ownership guaranteed | Poor unattended-management usability; approval may not identify legitimate CLI/GUI | **Incomplete**, not full security boundary |

The comparison is **not a technology vote**. It identifies the missing proof in proposed designs. There may be additional credible OS-specific solutions; evaluate them with the same security, user-session operation, machine-service operation, crash reconciliation, installation, portability, accessibility and maintenance criteria.

## 5. Architecture constraints worth specifying before APIs

1. **Protected authority vs disposable controller vs app process owner** are three distinct concepts. The protected authority controls admission/registration/policy and delegates exact OS-owned launches; the Manager CLI/browser/controller can crash independently; the owner and app survive when healthy. A single compromised desktop process cannot replace the durable registry or bypass service ownership.
2. **Command permission and process immunity are separate assertions.** A trusted Manager service does not confer protection on child processes unless OS principals, process-object security, service ownership or an independently verified confinement mechanism enforce it.
3. **Operator authentication must not be a same-UID bearer secret.** Design interactive and unattended authorizations by exact operation/scope with OS-enforced provenance and explicit privilege escalation/install consent. Verify hostile same-UID attempts to imitate the GUI, re-use browser sessions, pass a forged PID or feed a stale command. Browser origin/CSRF defense is necessary but **not an operator authentication substitute**.
4. **Provenance is part of the security boundary.** Verified, protected app binaries, launch configuration, delegated OS owner and installed service definitions must not be editable/substitutable by the hostile caller. Protect stateful app-owned data and reject untrusted path/symlink swaps across all install/update/uninstall paths.
5. **Fail closed, preserve uncertainty.** On owner unavailability, trust-verification failure, service disablement or crash windows, do not silently fall back to same-user subprocess launch or reveal write control through a developer socket. Use immutable per-attempt identifiers and owner evidence before stop/restart. Separate OS attempt from Riesel runtime session generation and published-interface callability (#75).

## 6. Next discriminating tests (the technical acceptance gate)

Design paired **allowed and forbidden** cases for all three platforms and both scopes. Do not freeze public wire/API schemas until this evidence exists.

| Gate | Real negative test | Real positive/control test | Release consequence |
| --- | --- | --- | --- |
| **M-S0** Linux raw process authority | Same-UID, no-manager stop of a same-user process succeeds; separate UID cannot send SIGTERM | Disposable process fixture runs without privileged attacker capabilities | Baseline that disproves assumption A/B; does **not** prove C |
| **M-S1** Protected process owner | Hostile same-user process attempts direct OS stop/restart, process termination, service control, executable/config replacement | Authorized Manager operator changes exact registered attempt; owner survives Manager-controller death | **Must** pass for both user-session and machine-service scopes per OS |
| **M-S2** Protected command admission | Untrusted same-UID process invokes register/start/stop/uninstall through CLI/backend/socket/browser and attempts credential reuse or forged signed identity | Legitimate authorized human or explicitly provisioned automation succeeds under a defined, auditable per-op authorization path | A separate principal/service without protected client admission **fails** |
| **M-S3** Attempt fencing | Old controller races stop with new launch; lost response, reserve-before-launch crash, delayed OS start, owner unavailable | New controller securely reconciles exact OS attempt/registration and refuses uncertain duplicate or stale stop | Must preserve controller-survival and no-duplicate accepted requirements |
| **M-S4** Artifact and stateful update | Same-user attacker edits/symlink-swaps artifact or update source, attempts deleting shared data; update breaks DB compatibility | Trusted install/update/rollback only on proved compatibility; uninstall preserves data by default | No blind rollback, migration or data removal |
| **M-S5** Real OS + user experience | User-session GUI cannot attach to its assigned protected owner, no-login service unexpectedly dies, browser has same-UID privilege bypass | Both app execution scopes actually run; local CLI/browser status and protected controls work | **Linux routine CI**; Windows/macOS **manual real-OS** first-release gates |

**Stop/go rule:** If an authentic user-session GUI app cannot both interact with the session and be insulated from a malicious sibling OS process on a required platform, #59 must explicitly record that conflict and **revise the mechanism, app execution/scope model or product requirement with owner approval**. Do **not** silently substitute a weaker “trusted local user” profile and call the first Manager release complete.

## 7. Proposed next engineering slice

Continue research in **#59**, not a new implementation issue, with **M-S1 and M-S2**: implement a disposable **Linux system-manager-owned protected broker + independent app principal** experimental path, test direct `kill` and service-manager bypass, and compare legitimate per-command authorization methods for CLI and browser, while preserving no installed permanent service on CI. Reuse #94's immutable attempt/durable admission method only where its cooperative threat model and limitations are explicitly stated. In parallel, document Windows Service DACL/process access and macOS XPC/launchd feasibility **without claiming tests**.

**No technical OS owner or protected auth mechanism has yet been selected by this research.** This research cannot close #59/#75, authorize M-A..E implementation, or merge draft PR #86 on its own.
