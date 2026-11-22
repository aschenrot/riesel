# Shared operational contract — research probe for #75

This directory is **research only**. It adds no production code, crate, dependency, public SDK API, daemon, platform authentication or exporter.

The Python standard-library gateway forwards permitted operations to the existing real `counter_resolved_facade_host`, whose Riesel `Application::async_handler` receives the actual SDK-resolved `AsyncCapability<AsyncCounterContract>`. The independent Python gate models epochs, attachments and operation checks; it **does not** establish that Riesel itself issues or revokes the externally represented epoch. A separate research-only Rust fixture uses actual Riesel managed-service recovery while keeping the application OS process and its test listener alive; its per-handler-invocation epoch is still **app/fixture-issued**, not a public SDK/runtime contract. Its same-UID negative test demonstrates why an owner-only credential file cannot satisfy this threat model.

From the repository root:

```sh
cargo validate --check
cargo smoke
python3 research/operational-contract/probe.py --real
python3 research/operational-contract/same_pid_probe.py
# To debug the Python-only apparatus without Rust:
python3 research/operational-contract/probe.py --model
```

See [REPORT.md](REPORT.md) for exact evidence, limitations, architecture alternatives and unresolved acceptance gates. The separate #87 negative fixtures are referenced rather than copied or modified.
