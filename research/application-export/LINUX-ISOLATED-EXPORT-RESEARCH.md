# #60/#75 — Isolated Counter capability export / real Linux kernel-principal grants

**State: unexecuted hypothesis.** Research-only Python/systemd fixture. No public Riesel SDK/IPC, production exporter, operator authorization, Manager service or security profile is approved. Owners: [native GUI #60](https://github.com/aschenrot/riesel/issues/60), [shared operational contract #75](https://github.com/aschenrot/riesel/issues/75), separate [Manager #59](https://github.com/aschenrot/riesel/issues/59).

## Product outcome and decision-critical gap

Riesel applications must be able to publish explicitly selected, versioned capability operations to independent clients, with deliberate trust profiles, externally verifiable admission, runtime-generation fencing, permission revocation and accurate uncertain-mutation behavior. Those app-owned permissions are **distinct** from privileged Manager OS commands.

The existing real [Counter/native GUI #90](https://github.com/aschenrot/riesel/pull/90) and [cross-session GUI experiment #147](https://github.com/aschenrot/riesel/pull/147) use a deliberately **unauthenticated** plain-loopback RPC endpoint. Correctly protected login-session and OS process ownership do not restrict callers of that TCP endpoint. The [app-export policy comparator #91](https://github.com/aschenrot/riesel/pull/91) uses simulated principal identities, not external client proof. [Linux peer-principal research #96](https://github.com/aschenrot/riesel/pull/96) established real Unix-domain SO_PEERCRED; **all clients of an allowed OS UID are still accepted by a UID-only allowlist**. [Real Counter generation research #92/#95](https://github.com/aschenrot/riesel/pull/95) concerns separate runtime-session identity and does not authorize app clients.

**Research question:** Can a real unchanged SDK-resolved Riesel Counter run under a separate low-privilege OS principal inside a Linux private network namespace, eliminating direct access to its legacy unauthenticated TCP endpoint, while an explicitly published filesystem Unix socket mediates operation grants and revocation using actual kernel client credentials? Can the same fixture make the remaining **same-UID client impersonation limitation** observable rather than misrepresenting the result as strong authorization?

## Reproducible real host fixture

[Linux Ubuntu 24.04 Actions workflow](../../.github/workflows/isolated-app-export-security-research.yml) pins the unchanged original Counter executable from [PR #90 SHA 5201e61cc74bab778dfc63a3788f1708b2530f21](https://github.com/aschenrot/riesel/pull/90), builds with Rust 1.100 and uses actual systemd PID1. It runs the isolated [gateway fixture](linux_isolated_export_server.py) and [independent client/adversarial probe](linux_isolated_export_probe.py).

1. A random transient systemd system service runs the **root test gateway** with mandatory PrivateNetwork=yes, and starts the genuine SDK-resolved Counter binary under a separate nonroot UID 60092. Verify its actual kernel UID, service InvocationID, main PID, backend PID and network-namespace inode. The real Counter listens on its original loopback TCP inside the service's namespace; verify a real RPC value probe works there.
2. The controller checks host-network TCP cannot reach that exact backend port and unprivileged UID 60090 cannot enter the private namespace using nsenter. Fail closed if PrivateNetwork is unavailable. No host-loopback bypass may be accepted as a feature of the gateway.
3. The service explicitly exposes a filesystem AF_UNIX research socket at a unique root-owned directory under /run with **0666 socket mode**. Thus all actual clients can connect, and application operation checks must use Linux SO_PEERCRED PID/UID/GID, **not a user-provided role** or socket-path secrecy. A separate control socket under root-only directory/mode is accessible only to a root test controller and verifies peer UID 0.
4. Test UID 60090 is denied by default. A different UID 60091 claims in JSON to be 60090 and remains denied even after 60090 is granted. The normal client checks the gateway's kernel server UID before sending requests. The client cannot use the root-only control socket.
5. A trusted **test controller**, not a human/production issuer, grants only value to UID 60090. A real Counter query returns zero, but add is denied; wrong gateway-issued epoch is denied before RPC dispatch. The grant then permits both value and add and a real add(2) changes Counter to 2.
6. An entirely separate process under **the identical UID 60090** is also permitted by the UID grant: a second real add(3) changes Counter to 5. The two kernel-observed PIDs must differ. This is an **intentional negative result**: per-UID authorization does not authenticate a specific app or prevent hostile same-UID programs.
7. Arm a deterministic research-only pause just before dispatch, submit another add(6) from the allowed UID, revoke the UID grant from the protected test control socket, then release the paused request. It must be denied **before** Counter mutation, and a fresh denied add must also fail. Re-granting value-only must return the actual Counter value 5. This is not an in-flight already-dispatched rollback guarantee.
8. Stop and remove only the exact randomly named service, installed binary/script and /run runtime directory. No real Manager app, production DB, systemwide authorization rule, permanent user or credential is affected.

**Source assertions are expected, not proven, until exact-head real CI passes.** The gateway lives outside the Riesel runtime, is synchronous and uses a deliberately gateway-generated random epoch, **not** Riesel's actual runtime session generation. The root test coordinator has privileged sudo and is trusted by construction. Its grant operations do not prove authentic user approval.

## Competing architectures and explicit security bias control

| Candidate | Verified or expected property | Irreducible limitation |
| --- | --- | --- |
| Plain loopback RPC | Current Counter typed contract works across processes | No client admission; knowing the endpoint is enough |
| Same-UID bearer/token file | Small local setup, easy app integration | Same-UID malware may read, reuse or extract token; not a robust same-UID trust boundary |
| AF_UNIX SO_PEERCRED + UID grants | OS-verified connection UID and bounded per-UID operation grants; private TCP gateway can close direct bypass | A malicious process under the **same allowed UID** receives the same grant; unsuitable as standalone strong app-client identity |
| Protected process PID/start-time registration | Can restrict nominal direct connections to an OS-observed process | PID reuse, exec, fd transfer, ptrace/process injection, launcher authority and per-request lifetime need additional OS proofs; not human authorization |
| Separate protected UID or mandatory security label per trusted client | May provide actual distinction among co-resident apps | GUI display/session/data access and platform-specific enforcement are difficult; must test untrusted same-user principal and no inherited privilege |
| App-owned versioned publisher with OS-backed client attestation, explicit grants, runtime-issued epoch, revocation and unknown-effect semantics | Best long-term **candidate contract** across Counter, Todo and native GUI | No accepted issuer or transport; compare UDS, native pipes/XPC, authenticated remote channels, API migration, lifetime, concurrency and cost |
| Mandatory Manager for all app capability calls | Central authority for installed apps | Overcouples standalone independent Rust apps/GUI to privileged Manager and confuses app permissions with OS-management authority |

The reference gateway runs as **root** only because it is an isolated adversarial **test controller**. It is not a proposed production root app exporter. Production issuance and root separation, per-client trust, service installation, cross-session GUI ownership and safe auto-update remain independent acceptance gates. Source [systemd.exec PrivateNetwork](https://www.freedesktop.org/software/systemd/man/latest/systemd.exec.html) documents private loopback namespaces while filesystem AF_UNIX remains visible; [Linux unix(7)](https://man7.org/linux/man-pages/man7/unix.7.html) describes kernel SO_PEERCRED. These are hypotheses to verify on the actual CI machine, not substitutes for results.

## Explicit exclusions and next decision gates

- **Strong hostile same-UID client security is NOT proven.** An allowed UID includes its untrusted sibling; process paths, PIDs and self-claimed executable identities are not authenticated client roles.
- Riesel runtime-generated epoch, same-PID session restart, process-controller death, actual native GUI authenticated attach, full Todo CRUD/revision interface, browser session integrity, event subscriptions, cross-session policy issuance and transport migration remain outside this fixture.
- There is no replay-proof outcome ledger, proven cancellation of already-dispatched async work, durable grant store or security assurance under power loss. A failed/disconnected mutation may have executed; no exactly-once assumption.
- There are no Windows named-pipe/DACL or macOS XPC/sandbox/manual native results. Linux only, and only test server/client UIDs.
- No selected Riesel crate, public SDK syntax, auth provider or Manager service is implied by a research pass.

**Provisional recommendation conditional on real evidence:** separate the trusted grant issuer, OS/network client principal proof, app-published capability identity, permitted operation set, Riesel runtime session generation, observed readiness and unknown-effect error semantics. Maintain multiple explicit trust profiles and leave implementation mechanisms to later accepted #75/#60 contracts. Treat Manager protected authorization as the stronger independent #59 gate.

## Execute on disposable Ubuntu systemd

    python3 -m py_compile research/application-export/linux_isolated_export_server.py research/application-export/linux_isolated_export_probe.py
    timeout 155 python3 research/application-export/linux_isolated_export_probe.py --exercise "$PINNED_COUNTER_ROOT"

Expected markers (not yet actual CI evidence):

- RESEARCH_M60_REAL_COUNTER_PROTECTED_IN_PRIVATE_NETWORK_NS
- RESEARCH_M60_DIRECT_TCP_BYPASS_AND_UNPRIVILEGED_NETNS_ENTRY_DENIED
- RESEARCH_M60_DEFAULT_DENY_KERNEL_CLIENT_UID_NOT_CLAIMED_ROLE
- RESEARCH_M60_KERNEL_ATTESTED_READ_WRITE_AND_STALE_GATE
- RESEARCH_M60_SAME_UID_UNREGISTERED_CLIENT_INHERITS_UID_GRANT
- RESEARCH_M60_PRE_DISPATCH_REVOKE_BLOCKS_REAL_COUNTER_MUTATION
- RESEARCH_M60_LOCAL_CAPABILITY_OS_ISOLATION_PROBE_PASS
