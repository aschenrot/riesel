# Cross-application GUI publication boundary comparator (#248)

**Research-only.** This is an isolated policy and authoring-cost experiment
against the actual Riesel SDK on a current-`main` baseline. It is not an
exporter API, independently authenticated GUI attachment, OS-verified caller,
transport adapter, runtime-issued publication epoch or authorization grant.
All principals and exporter generations here are **simulated** and are explicitly
unsafe for public or privileged publication.

The comparator reuses the **same `ResearchExport` policy** from [PR #91](https://github.com/aschenrot/riesel/pull/91), now verified against three
distinct application owners:

1. Counter: actual resolved `AsyncCapability<AsyncCounterContract>`; explicit
   manual `AsyncRpcHandler` relay for 3 operations.
2. Todo: actual SDK `Records<TaskData>` dependency with a deliberate
   two-operation `#[capability]` app-facing facade and generated dispatcher.
3. Execution graph: actual application-session-created graph runner and
   `GraphRunObserver`; deliberate read-only versioned `#[capability]`
   `GraphRead` facade with the generated dispatcher. The gateway admits a
   typed summary before and after a real two-node dependent execution;
   the GUI does not schedule or control nodes.

Run with Rust 1.100.0 from the root:

```sh
cargo fmt --manifest-path research/application-export/Cargo.toml -- --check
cargo test --manifest-path research/application-export/Cargo.toml -- --nocapture
python3 research/application-export/measure.py
```

CI must verify the **exact PR head** and the default Linux Validate/smoke.
The research workspace has its own manifest and is not a new SDK crate.

The gateway's atomic accepting flag only verifies **sequential** withdrawal.
Concurrent revocation can race the subsequent dispatch; the real #157
runtime-owned ingress permit and a synchronized grant admission policy would
be necessary for a production guarantee.

This test proves *policy-shape reuse*, not wire security or process attachment.
It demonstrates three explicit versioned interfaces with one gateway
implementation; generated facade dispatch can avoid the per-operation relay
where application-authored handlers are available, but the SDK-resolved
Counter forwarder still manually maps each method. The graph method returns
a bounded typed summary, **not** the full graph descriptor or the complete
visual timeline; [PR #250](https://github.com/aschenrot/riesel/pull/250)
separately verifies the actual independent Chromium read model using a
different, unauthenticated local adapter. The two proofs must not be called
one accepted or secure transport.

A same-UID local program can still impersonate a simulated permitted
principal; the actual negative controls are [PR #149](https://github.com/aschenrot/riesel/pull/149) and [PR #151](https://github.com/aschenrot/riesel/pull/151). Runtime-issued
same-process generation/revocation is already implemented internally
under [#157](https://github.com/aschenrot/riesel/issues/157), but the
research `ResearchExport` does **not** consume it. Consequently this
does not close #248 or #114 or authorize the public API.

See `REPORT.md` for the comparison and remaining decision gates.
