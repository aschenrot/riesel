# #60 / #75 — Real Riesel Counter, kernel-attested client security domains and same-UID entry bypass

**Status: unexecuted research hypothesis.** This research is not an accepted OS client sandbox, protected native GUI, installed Manager or production exporter. The test does not change core Riesel APIs or a shipped Counter capability.

## Product outcome and exact question

First-class, **application-owned** publication should permit separately running GUI clients to inspect and modify deliberately published capabilities, respecting an explicit permission model, runtime session and operation outcome. A trustworthy **per-client** mode must not silently degenerate to an allowlist for the entire user's UID. A **lower-trust per-user** profile may still be offered if the developer explicitly accepts its actual threats. These are distinct from the Manager's higher-trust, accepted hostile-same-user privileged OS command requirement.

The exact baseline:
- [#60 simulated app-export policy #91](https://github.com/aschenrot/riesel/pull/91) checks grants without authenticated external client identity.
- [#60 actual dual-PAM-session native GUI #147](https://github.com/aschenrot/riesel/pull/147) established that both native GUIs of the same login UID can mutate a correctly session-bound, separately owned Counter by its intentionally unauthenticated TCP adapter.
- [#60 real isolated Counter gateway #149](https://github.com/aschenrot/riesel/pull/149), **PINNED** at a5c811fbce184e8ff2684ab413798453e6d189f4, eliminated host access to the app's unprotected TCP listener and enforced kernel UID, but a second unregistered process under the same allowed UID still successfully mutated the actual Counter.
- [#59 real AppArmor label/verified CLI research #129](https://github.com/aschenrot/riesel/pull/129), **PINNED** as prior evidence, showed that an OS-enforced AppArmor label can distinguish a separately confined same-UID attacker. It also found that an **unconfined attacker under the same UID could self-select the deliberately permissive trusted profile** on the tested Ubuntu host; trusted binary provenance alone was bypassed by re-execution and LD_PRELOAD.
- [#75 runtime-issued session generation #95](https://github.com/aschenrot/riesel/pull/95) is a separate, research-only and stacked source feature; gateway test epochs must not be represented as runtime-issued identities.

**Question:** If the actual unchanged SDK-resolved Riesel Counter is inaccessible to host TCP, and a filesystem AF_UNIX gateway requires BOTH the kernel-observed Linux UID and the kernel's **SO_PEERSEC AppArmor profile label**, can we demonstrate correct real app-capability operation for a trusted *confined* client and rejection of an independently confined same-UID attacker? Does an otherwise unconfined same-UID process remain able to enter the trusted label and mutate the real Counter, exposing the profile **entry-policy** dependency rather than claiming security from a label string?

This is a **composition proof** of already verified mechanisms over genuine Riesel application mutations, NOT a new general-purpose authentication algorithm, selected network sandbox or secure human consent mechanism.

## Reproducible real Linux experiment

The [dedicated Ubuntu 24.04 workflow](../../.github/workflows/real-counter-client-domain-research.yml) fetches and SHA-pins **unchanged** prior gateway PR #149 and **unchanged actual Counter** PR #90 commit 5201e61cc74bab778dfc63a3788f1708b2530f21. It builds the real Counter from pinned Cargo sources and installs only copies into a random root-owned research location. The new [small extension adapter](linux_kernel_domain_export_server.py) imports the pinned gateway at runtime and adds a **mandatory kernel SO_PEERSEC label equality check before the already verified UID/grant/operation/epoch checks**. The [test controller](linux_kernel_domain_counter_probe.py) loads only two disposable uniquely named AppArmor profiles and uses actual Linux systemd PrivateNetwork ownership. The gateway still has a **test-only** pretrusted root grant authority.

Procedure:
1. Generate two unique AppArmor policies. Both profiles deliberately allow ordinary Python, files and Unix sockets; the **trusted profile is not automatically safe software**, and the **attacker profile forbids transition into the trusted profile**. A trusted ROOT **test** launcher may assign either profile to a process before dropping it to numeric UID60090. The host root test controller is itself broadly privileged; it is NOT a trusted Manager operator.
2. Launch actual Counter under numeric UID60092 inside a systemd root-owned PrivateNetwork=yes service, using unchanged pinned #149 gateway + new label-only adapter. Verify distinct actual network namespace inode, backend UID, original TCP unreachable from the host, and real Counter initial value.
3. Root test controller grants UID60090 the value/add operations through the original owner-only control socket. A new **unconfined** client under UID60090 must be rejected by the **kernel-label gate**, even though its UID and operation grant are correct.
4. Root test launcher creates an authorized **trusted-label** process under the same UID60090. Its real Counter value query succeeds and add(2) changes authoritative Riesel state 0→2.
5. Another root-launched process under the **attacker-label profile** and same UID60090 is denied at the label check. Its subsequent attempted aa-exec transition to the trusted profile must be rejected by the kernel with an explicit AppArmor permission failure; arbitrary process failure is not accepted as a security result.
6. A test client under different UID60091 but the *trusted* profile is separately denied by the **UID operation grant** despite a forged claimed UID in its request. Distinct UID and domain grant checks are independent.
7. An otherwise **unconfined** program under UID60090 attempts aa-exec -p into the trusted label *without root inside that client*. If the OS permits profile self-entry, it must **actually execute Counter add(3)** from that same-UID unregistered process, bringing Riesel state to 5. If the tested host **denies** such self-entry, require a real permission diagnostic and leave the Counter at 2. Record the observed branch; do not invent it or claim universal portability.
8. Root test issuer revokes the UID grant, and even correctly labelled client requests are denied. Stop the exact disposable service, unload both research profiles, remove uniquely named /opt and /run locations. No real production policy, users, credentials, applications or sessions are installed.

Expected markers are hypotheses until actual CI passes. The test uses **real Counter domain mutations**, not mock STOP text, but **no real independent GUI**: a CLI-like short-lived client supplies the lower-level OS identity controls.

## Comparison and bias audit

| Alternative | Meaningful capability | Fundamental limitation |
| --- | --- | --- |
| Unprotected loopback or a 0600 token under the user's own UID | Good low-friction trusted-local development comparator | Does not resist same-UID hostile processes |
| UID-authenticated gateway (#149) | Private network removes original TCP bypass, peer UID is kernel attested | All clients of allowed UID inherit grant |
| AppArmor SO_PEERSEC label **without enforced profile entry** | Kernel reports an actual process label; permits selective deny rules | An unconfined same-user process may enter the approved profile itself, so the reported label is not independent software identity |
| Mandatory, non-bypassable confinement of *all* relevant attackers plus protected authorized-client launch/transition | Can conditionally distinguish two **same-UID process domains** before real domain dispatch | Requires deployment provenance, unconfined-process coverage, trustworthy entry/exec controls, fd-relay/ptrace/input injection defense, and policy maintenance for desktop/Wayland/Windows/macOS |
| Per-client distinct OS UID/isolated OS principal | Inherently separates direct signal/UID-based IPC rights when service owner is protected | GUI session/seat/files/data integration, privilege issuance and portability; even genuine GUI may be driven by hostile same-X11-user input |
| Verified signed executable/PID/path alone | Valuable artifact installation and integrity evidence | Re-execution, loader injection and external input delegation do not prove current human or app-client authority (#129) |
| App-owned versioned capability grant with authenticated client trust domain, actual runtime session generation, revocation and uncertain outcomes | Long-term cross-platform semantic target | Mechanism unaccepted; must compare app-local portal, OS identity, separate process owner, LSM, native platform equivalent and independent Cargo/GUI ergonomics |

**Do not infer that AppArmor must be a Riesel requirement.** The goal is to identify **which enforced security guarantees** a selected local transport must provide for a *particular declared trust profile*, not to prescribe one Linux tool, root daemon or UID map for all platforms. The lower-trust profile must remain explicit and cannot be silently used where hostile same-UID Manager controls are accepted as mandatory.

Official OS context: [Linux unix(7) SO_PEERSEC](https://man7.org/linux/man-pages/man7/unix.7.html), [AppArmor profile/transition syntax](https://apparmor.dev/man/apparmor.d/), [systemd AppArmorProfile](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html), [AppArmor aa-exec](https://www.apparmor.net/man/5.0/aa-exec/). These references do not stand in for real CI policy enforcement.

## Still unproven even if the experiment succeeds

- An actually trustworthy full desktop-client profile issuance **for arbitrary same-user malicious processes**, including processes outside confinement; the unconfined self-entry counterexample may invalidate such a claim.
- Compromise/re-execution/FD passing of a legitimately labelled client, input injection against its GUI, process memory inspection/ptrace, loader dependencies, socket handoff, same-UID code execution, malicious browser/OS portals and denial of availability.
- Riesel runtime-owned externally authenticated session epoch, generation/restart fence, trusted grant persistence, concurrent bounded async draining, unknown-effects ledger and actual app-authorized cross-session GUI publication.
- Cross-platform equivalent proof on Windows or macOS and verified safe installation/uninstall, Manager CLI/browser human approval, secure graphical-session origin. This test is Linux CI only.
- Production code, public API, installed AppArmor deployment or implicit preference for a privileged gateway.

**Next requirement after this test:** specify a strong individual-client trust profile only after proving a **non-self-selectable** domain/credential issuance and preventing bypass via unconfined same-user entry and trusted-client relays. Prove independently that the actual Riesel runtime session owns the publication's epoch and revokes it correctly. Do not build an exporter API or Manager service until those acceptance gates are accepted.

## Reproduction (disposable, sudo-capable Ubuntu host)

    python3 -m py_compile research/application-export/linux_kernel_domain_export_server.py research/application-export/linux_kernel_domain_counter_probe.py
    timeout 165 python3 research/application-export/linux_kernel_domain_counter_probe.py --exercise "$PINNED_GATEWAY_ROOT" "$PINNED_COUNTER_ROOT"

Expected (not yet verified) markers:

- RESEARCH_M60_ROOT_LOADED_TWO_REAL_APPARMOR_PROFILES
- RESEARCH_M60_REAL_COUNTER_ISOLATED_AND_LABEL_GATEWAY_CALLABLE
- RESEARCH_M60_REAL_COUNTER_TRUSTED_LABEL_ALLOWED_SAME_UID_UNCONFINED_DENIED
- RESEARCH_M60_SAME_UID_CONFINED_ATTACKER_LABEL_REJECTED
- RESEARCH_M60_CONFINED_SAME_UID_TRUSTED_DOMAIN_SWITCH_DENIED
- RESEARCH_M60_TRUSTED_LABEL_WRONG_UID_STILL_DENIED
- RESEARCH_M60_UNCONFINED_SAME_UID_SELF_ENTERS_TRUSTED_LABEL_AND_MUTATES_REAL_COUNTER OR RESEARCH_M60_UNCONFINED_SELF_ENTRY_OS_DENIED_ON_THIS_HOST
- RESEARCH_M60_LABEL_PLUS_UID_REVOCATION_BLOCKS_NEW_MUTATIONS
- RESEARCH_M60_ACTUAL_KERNEL_DOMAIN_COUNTER_RESEARCH_PASS
