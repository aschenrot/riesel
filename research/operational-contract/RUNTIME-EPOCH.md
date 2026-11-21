# #75 — Runtime-owned external generation and dispatch-fence differential

**Status:** feature-gated, research-only **prototype**, not a public API, accepted architecture, authenticated exporter, transport, or production authorization boundary. Builds upon the baseline same-PID real Riesel recovery experiment in [PR #92](https://github.com/aschenrot/riesel/pull/92); this PR is intentionally **stacked on that draft branch** to avoid duplicating or prematurely integrating the unaccepted fixtures.

## Question and baseline

Issue [#75](https://github.com/aschenrot/riesel/issues/75) requires the *actual Riesel whole-application session* to issue external generation and invalidate it on session restart/shutdown. The original #92 fixture demonstrated same-PID Riesel recovery but minted its generation in the **app handler** with `Uuid::new_v4`. It could prove that a cooperative adapter changes epochs; it could **not** prove that the runtime owns the epoch.

Existing internal Riesel facts verified in source:

- `StartupState::new` constructs the `SessionToken` after preflight, before provider startup; each whole-session retry calls the startup state machine again.
- `RunningApp` owns that token and shares it with internal generated capability handles and the process monitor; `invalidate()` revokes handles.
- `Context::session_token()` is private to async SDK code and no external per-session generation, authenticated publisher or authorized attach is currently supported.
- A one-time ingress check of an atomic session flag does not serialize an in-flight mutation against concurrent invalidation.

## Isolated alternative B: reuse runtime token + synchronous fence

The **opt-in, off-by-default** `research-external-epoch` feature:

1. Attaches a fresh random UUID generation to `SessionToken::new()`, meaning the existing runtime session constructor—not app handler, listener, Manager, PID or gateway—mints the test external epoch.
2. Adds `SessionToken::research_admit(expected_generation)`, which holds a shared admission permit for an individual synchronous test operation. `invalidate()` takes the exclusive side of the same gate before revoking the token. No operation can **start after revocation completes**, though a previously admitted operation can run to completion.
3. Exposes this exact runtime token through an explicitly **experimental** `Context::research_publication_session()` feature-only accessor, used solely by the existing #92 Counter same-PID fixture when the example `research-runtime-epoch` feature is enabled.
4. Refuses attach when that token has already expired, even if the app's published counter field has not yet cleared, and refuses calls to an old generation. The same test's read/write app-level permissions remain **test strings, not real credentials**.
5. Adds two direct deterministic tests: cloning a session token shares one generation while separately minted tokens differ; and a held ingress permit delays `invalidate()` until the operation releases it, after which a late admission must fail.

No feature is enabled in default builds. The normal #92 application-handler-issued baseline stays available for direct **differential comparison**. The test-only bridge intentionally does **not** modify the existing reference RPC wire protocol or transport.

### Reproduction (from PR branch)

```sh
cargo validate --check
cargo smoke
cargo test --locked -p riesel_protocol --features research-external-epoch
cargo build --locked -p riesel_counter_example --bin counter_same_pid_recovery_host --features research-runtime-epoch
RIESEL_RESEARCH_RUNTIME_EPOCH_REQUIRED=1 python3 research/operational-contract/same_pid_probe.py
python3 research/operational-contract/probe.py --real
```

The dedicated [runtime-epoch research CI](../../.github/workflows/operational-runtime-epoch-research.yml) runs these commands on Rust 1.100.0/Linux. **A source diff or a green old #92 run is not evidence of this branch passing.** Record exact PR head and CI run before treating this as an executed experiment.

## Fair alternative comparison

| Mechanism | Ownership and demonstrated value | Costs and unresolved obligations |
| --- | --- | --- |
| **A: per-app generated epoch** (#92 baseline) | Works with existing SDK and app adapter, distinguishes many stale sessions in cooperative fixtures | No central runtime issuer, inconsistent contracts across apps, stale mutations possible between independent gateway and runtime revocation |
| **B: feature-gated runtime token + lock** (this prototype) | Runtime owns generation and invalidation; no app/Manager duplicate epoch; local admitted effects are fenced against revocation completion | A shared read permit can delay teardown indefinitely when a callback blocks; cannot generally hold a std `RwLockReadGuard` across async worker affinity or remote requests; feature surface is **not** accepted API |
| **C: session-owned ingress coordinator/actor** (not implemented) | Could separate bounded admission, queued operations, cancellation and observable rejection from business callback, suitable for async/GUI operations | More ownership and complexity, transport-to-session wiring and explicit failure/queue-drain semantics must be tested; cannot claim atomic rollback of sent mutations |
| **D: independent privileged broker** (not implemented) | Potentially meaningful OS principal boundary and persistent owner/registration verification | Adds installation, lifecycle, credential and platform-specific policy; not inherently required for every local app feature. This is chiefly an authentication/authority alternative, not a replacement for runtime epoch correctness |

These alternatives do not necessarily compete for the same role: ingress policy B/C and principal trust D are orthogonal. A plaintext loopback client can read/guess/use a non-secret generation; the generation is for fencing, not authorization.

## Verification and limits

The same-PID fixture exercises a real Riesel managed Counter crash, automatic whole-session recovery **within the same OS process**, an old attachment from before recovery, and a newly attached client after recovery. The feature-gated variant identifies the epoch as `runtime_session_token` in its own test response; that marker is **self-reported diagnostic evidence**, not proof of authenticated publisher identity.

The direct race test controls the permit-release order, rather than claiming a timing race cannot happen in production; it tests only the gate's ordering against **explicit** `invalidate()`. The runtime monitor may discover a child failure after the external interface accepted a call; outstanding remote effects can still complete or have unknown outcome. A blocking operation could indefinitely delay the owner from invalidating its session and releasing providers. A concurrent status query is not an authenticated readiness observation.

Most importantly:

- **S2 remains open:** the fixture's `proof` strings, TCP listener address, owner-only credential files, self-declared generation and app/session names do not authenticate the server or an untrusted same-UID caller. A real principal/broker boundary must be separately compared and adversarially tested.
- **S1 remains partial even if all tests pass:** no general SDK exporter/public epoch issuance contract, no durable published descriptor with build provenance, no atomic *remote async* request admission, no version evolution or transport-level lifetime integration beyond the one Counter fixture.
- **S3 remains open:** no external observer can verify callable readiness with authoritative principal, scope, interface shape and fresh runtime generation.
- **S4 preserved:** failed or timed-out mutations are *outcome unknown*; no automatic replay or exactly-once result.
- **#59 distinction:** systemd `InvocationID` is OS process-attempt identity, never this runtime token. Manager's registration/attempt journal and the runtime's session are different authorities even when they share an observation structure.

**Decision recommendation (not accepted):** A runtime-owned external generation and a session-scoped, revocation-coordinated admission boundary should be treated as a **common semantic requirement** for independently attached clients, but this `RwLock` implementation should **not** be selected as the production ingress mechanism. Test bounded async dispatch, independent external controllers and a real same-UID privilege boundary before freezing the interface/operational security contract. Keep #75 open.

No source PR merge, implementation ticket, accepted public feature, roadmap change or security claim is implied.
