# Independent Cargo workspace inspection probe

**Status:** test-only evidence for [shared operational specification #75](https://github.com/aschenrot/riesel/issues/75). This is **not** an accepted CLI, application control protocol, schema/manifest contract, or shipped feature.

This fixture is a **template** with its own `[workspace]` and a path dependency on the Riesel SDK. Because nested directories can still inherit ancestor Cargo configuration, the smoke runner **copies the fixture into an external operating-system temporary directory**, replaces only its SDK dependency path with the absolute local checkout path, and builds/runs it there. It therefore does not inherit the main repository's Cargo aliases, `xtask` or workspace membership. It is a headless Rust app, without Node, GUI, Storage or Manager.

The fixture writes `RIESEL_PROBE_SETUP_MARKER` **before** calling Riesel's application-scoped CLI. Its handler writes a separate `RIESEL_PROBE_HANDLER_MARKER` only if the application actually runs.

The existing `cargo smoke` builds and invokes the external fixture workspace and asserts:

1. `--print-definition` emits valid SDK-derived application metadata, **executes the application binary's setup code**, but does **not** enter its app handler.
2. `--help` and invalid app flags likewise execute setup but do not enter the handler.
3. A normal invocation enters both setup and handler.
4. Reading a saved JSON definition is passive **as a file operation**; this does not authenticate the artifact or establish runtime liveness.

Research conclusion expected to be tested, not preapproved: **SDK preflight purity is narrower than executable inspection purity**. A future Manager/CLI must treat invoking an application binary to obtain metadata as explicitly authorized code execution, or obtain trusted/cached metadata through another accepted mechanism.

This fixture does not test malicious executables, the trustworthiness of cached metadata, provider or application-interface readiness, process-tree cleanup, session epochs, external Cargo package distribution or GUI capability exports. Those remain separate decision-critical experiments under #75 and its linked specifications.

The external research workspace deliberately generates its own disposable lockfile and builds with `--offline` after main workspace dependency validation. CI results are bounded to the recorded pinned toolchain, cached dependency set and exact workflow run. Do not use this fixture as a model for shipping an application without a committed lockfile.
