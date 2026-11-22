# #75 — shared operational contract S1/S2 research

**Status:** research only; no public API, exporter, service or authorization mechanism accepted.  
**Starting revision:** main a35ac7c9ac9749ff023a72595c6851caee5f4859 (2026-11-21).  
**Reused evidence:** integrated #79, #81, #82 and #85; unmerged draft [#87](https://github.com/aschenrot/riesel/pull/87) at cf95f71d0c5775d4f64776afd6dc957bf700de03, whose [Linux validation and smoke run 44875814573](https://github.com/aschenrot/riesel/actions/runs/44875814573) completed successfully. No #87 fixture was copied or changed on this branch. See #75, #60, and the existing application operational boundary investigation.

## Research question and strict local threat model

Evaluate whether Riesel can offer stable externally verified app/interface identity, generation-fenced attachments, per-operation authorization, meaningful readiness and truthful uncertain outcomes through a **bounded shared semantic model** while CLI, Manager, independent native GUI and OS process owners remain different adapters.

- Distinguish published application identity, definition, trusted Manager registration, installed executable/build, OS launch attempt/PID, runtime session generation, provider instance, explicitly published interface (contract and version), and one client attachment. These have distinct issuers and lifetimes. A port, PID, self-reported contract, provider readiness and in-process SessionToken are not cross-process credentials.
- **Adversary:** hostile processes of the **same UID** as the app. They can connect to local TCP, inspect data/files readable under that UID, and invoke the current unauthenticated RPC. Same-UID peers are deliberately not assumed trustworthy. Peer UID, a private random port, a 0600 file, browser origin and unverified advertised metadata cannot by themselves distinguish a hostile peer.
- The protected operation must be denied **server-side before backend dispatch** unless trusted application identity, interface/version, runtime generation, attachment authorization and operation-specific right all match. A strong authorization claim requires evidence that a hostile same-UID process cannot obtain or impersonate the granting principal.
- Outside these narrow tests: privileged adversaries, OS compromise, remote transport security, hostile application code, cross-platform confinement and availability under DoS. They need their own acceptance gates if targeted.

## What is already established by real Riesel tests

1. **SDK-resolved independent process:** #82's integrated host receives an AsyncCapability<AsyncCounterContract> through Application::async_handler and forwards calls using manual AsyncRpcHandler, preserving domain errors separately from RPC/session errors. The generated external client calls it. #81 provides a distinct component-owned comparator.
2. **Stale process replacement — unsafe baseline:** #87's successful separate CI fixture keeps a generated external typed binding alive, stops the resolved-host OS process, starts a new process at the identical address and observes the unchanged old client mutate the replacement state. Client-local SessionToken does not fence a new server.
3. **Lost response — ambiguous outcome:** #87 withholds a response after a real resolved add(3), then observes value 3, with no implicit retry. A transport error cannot imply the command did not execute.
4. **Current transport:** the loopback reference RPC is unauthenticated. Contract verification describes API shape; it does not establish server authority, client authorization or authenticated callable readiness.

## New, reproducible adapter experiment

[probe.py](probe.py) is a **test-only Python gateway**; it is not a Riesel SDK exporter or a public protocol proposal. It starts the unmodified real SDK-resolved Counter host as a separate OS process and forwards permitted JSON wire requests. A stable public loopback listener, fixture-scoped app/interface identity, mock in-memory read/write proofs, unpredictable **gateway-owned** generation, attachments and a lock around authorization + dispatch let us exercise a possible policy state machine without modifying Riesel. The generator of the epoch is crucial: this gateway cannot assert that its epoch belongs to a real Riesel application session.

Run from repository root after the existing smoke gate:

    cargo validate --check
    cargo smoke
    python3 research/operational-contract/probe.py --real
    python3 research/operational-contract/same_pid_probe.py

Or verify only Python harness logic when Rust is unavailable:

    python3 research/operational-contract/probe.py --model

