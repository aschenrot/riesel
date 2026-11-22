# M-S2 research: protected exact-operation test authority

**Status:** research experiment, not a production Manager or an accepted authorization mechanism. **Primary:** [#59](https://github.com/aschenrot/riesel/issues/59); **coordination:** [#75](https://github.com/aschenrot/riesel/issues/75). **Test plan:** [draft PR #86 section 6](https://github.com/aschenrot/riesel/pull/86).

## Question

Can an OS-protected *test* authority admit one exactly bound privileged operation on a **real Riesel application** while two independent ordinary processes with the same Linux UID can neither approve it nor invoke OS-level process controls? This is a deliberately narrower subgate than the still unresolved requirement to distinguish a **human-approved Manager CLI/browser operation** from an untrusted process under that same desktop user.

**No human consent is implemented here.** A root-only CI test controller stands in for the future trusted authorization path. The CI runner itself has passwordless sudo; therefore its UID cannot be the hostile principal. Both independent requesters/attackers run with dropped groups as numeric UID 60090, which is verified to have no sudo authority. Results apply conditionally to this OS trust separation only.

## Architecture and scope

- Pin the actual already tested PR #90 Riesel Counter application source at commit 5201e61cc74bab778dfc63a3788f1708b2530f21. Build the unmodified Counter binary; there is no new GUI framework or separate app API.
- Install a disposable root-owned executable under a uniquely generated path in /opt. A systemd **system service** with DynamicUser=yes runs the real Counter under another kernel UID. Its actual process PID and immutable InvocationID are observed. Reuse PR #134's always-open-stdin workaround; **no graceful app shutdown proof**.
- Run one independent root-owned *research-only* broker with a public Unix proposal socket and a separate root-only approval socket. The latter has a private root-owned 0700 parent and 0600 socket; kernel SO_PEERCRED rejects non-root connections. No public endpoint can authorize mutations.
- The unprivileged proposal simply carries the exact systemd unit, InvocationID and stop action. The broker allocates different opaque request IDs for independent clients. These IDs are **not bearer grants**.
- A root-only **test controller** approves exactly one identified proposal, including its operation fields. The broker rechecks the active OS attempt, consumes the approval once, invokes the real system-service stop, and observes OS termination. Any other proposal remains pending.
- All fixtures are temporary and name restricted; there is no installed Manager, privileged product API, permanent service or user database mutation.

## Expected tests (not pass claims)

| Research case | Required result |
| --- | --- |
| Real application | Counter runs as separate non-root DynamicUser; actual PID and systemd InvocationID observed |
| Two independent same-UID clients | Each receives a separate pending proposal; the real Counter remains active |
| Unauthorized admission | Public authorization call rejected and root-only approval socket inaccessible to UID 60090 |
| Direct OS bypass | Same UID 60090 cannot sudo, stop the system service or signal the differently owned app |
| Incorrect privileged action | An approval specifying the wrong attempt is denied and app remains active |
| Exact approved action | Root test controller approves matching app, attempt and stop operation; actual Counter stops |
| Replay/other proposal | Consumed approval cannot run again; other same-UID proposal remains pending |
| Cleanup | Transient systemd unit stopped/reset; unique binary and broker directory removed |

Expected success markers are in the Python script. Exact-head dedicated Ubuntu research CI **and** normal repository Linux Validate/smoke are mandatory before reporting a passed experiment. The test script rejects non-systemd hosts and foreign unit/install paths.

## What this does NOT prove

1. **Human or CLI/browser intent.** The privileged test controller is already root. No secure interactive OS prompt exists; no separate legitimate same-UID client is privileged independently of an approval. The public requests remain indistinguishable by UID.
2. **Resistance to broadly sudo-capable adversaries.** The runner's own UID has passwordless sudo and could bypass broker controls; test UID 60090 does not. PR #123 has already verified that ambient sudo is a dangerous same-user bypass.
3. **A trusted desktop.** PR #134 demonstrated X11 input injection into an authorized native GUI. The tested broker does not resolve GUI input provenance, browser CSRF/Origin, Wayland/Xwayland or secure consent prompts.
4. **Durable exactly-once effects.** Pending proposals/receipts are in RAM; crash loses them. OS attempt observation followed by systemctl stop is not atomic with respect to a privileged service-replacement adversary. There is no durable journal, controller recovery, command-result reconciliation or safe retry guarantee.
5. **Cross-platform or user-session acceptance.** This is Linux system-service scope. It proves no Windows/macOS OS owner, UAC/XPC authorization, user-session app ownership, compatible update/uninstall or production capability export.
6. **Graceful Riesel shutdown.** The OS owner stops the actual process but the original Counter's stdin shutdown protocol is not exercised.

## Next decision-critical step

Test a genuinely trusted OS approval path that securely binds the exact registered application, immutable OS attempt, action, options and expiry while a hostile *same-user* program cannot cause approval, replay it, redirect it or inject input into the authorized surface. Its privileged process owner and user-session lifecycle must also be verified. If that cannot be achieved on a given platform/session, fail protected mutations closed rather than silently weaken #59's accepted security requirement.

## Reproduction

Use the dedicated Ubuntu 24.04 GitHub workflow with systemd PID 1 and exact PR #90 pinned source. Build the actual Counter binary under Rust 1.100.0 and execute the following after checkout:

    python3 -m py_compile research/process-ownership/manager_protected_exact_operation_probe.py
    timeout 155 python3 research/process-ownership/manager_protected_exact_operation_probe.py --exercise "$PINNED_COUNTER_ROOT"

The workflow runs the experiment before the separately enforced standard Linux validation/smoke. No production API or mechanism is approved by passing this test.
