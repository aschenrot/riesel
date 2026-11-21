# Independent Cargo workspace inspection probe

**Status:** test-only evidence for [shared operational specification #75](https://github.com/aschenrot/riesel/issues/75). This is **not** an accepted CLI, application control protocol, schema/manifest contract, or shipped feature.

This package has its own `[workspace]` and uses the Riesel SDK by path. It therefore does **not** inherit the main repository's Cargo aliases or its `xtask`. It is deliberately a simple headless Rust app, without Node, GUI, Storage or Manager.

The fixture writes `RIESEL_PROBE_SETUP_MARKER` **before** calling Riesel's application-scoped CLI. Its handler writes a separate `RIESEL_PROBE_HANDLER_MARKER` only if the application actually runs.

The existing `cargo smoke` invokes this fixture as a separate Cargo workspace and asserts:

1. `--print-definition` emits valid SDK-derived application metadata, **executes the application binary's setup code**, but does **not** enter its app handler.
2. `--help` and invalid app flags likewise execute setup but do not enter the handler.
3. A normal invocation enters both setup and handler.
4. Reading a saved JSON definition is passive **as a file operation**; this does not authenticate the artifact or establish runtime liveness.

Research conclusion expected to be tested, not preapproved: **SDK preflight purity is narrower than executable inspection purity**. A future Manager/CLI must treat invoking an application binary to obtain metadata as explicitly authorized code execution, or obtain trusted/cached metadata through another accepted mechanism.

This fixture does not test malicious executables, the trustworthiness of cached metadata, provider or application-interface readiness, process-tree cleanup, session epochs, external Cargo package distribution or GUI capability exports. Those remain separate decision-critical experiments under #75 and its linked specifications.

The independent research workspace deliberately does not commit a lockfile. It is used with `--offline` after main workspace dependency validation; CI results are bounded to the recorded pinned toolchain and exact workflow run. Do not use this fixture as a model for shipping an application without a lockfile.
