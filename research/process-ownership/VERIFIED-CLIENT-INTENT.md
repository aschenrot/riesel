# #59 — Verified executable identity is not authorized same-UID operator intent

**Date:** 2026-11-21  
**Baseline:** current `main` at branch creation; [#59](https://github.com/aschenrot/riesel/issues/59) and [#75](https://github.com/aschenrot/riesel/issues/75).  
**Status:** security research, **not** an accepted OS authorization mechanism or installed Manager. Actual GitHub CI results must be checked against the exact PR head before considering this executed evidence.

## Original first-release requirement

The owner accepted a **single-host**, Linux/Windows/macOS installed Manager that controls both **user-session apps** and **machine background services** through one authenticated, protected CLI/local-browser management contract. Privileged mutations must be denied to arbitrary untrusted programs **even when they run under the same human user's OS account**. Manager-controller crash independence, protected OS process owner, immutable attempts, update/uninstall provenance and honest unknown outcomes are separately mandatory.

Experiments already established:

- [#96](https://github.com/aschenrot/riesel/pull/96): kernel `SO_PEERCRED` sees actual UID/PID but cannot distinguish two programs sharing the allowed UID.
- [#116](https://github.com/aschenrot/riesel/pull/116): a program sharing the OS UID of a target process can directly stop that unprotected process without calling Manager.
- [#123](https://github.com/aschenrot/riesel/pull/123): a protected separate-UID system-service **target** can resist ordinary different-UID signals and system manager calls; **ambient `sudo -n`** shared by the operator UID remains callable by arbitrary code under that UID. Real trusted-detail polkit rules enforce user/target/verb but allow **both** same-UID client processes if the rule authorizes the user.

This follow-on tests a different proposed security control: **could a root-owned, verified, unchanged Manager CLI executable serve as the authorization identity?** It deliberately separates **trusted executable provenance** (useful supply-chain check) from **authorized operator intention** (a different guarantee).

## Experiment: real root-owned binary, actual Linux peer credentials

Fixture: [manager_verified_executable_intent_probe.py](manager_verified_executable_intent_probe.py), run in a [disposable Ubuntu 24.04 workflow](../../.github/workflows/manager-verified-client-intent-research.yml).

- Compile a small native C `manager-cli` and a separate attacker-owned `LD_PRELOAD` shared library. The native CLI sends the single **mock** request `STOP` to a local Unix socket. No real Manager, registered process or systemd service is modified.
- Install **only** the test CLI and root-side Python mock verifier in a uniquely named **root-owned directory**, with read/execute permissions but no write permission for ordinary user code. Verify UID, file mode, inode and SHA-256 of the unchanged executable.
- The independently privileged root-side server uses **actual kernel `SO_PEERCRED`** to obtain the connecting process PID/UID/GID. While each client stays connected, it checks the **observed `/proc/PID/exe` target device/inode** against the verified root-owned CLI and rechecks the digest. It replies `ALLOW` only if the peer UID and unchanged executable match, and the request is the expected mock `STOP`. This is an intentionally **naive executable allowlist**, not a production race-free attestation or proof of signing trust.
- Run one same-UID **direct Python client** (wrong executable), then one intended normal CLI invocation, one separate attacker-initiated **re-execution of that exact trusted CLI**, and one attacker-initiated CLI invocation with a user-controlled **`LD_PRELOAD` library**. The library executes a constructor inside the *trusted executable's process* before sending the mock request; the installed executable itself remains root-owned and unchanged.
- The verifier should deny the direct Python client while allowing **all three calls from the very same trusted binary**. The library marker must prove attacker-controlled code actually executed inside the whitelisted process. This uses standard non-setuid dynamically linked Linux execution; it is **not** a setuid/AT_SECURE test.
- An unconditional cleanup removes only the uniquely named temporary root-owned fixture and unprivileged build directory. No permanent accounts, OS policy, systemd units, credentials or application data are touched.

Run on a disposable Linux machine with `gcc`, Python 3, and a coordinator authorized to use `sudo -n` for the temporary fixture only:

```sh
python3 -m py_compile research/process-ownership/manager_verified_executable_intent_probe.py
timeout 70 python3 research/process-ownership/manager_verified_executable_intent_probe.py
```

**Executed result:** exact-head Ubuntu 24.04 [research workflow #44915773507](https://github.com/aschenrot/riesel/actions/runs/44915773507) **PASSED** on `67a7d052ae08e46c6071a0f49d4f1e8b99fe3f1c`. The mock root verifier enforced real kernel `SO_PEERCRED`, unchanged root-owned executable device/inode and SHA-256. Direct same-UID Python access was denied, whereas normal CLI, hostile same-UID re-execution of the protected binary, and hostile invocation with an attacker-owned `LD_PRELOAD` constructor were **all allowed**. The test asserts the injected constructor actually ran, while the root-owned executable content remained unchanged. No real system-service or Riesel mutation occurred. The independent standard Linux Validate/smoke result remains a **separate** exact-head check.

**Observed/checked evidence markers:**

- `RESEARCH_M59_ROOT_OWNED_VERIFIED_CLI_PREPARED`
- `RESEARCH_M59_DIRECT_SAME_UID_UNTRUSTED_CLIENT_DENIED`
- `RESEARCH_M59_LEGIT_ROOT_OWNED_CLI_ALLOWED`
- `RESEARCH_M59_UNTRUSTED_SAME_UID_CAN_REEXEC_TRUSTED_CLI`
- `RESEARCH_M59_UNTRUSTED_LIBRARY_CODE_IN_VERIFIED_CLIENT` (asserted in captured injected child stderr, not printed by the root-side verifier)
- `RESEARCH_M59_PRELOADED_SAME_UID_TRUSTED_EXE_STILL_ALLOWED`
- `RESEARCH_M59_ROOT_VERIFIER_MOCK_MUTATIONS_3_DENIED_1`
- `RESEARCH_M59_VERIFIED_CLI_CODE_IDENTITY_INTENT_GAP_PASS`

## Interpretation and limitations

1. **Executable integrity remains essential**, especially for Manager-owned installation/update and OS owner service definitions. But **the ability to execute the root-owned CLI is usually shared by untrusted code under the same human account**. Confirming the executable inode, hash, owner or signed client binary is not proof that a human chose a particular mutation or that its command line/request came from a trusted origin.
2. **Library/code injection is independent of executable-file integrity** for ordinary non-setuid dynamically linked programs. A malicious process can create and load its own shared library through `LD_PRELOAD` when it invokes the trusted CLI. Secure-execution/LSM confinement, an independently protected client principal and/or trusted loader/dependency controls may change this; the test **does not** establish how to configure them safely.
3. A verified binary may perform **read-only** operations or request an **additional privileged approval** without being entitled to perform the effect automatically. App code signing, Windows service DACLs, macOS XPC code requirements, and Linux MAC labels can be useful *inputs* to a broader protected authorization architecture. **They are not interchangeable with a verified human approval event**.
4. The root-side mock server performs **no real privileged Manager mutation**. Its `sudo -n` fixture authority is itself broadly accessible to all CI runner processes, **already proven insufficient** for hostile-same-UID operator admission. The experiment is a deliberate counterexample to a simplistic executable allowlist—not a trusted broker or deployable OS security proof.
5. The `/proc/PID/exe` observation is read with the process connected; this reduces the fixture's timing ambiguity but is **not** a race-free, production process identity/credential protocol. The test does not address FD delegation, ptrace, signed dependency loading, shell/user input forgery, UI spoofing, IPC relay, process replacement and actual in-flight operation authorization.
6. There is **no Windows/macOS test** and no direct user-session GUI/browser experiment. The first release remains cross-platform with normal Linux-only CI and real platform-specific manual checks.

### Source-backed platform and API semantics

- Linux [`unix(7)`](https://man7.org/linux/man-pages/man7/unix.7.html) documents kernel-supplied `SO_PEERCRED` PID/UID/GID; it does not claim human intention or a trusted executable.
- Linux [`ld.so(8)`](https://man7.org/linux/man-pages/man8/ld.so.8.html) documents `LD_PRELOAD` and the **secure-execution mode** distinction. This fixture uses an ordinary non-setuid CLI, so a user-controlled preload is expected. Do not generalize this to setuid, hardened/fully confined or verified dependencies.
- Linux [AppArmor](https://ubuntu.com/server/docs/security-apparmor/) and [policy syntax](https://apparmor.dev/man/apparmor.d/) are *potential comparators* for OS-enforced executable/process security contexts, not accepted design or proof of feasible cross-platform same-UID client isolation. The research workflow records **only diagnostic LSM availability**; it does not load an AppArmor profile or claim a protected security label. On [the executed Ubuntu run](https://github.com/aschenrot/riesel/actions/runs/44915773507), the host reported AppArmor in its enabled LSM list and `apparmor_parser` present, while the runner was `unconfined`. That is a **candidate-platform prerequisite**, not evidence a protected labelled caller can be launched, cannot be self-labelled by an adversary, or can support Riesel desktop GUIs.

## Architectural comparison — same accepted outcome

| Candidate | What it can establish | What it cannot establish alone | Disposition |
| --- | --- | --- | --- |
| User UID + loopback/Unix peer UID | Actual OS account and peer process | Legitimate vs malicious process under same UID | **Reject** as privileged Manager mutation authority (#96) |
| Polkit user + exact action/target | Trusted action details, bounded user permission | Human intent or trusted code when policy allows all processes of user | **Insufficient alone**, useful layer (#123) |
| Protected app service owner / separate OS principal | Reject direct target process manipulation by disallowed UIDs | Which same-UID *operator client* has valid rights | **Promising OS target component**, not full Manager (#123) |
| **Root-owned/signed CLI executable attestation** | Code artifact provenance / expected process executable | Who launched it, who supplied arguments, whether untrusted code shares/injects process | **This experiment tests and should reject as sufficient authority** |
| OS-enforced process isolation/labels + trusted client launch | Potentially make client code/process rights harder to forge or inject | Authorized human intention or browser actions without an independent trust boundary | **Parallel research candidate**, must test label spoofing, injection, relays and launch privileges |
| Protected OS approval/consent broker + scoped operation authority | Potentially bind deliberate approval to exact mutation/registration/attempt and independently protected execution | Resistance to compromised same-user desktop UI, credential theft, phishing and unattended policies without real tests | **Leading research comparator**, not selected implementation |

**Security invariant to specify:** the protected OS owner is the *only* authority to mutate application registration/executable/service ownership, and it must consume **protected operator authorization evidence bound to the exact action + target + launch/registration epoch**, not merely account identity, executable path/signature, user-readable token or browser origin. Treat the actual origin/approval and its revocation/replay protection as separate from executable provenance.

## Required next proof: true same-user protected operator admission

1. An intended user action reaches an independently protected authority with exact immutable operation ID, app registration/attempt and authorized scope. It succeeds under a **verified** protected operator/consent mechanism.
2. An arbitrary untrusted process under the **same user** tries (a) issuing the API request directly, (b) launching the trusted CLI, (c) sending it malicious arguments, (d) injecting library/process code or relaying an already authorized connection, and (e) reusing an approval/receipt. It cannot cause the mutation without a new, explicitly valid, scope/target-bound authority.
3. A browser-specific test checks origin, CSRF, session binding, host-loopback attack, client-side script control and human permission escalation separately; merely displaying a privileged prompt to the human does not ensure the initiating program was trustworthy.
4. Protected first-release **user-session GUI** apps must still interact with the real desktop session under the chosen OS security boundary while preventing untrusted same-user code from bypassing app process/registration ownership.
5. Reproduce corresponding OS-denial/authorized-operation cases manually on Windows and macOS, with their own service/process rights and OS consent mechanisms. Do not infer parity from Linux.

**Stop/go:** If no independent trusted path exists from human intent or explicitly provisioned unattended policy to a protected OS owner for ordinary CLI/browser clients on a target platform, the accepted product guarantee may be **infeasible as written**. Escalate this exact incompatibility to the owner for a documented requirement or threat-model decision; do not quietly reduce security to trusted-local-user assumptions. #59/#75 remain OPEN and this research alone does not authorize implementation, release, milestone or any new production API.
