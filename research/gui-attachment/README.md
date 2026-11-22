# Real OS-process GUI attachment boundary — research #248

**Status: isolated Linux/Unix research, NOT a production exporter or
authenticated GUI connection.** Built from `main`
`e1a4bcacc08ea9c6833a49b090fceaf9e553af4a` (22 November 2026).
No SDK, Manager, browser, native window or production transport changes.

## Question

Can an independently executing client read a selected, versioned
Riesel-owned graph observation through a Unix socket while the *real*
Riesel application, in the **same OS process and on the same socket**,
ends one runtime session and starts a successor, without treating an
old application generation as current or letting a new request slip
through a withdrawn publication? What does that prove about caller
identity?

## Reproduce

Linux with the repository's pinned Rust 1.100 toolchain:

```sh
cargo fmt --manifest-path research/gui-attachment/Cargo.toml -- --check
cargo test --manifest-path research/gui-attachment/Cargo.toml -- --nocapture
```

The integration test starts `attachment_host` as a **real separate OS
process**. The host runs a real `riesel_app::RunningApp` and an actual
validated two-operation dependency graph. Its observer stays inside
the application. The host deliberately publishes **one read-only
graph summary** on a disposable, 0600 Unix-domain socket. A test
process and a second spawned process are independent clients.

The host uses `SessionToken::internal_publication_session()` from
integrated #157 to obtain a **runtime-minted**, non-secret generation
and finite ingress permits, not a client identity. A short-held
adapter lock composes explicit withdrawal with that permit. There is
no second graph runner, provider resolver or alternative state owner.
A dedicated *test-owner stdin* channel controls `REVOKE`,
`RESTART`, `RESUME`, `STOP`; no socket client has those operations.

The same test-only host can run an 80-node dependent graph after a
new owner-session restart (`OVERFLOW` on the test-owner stdin).
Its real graph observer retains at most 128 transitions. A client
requests `cursor_status` with its last sequence; a cursor older than
retained history reports `truncated=true`. A fresh `snapshot`
then returns all 80 authoritative operation states and the new run
identity. `cursor_status` is **only a loss/sequence diagnostic**:
it deliberately does not export event payloads or implement streaming.
This proves an explicit resnapshot fallback, not a complete GUI event
protocol or a validated operation-detail schema.

After the legitimate host exits, the test client itself binds the
previous socket pathname and responds with a copy of the old
`describe` fields (`app`, `interface`, `generation`, `pid`).
A naive client accepts the counterfeit announcement. This is
**negative server-identity evidence**: no trusted server
attestation, registration or per-peer credential exists in the fixture.

The server accepts bounded JSON test requests over Unix sockets. This
is a **disposable research protocol**, not a Riesel protocol or public
schema. `research_pause=before/after` is a controlled test
synchronization injection to prove two opposing revocation orders.
A client can request `describe`, and exact selected
`research.graph.read` v1 `snapshot`. It cannot invoke graph control,
internal providers or plan activation.

## Observed assertions

- One headless app process and independent client OS processes.
- Real authoritative graph observer: completed dependency operations,
  plan/revision/run identity and nonzero run transitions.
- Exactly the selected interface/version/operation is admitted;
  a forged generation or unsupported interface/operation is denied.
- An external reader's old generation cannot attach to the successor
  Riesel runtime despite reusing the same OS PID and socket.
- An operation stopped *before* admission is denied after withdrawal.
- An operation already admitted may finish after withdrawal; the
  in-flight permit is not a guarantee of rollback or cancellation.
- Another process under the **same Unix login UID** can independently
  discover the generation and read the graph, despite having no
  separately verified GUI principal/grant. **This is expected negative
  evidence**, not successful protected client authentication.
- An actual 80-node graph overruns the retained transition ring;
  stale history is detected, and the independent client can retrieve
  all current operation states from the authority, without inferring
  missing events or treating a future cursor as valid.
- Another process can impersonate the stopped owner's endpoint
  announcement by reusing its socket pathname and copied descriptor
  fields; server identity is unverified.
- Closing external test clients does not shut down the app. The
  process and socket remain until their test-owner shutdown.

## Deliberate exclusions

This fixture is **not** a reusable protected exporter: socket mode
0600 is not protection against hostile same-UID programs; a runtime
generation is correlation/fencing, not a secret or capability token;
a published Unix path or `describe` reply is not an authenticated
server identity. No browser origin protection, network TLS, secure
native client identity, user consent, privileged mutation, binary
attestation, Manager control or cross-platform parity is delivered.

It does not implement an `AsyncTasks`-owned continuous listener,
multi-session supervisor, general client discovery, revocable
per-client grants, bounded event subscriptions or full graph
descriptor/dependency layout. The bounded snapshot here carries
operation state names and run identity, while the cursor diagnostic
does not carry transition payloads; it is not a full frontend
synchronization protocol. The test fixture has a fixed 4 KiB request budget and
16 request-thread admission slots; those are **not production sizing
results**. Its read-only replies are not transaction receipts.
No security guarantee follows from the test-only `research_pause`
or stdin control signals.

The [three-app generated/public interface policy comparator PR #252](https://github.com/aschenrot/riesel/pull/252) separately tests Counter,
Todo and Graph in-process. [Live browser inspector PR #250](https://github.com/aschenrot/riesel/pull/250) separately tests actual
headless graph visualization with an unauthenticated loopback adapter.
These experiments are **not one integrated secure client stack**.

## Decision relevance

This experiment narrows #248's session/reattach and revocation questions
using actual runtime state and OS processes. It cannot decide the
protected client-principal policy or the final public API on its own.
A maintainable shared app-facing boundary is conditional on selecting
an actual caller trust profile, adapter placement and generation/authorization
issuer. Browser, native toolkit, Bevy and custom rendering remain
independent of this research-only transport.
