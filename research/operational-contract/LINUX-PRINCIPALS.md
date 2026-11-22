# #75 — Linux caller identity and same-UID threat boundary

**Status:** isolated security-boundary experiment, **not** a Riesel SDK credential issuer, authorization provider, published interface, Manager process owner, network protocol or accepted local threat profile. This research is independent of [#92](https://github.com/aschenrot/riesel/pull/92) and the S1 runtime-epoch experiment; it deliberately does **not** modify runtime code.

## Question

Can a process on the same Linux host distinguish an authorized independent GUI/CLI/Manager client from a malicious process running under the **same account**, while also preventing the malicious process from pretending to be the application server? Which of these are actually provided by `SO_PEERCRED`, a separate server UID and owner-only credential files?

The Linux `unix(7)` documentation specifies that `SO_PEERCRED` returns kernel-supplied peer PID/UID/GID for connected Unix stream sockets; it is *not* a general application-principal attestation or per-PID authorization certificate: <https://man7.org/linux/man-pages/man7/unix.7.html>. `SO_PEERSEC` can expose an SELinux/LSM security label when an appropriate confinement policy exists; this is not automatically a portable per-process role: <https://man7.org/linux/man-pages/man2/SO_PEERSEC.2const.html>.

## Real OS experiment

[linux_principals.py](linux_principals.py) requires an **unprivileged test controller with `sudo -n` available on a disposable Linux runner**, uses `setpriv` to launch two distinct temporary **numeric UIDs** without creating permanent users or services:

- A separate service owner UID `60091` binds a real AF_UNIX stream socket and writes a secret under its own `0700` directory/`0600` file.
- A legitimate client runs under the runner's normal UID.
- A **different process under the same runner UID** connects independently, claims the same role and is accepted by a server policy that allows the runner UID. This demonstrates the insufficiency of UID-only role authorization against same-UID adversaries. PID values differ but are not trusted role credentials.
- A third process under foreign UID `60092` lies in its JSON payload about its UID. The server rejects it using the actual **kernel-reported `SO_PEERCRED`** value, not user-provided data.
- Clients inspect the server's kernel-reported UID and require exactly the isolated service UID before sending a command. A separate server account provides a meaningful **OS-level server-owner distinction** under this limited assumption.
- The runner UID and foreign UID cannot read the isolated service owner's private credential file; the service owner UID can. **This tests separation between UIDs**, not secrecy from another process in the *same isolated service UID*.

No root system service, systemd unit, Riesel executable, application runtime epoch or privileged real-world data is used. The identity-test runner has no deployed user assets. The service and UIDs exist only for this temporary subprocess fixture.

### Exact CI reproduction

```sh
python3 -m py_compile research/operational-contract/linux_principals.py
python3 research/operational-contract/linux_principals.py
```

The dedicated [isolated Linux workflow](../../.github/workflows/operational-linux-principals-research.yml) checks sudo/setpriv availability before running. **Do not claim the experiment passed until the exact PR head and CI log are checked.**

Expected result markers:

- `RESEARCH_S2_PEERCRED_KERNEL_UID_NOT_CLAIMED_UID`
- `RESEARCH_S2_SAME_UID_SIBLING_CAN_INVOKE_PRIVILEGED_ROLE`
- `RESEARCH_S2_DISTINCT_UID_ISOLATES_SERVER_SECRET`
- `RESEARCH_S2_REAL_LINUX_PRINCIPAL_DIFFERENTIAL_PASS`

## Competing boundaries and limitations

| Candidate | Useful guarantee under the stated assumptions | What it **cannot** guarantee |
| --- | --- | --- |
| A — Same-UID loopback TCP or same-UID Unix socket | Local reachability, optional typed/versioned RPC | Server/caller authenticity, protected operator privileges, per-client same-UID identity |
| B — Unix peer credentials + explicit per-UID policy | Kernel-attested UID/GID on a connected local socket; can reject a different unapproved UID | Distinguishing a malicious sibling process running under an already authorized UID |
| C — Separate OS-principal service/broker with strict socket peer-UID validation | Broker-owned secret protection and a verifiable owner UID, stronger server authenticity from a client under another UID | Authorizing a specific app/GUI process among several **same-UID** operator clients; installed broker lifecycle/provenance/permission issuance |
| D — Distinct confined client principals, LSM security domains or trusted interactive authorization | May provide finer per-application roles when OS policy actually enforces and audits them | Cross-platform equivalence, deployment simplicity, support on systems without the relevant confinement/policy |
| E — App bearer token stored in owner-only file | Convenient explicit opt-in trust for *cooperating, fully trusted same-UID processes* | Attacker operating as the same owner reading/copying credentials; robust same-UID role isolation |
| F — Protected remote transport with mutually authenticated peers | Can bind remote clients and servers to external identities if credential provenance/lifetime are sound | Security simply from choosing TLS when secrets/caller identity are already compromised in the same user principal |

**Important product limitation:** A non-root `systemd --user` service ordinarily runs under that same user identity and cannot freely set `User=` to a different account. The OS vendor documentation identifies such a restriction; dedicated service identity generally requires a *system-level* service/broker or a separately configured external principal: <https://www.freedesktop.org/software/systemd/man/261/systemd.exec.html>. No system-level broker is implemented by this fixture. Manager [#59](https://github.com/aschenrot/riesel/issues/59)'s already tested user `.service` remains a valid process-owner *candidate* for a **trusted user** deployment and is not automatically a hostile-same-UID security boundary.

## Scope and owner decisions still required

1. **Classify each interface by trust profile**, not one global shorthand. Public app reads/mutations, Manager stop/install/launch and GUI inspect/step may have different caller policies; the profile and limitations must be visible to clients and operators.
2. Decide whether the **first supported local installed Manager mode trusts the entire OS user**, and separately whether privileged operations require a different OS authority. A subset can be accepted without silently claiming strong same-UID protection.
3. If stronger same-UID separation is required, specify *how* each legitimate independent client receives its protected authorization and prove that a malicious sibling process cannot impersonate it. A dedicated broker UID alone protects the broker, not the peer clients of the same UID.
4. Publish explicit **server authentication**, **per-operation authorization**, **runtime session generation** and **readiness** requirements. A trusted socket/peer UID does not identify the app's current Riesel runtime session, and a generation token is not an authorization credential.
5. Require negative tests for forged claimed UID, wrong server owner, stale generation, stolen/replayed bearer proofs, untrusted executable/registration swaps, and privileged management requests. No transparent mutation replay.
6. Investigate Windows/macOS equivalents separately; do not copy Linux-specific UID/SO_PEERCRED claims into a transport-neutral public guarantee.

**Recommendation (not accepted):** treat a trusted-user local profile and a protected-authority profile as **different capability levels** for product requirements, not as automatically interchangeable deployments of the same asserted security semantics. Keep #75 open until the owner explicitly selects the supported threat profiles and the necessary OS/credential guarantees. No implementation authorization, public SDK API, new daemon or merge follows from the experiment.
