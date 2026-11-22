# #59 — Real polkit authentication research (M-S2)

**Status:** bounded Linux polkit/PAM research **EXECUTED and PASSED** at source commit `8ceb5216b74fff127a5ebfa2709ef83a6976524b` in [Ubuntu research CI 44967871027](https://github.com/aschenrot/riesel/actions/runs/44967871027). Final report-head CI must also be checked. The result is limited to credentials, challenge semantics and the tested noncached policy; it is not a trusted operator approval surface. Not a Manager implementation, product authorization mechanism, trusted human-intent proof or Linux release acceptance. Parent [#59](https://github.com/aschenrot/riesel/issues/59), coordination [#75](https://github.com/aschenrot/riesel/issues/75), prior real protected-app proof [PR #139](https://github.com/aschenrot/riesel/pull/139), independent same-user polkit counterexample [PR #123](https://github.com/aschenrot/riesel/pull/123).

## Question

Can the **real Linux polkit authority and PAM** require fresh credentials for one specifically described operation, avoid an ambient same-UID permission and deny wrong target details? Previous research established distinct-UID process ownership, trusted root operation admission, and a Linux polkit rule that admitted **both** same-UID programs under username-based YES. The remaining **product** question is whether a real authorized operator, as opposed to arbitrary same-user code and spoofable desktop input, approves the exact operation. This investigation measures only the **credential/authentication** subgate, not that final guarantee.

## Actual bounded experiment

1. In disposable Ubuntu 24.04 CI, create a random NSS user with a random, test-only password; do not alter the runner account or a real administrator.
2. Install a randomly named **fictional** polkit action and temporary administrator research rule, confined to that action and an exact dummy unit/verb. Return AUTH_SELF, not a YES or cached-authorization variant. No real systemd service or Riesel application is controlled.
3. Start two independent unprivileged processes under the **same test UID** with observed PID/start-time/kernel UID, zero effective capabilities, and blocked stdin to retain their identities. A root-side trusted mechanism supplies the action details to polkit.
4. Without interaction, require both subjects to be **challenged rather than admitted**, and incorrect unit or verb to be **denied**.
5. Use the real textual polkit authentication agent and PAM through a disposable PTY. Supply the generated test credential programmatically. This is a **credential feasibility test**, NOT a human or secure UI approval.
6. After authenticating the first subject, require both subjects to still be challenged with noninteractive checks. This verifies **no cached KEEP-style grant** in the tested configuration.
7. Unconditionally remove both temporary policy files, delete the test account, and clean up processes. No real admin action, persistent Manager control, user data or system service is modified.

The upstream [polkit reference](https://polkit.pages.freedesktop.org/polkit/polkit.8.html) explicitly says applications should not ship their own JavaScript authorization rules; this rule is a disposable research fixture, **not a product recommendation**.

## Expected outcomes — verify in CI before reporting

| Check | Meaning of success | Cannot establish |
| --- | --- | --- |
| Two independent same-user checks challenged | A UID does not automatically produce a YES for this policy | Legitimate versus hostile client intent |
| Correct target challenges; wrong verb/unit denied | Trusted action details influence policy | Atomic binding to later OS mutation |
| Text agent accepts generated test password | Real PAM/polkit AUTH_SELF credential mechanism exercised | Trusted human click, trusted X11/Wayland input or browser |
| Follow-up checks still challenge | Non-KEEP grants are not reused in this fixture | Safe caching across all OS versions, epochs and sessions |
| Cleanup complete | Disposable action/account deleted | Any installed Manager authority or production release |

The dedicated job passed with all expected markers: two same-UID clients independently challenged; correct target/verb challenged while wrong details denied; a generated password completed **real polkit/PAM AUTH_SELF** in a textual agent; both clients required fresh challenges afterward without KEEP; and disposable policy/account cleanup succeeded. Source revision and job link above are direct observed CI evidence. These results **do not** mean a malicious same-user app could not trigger or spoof a session prompt. The dedicated [Ubuntu workflow](../../.github/workflows/manager-polkit-interactive-research.yml) runs [the probe](manager_polkit_interactive_challenge_probe.py). Run from root of a **disposable** checkout with authorized fixture sudo:

    python3 -m py_compile research/process-ownership/manager_polkit_interactive_challenge_probe.py
    timeout 100 python3 research/process-ownership/manager_polkit_interactive_challenge_probe.py

## Critical review and wider architecture comparison

**A — Ordinary polkit agent and PAM per operation.** Provides OS-managed credential verification, but the agent normally runs in the user's own desktop session and can be manipulated under some same-UID models. A malicious same-user program can initiate requests that cause approval prompts; the policy API advises allowing interaction only for genuine user actions but does not independently attest those actions. PR #134 already demonstrated X11 native-GUI input injection. A credential challenge is not a human-intent guarantee. Never use AUTH_SELF_KEEP or AUTH_ADMIN_KEEP for variable-dependent exact-operation approval: polkit documents that temporary grants for one action ID can succeed despite changed details. Sources: [polkit rules](https://polkit.pages.freedesktop.org/polkit/polkit.8.html), [authority API](https://www.freedesktop.org/software/polkit/docs/latest/eggdbus-interface-org.freedesktop.PolicyKit1.Authority.html), [pkcheck](https://www.freedesktop.org/software/polkit/docs/latest/pkcheck.1.html).

**B — Independently OS-trusted consent.** An OS-owned trusted desktop/secure attention boundary or out-of-band administrator approval could show one frozen operation, independently of untrusted desktop input. On Linux, general availability of such a trusted UI must be established per supported environment; do not infer it from generic Wayland or a regular polkit dialog. Compare Windows UAC secure desktop and macOS privileged helpers independently. A secure prompt needs operation integrity **through the final OS mutation**, not only authentication.

**C — Durable narrowly approved unattended policies.** A separate authorization of a bounded per-app policy allows automatic startup/recovery/update under server-held authority. It avoids a recurring interactive prompt during background work, but first issuance/revocation still needs protected human approval. The policy cannot be inferred from registering a binary, an update feed or a normal user-session token.

**D — Unsupported protected interactive context.** When a supported host cannot distinguish malicious same-user input from legitimate approval or when the user has ambient privilege allowing direct app stop, first-release privileged Manager operations must **fail closed** or the deployment cannot claim the accepted protected profile. A separate explicitly trusted-development profile does not substitute for the accepted same-user-resistant first-release guarantee.

## Cross-platform mechanism comparison — documentation research, not executed evidence

| Platform | Existing OS mechanism worth evaluating | Boundary it actually provides | Additional unproven Manager requirements |
| --- | --- | --- | --- |
| Linux | polkit authority, PAM challenge, text/graphical agents, OS-owned process principals and optional OS-enforced confinement | This PR exercises a **real credential challenge** and no-Keep check with two same-UID clients; #139 exercised a real protected Riesel system process | Trustworthy user-intent/input path on each supported desktop, hostile same-user denial for actual Manager CLI/browser, exact attempt/action binding through mutation, and user-session process semantics |
| Windows | Service Control Manager access descriptors and isolated service accounts; UAC elevation on **secure desktop by default** | Windows documents service-object ACL checks and a protected desktop for elevation UI | UAC approves **elevation of a program**, not automatically one immutable Manager action; a separate protected helper must verify parameters, prevent same-user process replay and reconcile service stop/races. The default secure desktop can be changed by policy; test actual host configuration |
| macOS | SMAppService registration of LoginItems, LaunchAgents and LaunchDaemons (macOS 13+); macOS Authorization Services and Security Server | Apple documents system approval for launch-daemon registration and OS-backed authorization rights | Approval of installing a helper is **not a continuing blanket authorization of arbitrary Manager commands**. The exact per-operation consent, helper executable signing/ownership, XPC caller verification, stored right lifetime and GUI input trust require executable tests and explicit design |

Official source pointers: [Windows UAC secure desktop](https://learn.microsoft.com/en-us/windows/security/application-security/application-control/user-account-control/how-it-works), [Windows service ACLs](https://learn.microsoft.com/en-us/windows/win32/services/service-security-and-access-rights), [macOS SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice), [LaunchDaemon approval](https://developer.apple.com/documentation/servicemanagement/smappservice/register%28%29), [macOS Authorization Services](https://developer.apple.com/documentation/security/authorization-services).

**Anti-bias check:** Windows has an apparent existing trusted elevation UI advantage, but it is not evidence of exact-command protection without a constrained helper; Linux's successful PAM challenge is not evidence of user-intent protection; macOS helper registration approval is not equivalent to application-operation approval. Do not impose a lowest-common-denominator transport or one OS-specific privilege mechanism across all three systems. Share **operation identity, grant intent, receipt and failure semantics**, with independently demonstrated OS-specific authority and UI trust.

## Acceptance remains open

The next real proof must include a legitimate positive and independent hostile same-UID negative **against a real OS-owned Riesel app**, through the actual accepted CLI/browser-to-OS-trusted approval path, with exact registration/attempt/verb/options, no transferable token, no unsafe cache, no direct-signal/elevation bypass and an outcome journal that survives ambiguous dispatch. Repeat on Windows/macOS and both user-session/machine-service scopes for first release. This experiment is deliberately not authorization to freeze a broker, prompt, SDK API or public transport.

Verified research markers from the linked exact-source CI: RESEARCH_M59_TWO_SAME_UID_CLIENTS_REQUIRE_REAL_AUTH_CHALLENGE, RESEARCH_M59_POLKIT_WRONG_TRUSTED_DETAILS_DENIED, RESEARCH_M59_POLKIT_REAL_PAM_CREDENTIAL_AUTHENTICATED, RESEARCH_M59_POLKIT_NO_KEEP_SECOND_SAME_UID_REQUIRES_AUTH, RESEARCH_M59_REAL_POLKIT_AUTH_CHALLENGE_DIFFERENTIAL_PASS, RESEARCH_M59_DISPOSABLE_POLKIT_USER_AND_POLICY_REMOVED.
