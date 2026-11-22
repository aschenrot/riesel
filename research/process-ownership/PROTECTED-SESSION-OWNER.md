# #59 — Protected logind-session and machine-service Riesel owner research

**Status: bounded Linux research EXECUTED and PASSED** at source revision 076da2606d75849de9cc812bfc6316b7bdbee27b: [actual Ubuntu PAM/logind + protected Riesel CI 44972242575](https://github.com/aschenrot/riesel/actions/runs/44972242575). Final report-head CI must also pass separately. These are research facts, not production Manager acceptance. Primary [Manager #59](https://github.com/aschenrot/riesel/issues/59); shared identity [#75](https://github.com/aschenrot/riesel/issues/75). This is research-only, not an accepted Manager backend, process-owner policy, GUI framework or API.

## Original requirement

The owner accepted both **user-session** and **machine-background** applications in the first installed Manager release. User-session logout **may** terminate a session-bound app; machine services must operate without a logged-in operator. Privileged management actions must reject unauthorized processes even under the same operating-system user. Linux, Windows and macOS remain first-release platforms with real manual Windows/macOS release proof.

Existing evidence is insufficient: [PR #99](https://github.com/aschenrot/riesel/pull/99) demonstrated real Todo under a systemd user manager but not protected hostile same-UID process ownership. [PR #116](https://github.com/aschenrot/riesel/pull/116) executed a same-UID direct-stop bypass. [PR #134](https://github.com/aschenrot/riesel/pull/134) demonstrated a real separate native GUI controlling a different-UID, OS-owned Riesel Counter, but not that the owner was bound to a real login session or that login termination would affect only the session app. [#139](https://github.com/aschenrot/riesel/pull/139), [#142](https://github.com/aschenrot/riesel/pull/142) and [#143](https://github.com/aschenrot/riesel/pull/143) separately tested protected command admission, OS credential/desktop input limits and durable unknown-result reconciliation.

**Question:** Can a system-owned real Riesel app run as a separate kernel UID **and** follow an exactly observed actual PAM/logind session scope, while another separately owned Riesel app remains independent of that login?

## Falsifiable actual-Linux experiment

Run on disposable Ubuntu 24.04 with real systemd PID 1 and logind. Build the **unmodified** pinned Riesel Counter from PR #90 at SHA 5201e61cc74bab778dfc63a3788f1708b2530f21. The [dedicated workflow](../../.github/workflows/manager-protected-session-owner-research.yml) executes [the probe](manager_session_bound_owner_probe.py).

1. Create one random, disposable Linux login account and start a transient system service with PAMName=login. Record the **real** session returned by loginctl: UID, PAM service, session ID and session-*.scope. **This is non-graphical**: it does not establish an X11/Wayland session, GUI permissions or a logged-in real operator.
2. Start two separate **real Counter binaries** from root-owned random installation paths, each under a different systemd DynamicUser principal. One service explicitly has BindsTo=<exact observed session scope> and After=<scope>; the other has **no** such session dependency. Observe actual Rust binary process PIDs and kernel UIDs, not merely shell MainPID, plus actual service InvocationIDs.
3. Execute an independent process under the disposable login user's actual UID with zero effective Linux capabilities and no sudo rights. It must fail to read/modify a populated root-only registration, directly SIGTERM the session-bound Counter, or issue an unapproved systemd system-manager stop. After each attack the old protected OS launch attempts must remain running.
4. A separately privileged **test** operator terminates only the exact disposable login session with loginctl. The dependent actual Counter must stop; the machine-background Counter must remain running with the same invocation identity and PID/start-time.
5. Cleanup only the generated session, account, uniquely named transient services, installation directory and protected registration.

**Never claim** the test coordinator is an authorized human Manager client. GitHub runner UID has broad sudo and is not the attacker; the lower-privilege login UID is the negative principal. No normal real application is modified.

## Executed real OS test and multi-login critical extension

The disposable Ubuntu 24.04 test **PASSED** on code SHA 076da2606d75849de9cc812bfc6316b7bdbee27b in [dedicated research run 44972242575](https://github.com/aschenrot/riesel/actions/runs/44972242575). The initial [CI run 44972048973](https://github.com/aschenrot/riesel/actions/runs/44972048973) failed **before examining logind** because a Type=exec PAM-backed systemd-run launch blocked the initiating process beyond its timeout. That was a **fixture failure**, not evidence against the architecture. The corrected fixture submits the real PAM job with systemd-run --no-block, observes an actual session scope independently and cleans exact disposable units even after failed launches.

**Original one-login test:** Real PAM/logind session ID, actual account UID and Scope were observed. Two real unmodified Riesel Counter binaries launched under distinct DynamicUser=yes identities, one with exact BindsTo/After dependency on the observed session scope, the other a machine-background service with no such dependency. They had independently observed actual Counter PIDs/start ticks and OS InvocationIDs; shell-wrapper MainPID was not mistaken for the app process. A separate test attacker under the login UID, with zero effective Linux capabilities and no sudo, was denied actual SIGTERM, systemd system-manager stop and root-only registration access. Terminating the bound session stopped only its Counter; the background Counter remained running with the original OS attempt.

**Decision-critical strengthened case:** Create **two simultaneous genuine PAM/logind sessions with the same UID but different session scopes**. The protected Counter is bound only to session A. Terminating the unrelated session B leaves *both* Counter processes, exact PIDs/start ticks and OS InvocationIDs unchanged. Terminating session A stops only the bound Counter; the independent machine-background Counter remains active. The successful CI emitted:

- RESEARCH_M59_ACTUAL_LOGIN_SESSION_SCOPE_OBSERVED
- RESEARCH_M59_SAME_HOST_TWO_REAL_RIESEL_EXECUTION_SCOPES
- RESEARCH_M59_SESSION_SAME_LOGIN_UID_DIRECT_MUTATION_DENIED
- RESEARCH_M59_UNTRUSTED_LOGIN_UID_UNCHANGED_ATTEMPTS
- RESEARCH_M59_TWO_PARALLEL_SAME_USER_LOGIN_SCOPES
- RESEARCH_M59_UNRELATED_LOGIN_LOGOUT_LEAVES_BOTH_APPS_ALIVE
- RESEARCH_M59_LOGIN_SESSION_END_STOPS_ONLY_BOUND_RIESEL_APP
- RESEARCH_M59_PROTECTED_SESSION_BOUND_OWNER_RESEARCH_PASS
- RESEARCH_M59_TWO_PARALLEL_SAME_USER_LOGIN_SCOPES
- RESEARCH_M59_UNRELATED_LOGIN_LOGOUT_LEAVES_BOTH_APPS_ALIVE

**Interpretation:** Linux can compose a separately OS-owned application process with **an exact single login-session lifetime** while leaving an independent machine-background app running. That is stronger than “some session under this UID is active” and supports a future explicitly scoped owner design.

**Important exclusions:** Both login sessions are generated non-graphical PAM/logind sessions, not real graphical human logins, user-seat/display ownership or a tested Wayland/X11 session. The attacker is a separately spawned process under the same login **UID**, not proven attached to the actual PAM session cgroup. The coordinator's runner UID has broad sudo and is excluded as hostile. No user-session GUI capability export, login-event authentication, logout policy from an actual user, lifetime across rapid logout/relogin, managed SQLite data ownership, graceful Riesel runtime stop, command journal/atomic OS stop, or Windows/macOS equivalent is proved. This fixture only tests a legitimate root test-operator login termination, not protected human CLI/browser authorization.

## Competing architectures and long-term risks

| Candidate | Session lifecycle | Same-UID resistance | Architectural limitation |
| --- | --- | --- | --- |
| systemd user service | Directly integrated with a user manager; can linger after logout | Ordinary same-user process has signal and user-bus control unless additional OS-enforced isolation is verified | Does not meet protected first-release owner guarantee by itself |
| Separate-UID system service tied to exact logind session scope | Potentially supports explicit per-session logout while an OS owner protects actual app PID | Different app UID denies ordinary same-login-UID signals; service permissions still need protected authorization | Must prove precise per-session attachment, data/GUI access, parallel sessions, safe logout/relogin and platform-specific equivalents |
| Protected isolated owner with no session dependency | Continues without any user login, as a background service | Different app UID provides a defined process boundary | Fails user-session logout/lifecycle requirement if used indiscriminately |
| Mandatory access control under the same UID | Potentially preserves desktop UID and direct OS session affiliation | Conditional on independently enforced, non-self-selectable trust domains | AppArmor [#129](https://github.com/aschenrot/riesel/pull/129) only established a bounded profile; cannot assume every supported OS has equivalent confinement |
| UI attached to machine-owned app | Real native GUI calling OS-isolated Counter was shown in #134 | GUI-side input/client authorization still weak in previous experiment | This is not sufficient proof of login-session-bound application semantics |

Official systemd documentation: [PAMName session creation](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html), [BindsTo with After](https://www.freedesktop.org/software/systemd/man/latest/systemd.unit.html), [login session/linger differences](https://www.freedesktop.org/software/systemd/man/latest/logind.conf.html), [pam_systemd and XDG_RUNTIME_DIR](https://www.freedesktop.org/software/systemd/man/latest/pam_systemd.html).

### Explicit gaps

- The generated PAM session is **not** an actual operator desktop/seat. No X11/Wayland display, authenticated native GUI input or stable application export is tested.
- The user may terminate their own session, legitimately causing logout and ending session-bound applications; protection from **unauthorized Manager stop** and absolute app availability during deliberate logout are different guarantees.
- A normal same-user process holding broad ambient sudo can bypass root process protection. The actual runner UID has sudo; it is excluded as an adversary.
- No stable data ownership/SQLite UID mapping, managed installation/uninstall, multiple simultaneous sessions, cross-session concurrency, runtime session epochs, app graceful drain, readied capability export, or controller-restart command journal is tested here.
- Windows/macOS require their own real session/OS owner and protected authorization validation. The fixture cannot justify a portable backend or installation mechanism.

**Stop/decision rule:** A passing Ubuntu CI proves only the observed login scope, separate principal and lifetime-coupling claims, *not* first-release protected user-session functionality. If PAM/logind prerequisites cannot create a real scope, mark the test **blocked**, not a positive or negative product guarantee. No interface, trusted prompt, concrete process-owner backend or implementation issue is accepted merely from a green research job.

## Reproduce (disposable Linux host only)

    python3 -m py_compile research/process-ownership/manager_session_bound_owner_probe.py
    timeout 150 python3 research/process-ownership/manager_session_bound_owner_probe.py --exercise "$PINNED_COUNTER_ROOT"

Verified research markers at SHA 076da260, run 44972242575 (full proof and limits above):

- RESEARCH_M59_ACTUAL_LOGIN_SESSION_SCOPE_OBSERVED
- RESEARCH_M59_SAME_HOST_TWO_REAL_RIESEL_EXECUTION_SCOPES
- RESEARCH_M59_SESSION_SAME_LOGIN_UID_DIRECT_MUTATION_DENIED
- RESEARCH_M59_UNTRUSTED_LOGIN_UID_UNCHANGED_ATTEMPTS
- RESEARCH_M59_LOGIN_SESSION_END_STOPS_ONLY_BOUND_RIESEL_APP
- RESEARCH_M59_PROTECTED_SESSION_BOUND_OWNER_RESEARCH_PASS