**Verified exact CI evidence:** on research head `d62827da94ad76b66e5df6b7e91da7a8d5884782`, [Linux workflow 44878277483, attempt 2](https://github.com/aschenrot/riesel/actions/runs/44878277483) passed `cargo validate --check`, `cargo smoke` and **both real research probes**; Windows skipped under the manual pre-release policy. Attempt 1 of this run failed in unchanged `packages/storage/sqlite/tests/restore.rs::incomplete_native_backup_copy_does_not_publish_a_partial_restore` with a cooperative SQLite owner conflict; the same exact revision passed on rerun. That intermittent baseline failure is not explained by these research changes and is not silently treated as resolved. Earlier research head `3c697beb136b7041a025b414e42b2c4868ae92e9` also passed all corresponding initial probe gates in [run 44877324774](https://github.com/aschenrot/riesel/actions/runs/44877324774). The local model-only execution produced:

    RESEARCH_MODEL_GATE_REPLACEMENT_PASS
    RESEARCH_GATE_SAME_PID_ROTATION_MODEL_ONLY (NOT Riesel recovery)
    RESEARCH_GATE_UNKNOWN_OUTCOME_NO_RETRY_PASS
    RESEARCH_SAME_UID_0600_SECRET_READABLE (no strong same-UID isolation)

**Separate real-backend observations:** the successful Linux logs contain `RESEARCH_REAL_RESOLVED_COUNTER_GATE_REPLACEMENT_PASS`, `RESEARCH_GATE_UNKNOWN_OUTCOME_NO_RETRY_PASS`, `RESEARCH_SAME_UID_0600_SECRET_READABLE`, and `RESEARCH_REAL_RIESEL_SAME_PID_RECOVERY_EXTERNAL_STALE_REJECT_PASS`. The second probe [same_pid_probe.py](same_pid_probe.py) runs a purpose-built *Rust application* [fixture](../../../examples/counter/app/tests/fixtures/counter_same_pid_recovery.rs) through actual `Application::run_from_env`, a managed Counter crash and `restart_application` with **one unchanged application PID** and a listener surviving the restart. It retains an old external attachment, rejects that stale attachment before `add(2)` dispatch, confirms fresh replacement state 0, and permits a newly attached client to call `add(5)`. However, the test app callback (not the SDK runtime) mints each externally represented epoch and the proofs are insecure fixed research strings. These experiments establish a viable **adapter behavior**, not runtime-issued external generation or protected local authorization.

| Required test | New adapter observation / reused real evidence | Limitation |
| --- | --- | --- |
| SDK-resolved capability across independent processes | Real Counter host from main, already verified #82/#87; real-mode gateway sends actual RPC to it | Gateway not SDK-native |
| Old attachment rejected after app **OS process replacement** | Gateway's stable endpoint retains old attachment; swapping real backend and rotating gateway epoch denies add before dispatch; backend stays 0 | Epoch artificially issued by gateway, not runtime |
| Old attachment rejected on **same-PID whole-session recovery** | **Real Riesel managed-child crash and whole-session retry**, unchanged app PID and external listener, old client fails before backend mutation, fresh client adds 5; verified 44878277483 | Externally conveyed epoch issued by the research app callback, not a runtime-owned published SDK interface; fixed mock proof insecure |
| Freshly authorized reattachment | New gateway-issued attachment after rotation permits read/add | Mock credential delivery not a security guarantee |
| Unauthorized, wrong-app, wrong-interface/version requests | Gateway refuses missing attachment, read-only mutations and mismatched app/interface/version before forwarding; mutation leaves backend unchanged | No proof against hostile same-UID secret theft or impersonation |
| Listener-bound vs app-verified vs callable interface | Gateway reports bound before backend; backend query makes app operation reachable; only authorized value call marks an attachment callable | Not a Riesel-issued ready attestation, nor full app health |
| Lost mutation reply with no retry | #87 backend reply suppressed; new gateway drops client reply *after* backend add(3), reads 3, explicitly adds 2 to get 5 | No durable idempotency/exactly-once or network crash guarantee |
| Hostile same-UID process | Separate interpreter of same UID successfully reads file restricted to owner mode 0600 | Negative example; does not exhaust all attacks or demonstrate a secure deployment |

## Negative hypotheses and security boundary

- **Loopback/contract verification authenticates:** contradicted by credential-less #81/#82/#87 mutation and transport design.
- **Client-local session invalidation fences a server restart:** contradicted by #87.
- **Timeout means no side effect:** contradicted by #87.
- **Owner-only 0600 token protects against same-UID readers:** contradicted by separate-process POSIX test.
- **A gateway-minted generation guarantees actual Riesel recovery:** **not demonstrated**, and must never be treated as proof. The runtime must issue and revoke the externally represented epoch on every fresh whole session; proxy-only rotation could silently diverge after a crash.
- **Bearer-only transport proves strong local auth:** **not demonstrated**. The mock proves protocol checks can be ordered before dispatch, not that secret issuance, storage or distribution is secure. A malicious same-UID process that obtains the proof can impersonate the allowed client.

Credible principal separations to evaluate: different OS identities plus verified IPC permissions, OS-specific sandbox identity and mandatory access controls, or an independently trusted higher-privilege broker with a defended issuance process. A daemon **running as the same UID** is not automatically stronger. For any platform lacking a verifiable principal boundary, reject privileged attachment or require a clearly marked weaker local-trust mode instead of silently promising isolation.

## Equitable architecture comparison

| Criterion | A: product-specific adapters | B: bounded shared operational semantics + adapters | C: mandatory central control service |
| --- | --- | --- | --- |
| Source of runtime truth | Existing app, each adapter interprets | Existing app session owns lifecycle; model defines shared facts | Danger of second lifecycle/session authority unless delegated |
| CLI / Manager / GUI reuse | Repeated rules and inconsistency risk | One identity/version/readiness/error meaning, independent implementations | Central features but a new required deployment component |
| Security | Each adapter must independently implement true principal separation | Shared requirements, platform-specific vetted trust implementation | Helpful only if broker principal really differs and is trusted |
| Recovery/outcome semantics | Divergent stale and timeout behavior likely | Shared stale/unknown meaning, runtime-anchored generation | Service restart adds more uncertain states |
| Portable headless operation | High, with duplication | High, optional transports, no mandatory daemon | Lower; discovery, service startup, permissions and installation |
| Initial engineering | Lowest per adapter | Explicit foundational contracts and multiple proof points | Highest operations/maintenance/security cost |
| Cumulative migration | Increases if incompatible conventions ship | Gradual per adapter; controlled version evolution | Hard once external clients depend on service |
| Future installed apps/events | Custom extension per product | Shared identity/generation + snapshot-first reconnect; streams separately specified | Strong when accepted cross-app privileged multi-tenant orchestration needs justify it |

**Recommendation:** B, in line with the owner's accepted *direction*, not acceptance of concrete mechanism. Neither the smallest initial patch (A) nor the largest framework (C) is categorically future-proof. C is not required by the currently accepted independent, headless developer/Manager/native-UI outcomes. Compare duplication avoided and actual security/correctness costs before freezing even B's SDK surface.

## Proposed smallest coherent invariants — not yet a public contract

1. **Stable published identity.** An explicit publisher-issued app/interface identity, separately versioned application artifact and capability API. Manager registration remains local to its trusted registry; one interface publishes only explicitly allowed operations. Version and typed API shape both checked, with defined incompatible/downgrade behavior.
2. **Authoritative session generation.** A cryptographically unpredictable externally conveyed generation comes from the **Riesel whole application session**, not PID, port, client, gateway or provider. Mint anew on each process start **and on same-process whole-session recovery**. Invalidate old attachments atomically with dispatch admission; in-flight effects may still be ambiguous.
3. **Authenticated attachment and authorization.** Verify the server against a trusted owner/registration, verify the attaching principal against a defensible OS trust boundary, then bind its scoped grants to app + published interface + version + generation. Authorize read, mutate, inspect and stop independently **before every operation**; no implicit publication of internal providers.
4. **Evidence-tagged readiness.** Declared plan, spawn, provider ready, application handler active, listener bound, and **specific authenticated interface-callable** are separate facts. Record the observer/provenance, generation and observation age; unknown, failed, degraded and stale are first-class outcomes.
5. **Unknown mutation outcome.** Timeout/disconnect after possible dispatch means outcome unknown; never automatically replay. Clients may explicitly reattach, query and reconcile. Idempotency keys require a separately specified authoritative durable deduplication boundary; they are not an incidental RPC flag.
6. **One lifecycle owner.** The SDK/runtime owns its sessions, CLI/Manager/OS supervise processes as their documented authority permits, GUI owns its window event loop. None should require a new global daemon or duplicate resolver; graph scheduling stays with #83.

## Blockers and acceptance decisions

**S1 (runtime external generation) partially verified:** the new same-PID Rust fixture proves that after actual Riesel whole-session recovery an **app-authored** local listener can revoke an external attachment before mutation and admit a fresh one. It does **not** establish runtime-**issued** external generation, verified publisher identity, atomic revocation/admission under concurrent in-flight calls, or the eventual SDK exporter. The remaining acceptance gate must bind epoch issuance/withdrawal to authoritative Riesel session ownership and test the revocation race.

**S2 (same-UID trust) unverified:** identify and *attack-test* a credible Linux principal boundary/credential issuer and platform portability policy. 0600 same-UID files, loopback and provider metadata explicitly fail. Secure readiness requires trusted server identity too.

**S3 readiness unverified as app-runtime proof:** tie a permitted authenticated callable probe to the actual session, not merely to a reachable proxy. Distinguish provider readiness, app handler readiness, listener bind and interface readiness.

**S4 unknown outcome partially proven:** #87 establishes real lost reply and no automatic retry. Future explicit reconnection must avoid replay and preserve an uncertainty category even across revocation.

**S5 cross-product adoption unverified:** interface ownership/registration assumptions must be reviewed with separate #60 exporter agent; #59 Manager, #68 tooling and #83 scheduler retain independent work boundaries.

## Independently verifiable future slices (not implementation authorization)

1. **Runtime-owned epoch and publish/withdraw semantics:** build on the tested same-PID app-authored adapter; prove the runtime itself issues/withdraws authenticated external generations, with revocation-race and wrong-generation dispatch tests. Requires owner decision on identity and runtime authority.
2. **Trust/authorization adapter:** cross-process principal separation and operation rights tested against hostile same-UID caller; fail-closed on unsupported deployments. Requires accepted local threat model and trusted-issuer choice.
3. **Callability/provenance observations:** authenticated interface readiness separate from plan/provider/socket and ownership states. Depends on the trusted identity/epoch boundary for authenticated claims; ordinary local status research is independent.
4. **Uncertain mutation and resynchronization:** error classification, no automatic replay, explicit refetch; durable idempotency only if separately accepted.
5. **Product proofs:** one independent #60 native event loop using actual app-owned published capability and lifecycle, then product-specific #68/#59 adapters. Native GUI toolkit and Manager service ownership do not belong in #75.

**Conclusion:** the tested research adapters demonstrate server-side stale rejection after **both new-OS-process replacement and actual same-PID Riesel whole-session recovery**, fresh attachment, scoped operation policy, callable-readiness distinctions and unknown mutations without replay. This **does not pass #75's full S1/S2 acceptance gate**: runtime-owned external identity/generation and strong hostile-same-UID authorization remain unverified, as do true secure readiness and a public exporter. Keep #75 open. No production API/new crate, milestone change, merge or implementation is authorized by this branch.
