# Application export research probe — #60 / #75

This is a **disposable, isolated research Cargo workspace**, not a product crate, published API, endorsed local authentication system or native GUI.

From the repository root, using Rust 1.100.0:

```sh
cargo validate --check
cargo smoke
cargo test --manifest-path research/application-export/Cargo.toml --offline -- --nocapture
python3 research/application-export/measure.py
```

It reuses a real Riesel-resolved Counter async capability and a real Riesel-resolved Todo `Records<TaskData>` dependency inside two distinct `Application::async_handler` sessions. Counter uses a manual `AsyncRpcHandler` relay, exercising the known code-generation impedance from #82; Todo declares an app-facing `#[capability]` and uses the existing generated dispatcher over its domain facade. Both then use the same `ResearchExport` policy, no second resolver and no duplicate handwritten wire schema for the declared interfaces.

The gateway checks selected contract/version, application+interface identity, a **simulated** session generation, allowlisted operations, a **simulated trusted principal**, and admission/revocation; it classifies lost replies on commands as unknown outcome. It does **not** run a listener or identify a real OS peer. The `SimulatedPrincipal::for_test` constructor must never be used as an authentication mechanism. Tests deliberately show the unreliability of failing to revoke an old gateway.

PRs #81/#82 already prove the independent-process typed transport and app-owned lifetime; draft #87 proves negative stale-rebind and withheld-reply cases on the actual loopback transport. None establish a safe public exporter. The new fixture isolates cross-application policy reuse **without duplicating #75's network negative fixtures or accepting a new wire protocol**.

See [REPORT.md](REPORT.md) for comparative decisions, exact #75 dependencies and implementation-ready acceptance tests.
