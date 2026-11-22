# Offline execution graph inspection — research #248

**Status:** Research-only visual read-model proof. Neither a supported live
attachment nor a new SDK/wire format, publisher, credential, frontend standard
or completed GUI implementation issue. Do not use this untrusted JSON file as an
S2 executable source, a session token or permission to call the application.

## Reproduce from the current repository

Use Rust 1.100.0:

```sh
cargo run --locked -p riesel --features async-tcp --example execution_graph_inspection_export -- graph.json
python3 research/gui-boundary/verify.py graph.json
```

Open `research/gui-boundary/inspector.html` in a desktop browser
(as a local file), choose the generated `graph.json` using the file input,
and inspect dependencies, before/after node states, run/session correlation and
the retained transition timeline. The viewer requires no network service,
package install, Riesel Manager, special renderer or application mutation
authority; browser file access is user-selected. No data is uploaded.

A reproducible real-browser smoke and screenshot using the repository's
existing locked Playwright dependency run in the separate research CI:

```sh
(cd examples/todo/web && npm ci && npx playwright install chromium)
node research/gui-boundary/browser-test.mjs graph.json
```

## What this proves

- The existing **real** Riesel session-owned execution graph produces an
  authoritative snapshot and immutable validated dependency description.
- A separate browser process can visualize the exported state without
  starting providers, changing graph semantics, loading Rust code or issuing
  commands.
- The browser displays bounded transitions, confirmed node results and
  alternate pending/unknown states; it indicates missing trace coverage rather
  than treating absent events as success.
- A simple SVG diagram and accessible keyboard-operated operation list can
  coexist. File input and rendering use browser APIs and text nodes, not HTML
  injection.

## What this deliberately does not prove

- **No live attachment or discovery:** this is a completed-run file export;
  there is no reconnect, current-session readiness, event subscription or
  source of fresh snapshots after the export.
- **No authenticated principal, grant or transport:** file access inherits
  local filesystem/browser policy. Same-user file readers are not isolated.
  The serialized runtime generation is correlation, not a credential.
- **No accepted GUI schema:** the version label is restricted to this research
  fixture. Its serializer is manually authored and duplicates a little display
  mapping; there is no new exported Rust DTO or reusable app exporter yet.
- **No graphical editor or activation:** the GUI cannot publish, mutate, step,
  schedule, activate or import executable plans.
- **No full scale/portability proof:** the UI caps imports at 2 MiB, 256
  operations and 1024 events, for a deliberately bounded specimen. The graph
  renderer and assistive-technology support beyond browser keyboard semantics
  require workload-specific evaluation. This is not a Windows/macOS release gate.

## Architectural implication for #248

The graph inspector needs only selected, versioned read projections from an
authoritative Riesel app. It does **not** need to become a provider, plan author
or executor. Real native/browser GUI attachment still needs a supported
application-facing publication and access boundary before claiming #114
complete. This fixture helps test one presenter while leaving the eventual
transport, exporter contract and frontend technology open. The existing
Counter/Todo facade research remains the separate evidence for reusable
operation exposure; do not extrapolate its simulated principals to production.
