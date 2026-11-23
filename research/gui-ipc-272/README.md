# #272 — local IPC candidate portability probe (research only)

This independent fixture is **not** part of Riesel's workspace, production SDK, or supported GUI protocol. It checks two Rust transport candidates (pinned `interprocess 2.4.9` and `tipsy 0.7.1`) against the **same** small bounded frame handler and a real independent Node.js client over Unix sockets / Windows named pipes. The application still owns `AppReadPublication` and the authoritative application session; neither candidate implements those policies.

## Reproduce

With Rust 1.100.0 and Node 22+:

```sh
cargo build --manifest-path research/gui-ipc-272/Cargo.toml
node research/gui-ipc-272/probe.mjs
```

The research workflow runs the same commands on `ubuntu-latest`, `windows-latest`, and `macos-latest`. No `--locked` claim until the experiment's dependency lock and source revision have been reviewed. This is a **functional** probe, not a throughput/latency benchmark. CI success is not production security verification.

The experimental wire encoding is a **4-byte big-endian length followed by exactly that many UTF-8 JSON bytes** (one request and one response per connection), bounded to 128 KiB request / (1 MiB + 4 KiB) response. It sends fragmented headers and bodies **without any client write-half shutdown**. Server errors for oversized/truncated requests close the connection. This is a candidate framing exercise, *not* an accepted `wire_version=2` or migration policy.

The fixture tests: real Rust server + independent Node client process; 8 concurrent readers; read-only operation denial; version mismatch; oversized declared length rejection; explicit stale-generation rejection after server restart under the same endpoint; no inferred mutation retry. The generated state is synthetic. It does **not** run Riesel's actual graph/Counter/Todo domain or any user data.

## Decision gaps

The fixture is necessary but **insufficient** for #272's acceptance. Before any library selection or production migration, verify:

- Actual OS-specific directory/socket permissions, Windows named-pipe ACL and remote-rejection, endpoint uniqueness and collision/cleanup behavior, along with negative cross-user tests.
- Same-UID endpoint impostor remains within T1's documented trust assumptions; do **not** report tested endpoint names, UUIDs or ACLs as protected per-client/server authentication.
- Real `AppReadPublication` admission/withdrawal/reconnect, actual graph loss/full resnapshot and Node browser compatibility; wire-version migration or explicit opt-in/dual-reader behavior for existing Unix v1 clients.
- Direct Tokio baseline under the same tests and dependency/MSRV/API maintenance comparison; compare latency and allocations if decision-relevant.
- Rust formatting, exact integrated revision, relevant code review and target-platform tests before production release. Research success cannot close #258 or #272 or authorize writes, private data, Manager operations or plan activation.

[Investigation #272](https://github.com/aschenrot/riesel/issues/272) owns the research outcome and eventual maintainer decision. The accepted architecture remains [#248 P3/T1](https://github.com/aschenrot/riesel/issues/248).
