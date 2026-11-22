# Execution graph inspection — offline and live research #248

**Status:** Research-only visual read-model proof, with an optional **unprotected
loopback live reader**. Neither an authenticated or supported external attachment,
nor a new SDK/wire format, publisher, credential, frontend standard, or completed
GUI implementation issue. Do not use this untrusted JSON file as an
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
authority; browser file access is user-selected. No data is uploaded. Imported JSON cannot authenticate its own provenance; only the actual headless exporter and the caller’s trusted file-handling process can establish its source.

A reproducible real-browser smoke and screenshot using the repository's
existing locked Playwright dependency run in the separate research CI:

```sh
(cd examples/todo/web && npm ci && npx playwright install chromium)
node research/gui-boundary/browser-test.mjs graph.json
```

## View a real running graph (optional local research adapter)

Run the same example from the repository root. On Windows PowerShell, enter
the command on **one line**:

```powershell
cargo run --locked -p riesel --features async-tcp --example execution_graph_inspection_export -- --live 45
```

The example prints a unique local URL in the form
`http://127.0.0.1:<OS-selected-port>/`. Open that URL in a desktop browser
while the command is running. The viewer automatically reads the live Riesel
`GraphRunObserver` snapshot every 500 ms; no file selection or GUI-driven
execution is required. The backend starts admission after three seconds,
executes two parallel delayed operations followed by their dependency join,
and retains the finished state for the requested 45 seconds. The final
argument is the post-execution hold period (1–600 seconds). Once the process
ends, the browser must indicate disconnection/staleness. Closing or reopening
a browser does not stop the application.

This server is **not protected against other programs running under the same
user account**. It is a deliberately limited, GET-only, read-only **research**
adapter bound to `127.0.0.1` on an OS-selected port, with exact Host checking,
same-origin browser behavior, no cross-origin grant and no executable mutation
routes. Origin restrictions and loopback do **not** constitute caller
authentication. Do not place sensitive application data behind this adapter,
treat the session UUID as a secret, or use it as a public/privileged GUI
transport. No permanent Riesel GUI API, server dependency, or production
security contract is established.

The CI smoke executes the actual headless Rust graph and independent
Chromium clients, verifies current snapshots, disconnection safety boundaries,
read-only methods, independent browser detach/reopen and multiple readers,
and retains real screenshots. The native/Bevy/web frontend decision remains
separate from the authoritative observation boundary.

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

- **No supported authenticated attachment/discovery:** the live example
  has a local read-only snapshot endpoint and browser reconnect-by-URL, but no
  independently verified client identity, grant, published interface readiness,
  subscription, protocol stability or application recovery reattachment.
- **No authenticated principal or grant:** offline file access inherits
  local filesystem/browser policy; the live loopback adapter is accessible
  to other local processes. Same-user readers are not isolated.
  The serialized runtime generation is correlation, not a credential.
- **No accepted GUI schema:** the version label is restricted to this research
  fixture. Its read-only serializer is manually authored; there is no new
  exported Rust DTO or reusable app exporter yet.
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
