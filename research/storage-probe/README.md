# Storage engine research probes

**Research only.** This independent Cargo workspace is not part of Riesel's production dependency graph. It compares limited SQLite (`rusqlite`) and redb transaction behavior using Rust 1.100.0 in [Stage 6 Issue #24](https://github.com/aschenrot/riesel/issues/24).

Run `cargo test --manifest-path research/storage-probe/Cargo.toml` (the first run fetches dependencies). The dedicated PR workflow validates the pinned Rust version.

These tests do **not** verify Riesel's provider lifecycle or model-registration contract, operating-system/power failures, durable-storage deployment on network filesystems, or all data-loss/corruption scenarios. Do not treat them as an accepted backend choice or production durability certification.
