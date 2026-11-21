# Independent native GUI process — research #60

**Status: experimental fixture, NOT an accepted GUI toolkit, SDK exporter, security design, or production UI.**
Main baseline at branch creation: `a35ac7c9` (2026-11-21).
This directory is an **independent Cargo workspace** and never alters the normal Riesel SDK/workspace dependency graph.
Specification: [#60](https://github.com/aschenrot/riesel/issues/60). Shared export/identity/authorization: [#75](https://github.com/aschenrot/riesel/issues/75).
Prior headless generated and resolved exports: merged [#81](https://github.com/aschenrot/riesel/pull/81) and [#82](https://github.com/aschenrot/riesel/pull/82).
Negative session/lost-response work: draft [#87](https://github.com/aschenrot/riesel/pull/87). This fixture does **not** modify any of their files.

## Concrete experiment and boundaries

`counter_resolved_facade_host` from the existing Counter example is a **real Riesel headless application**. Its `Application::async_handler` resolves an actual `AsyncCapability<AsyncCounterContract>` through the SDK and manually forwards the selected versioned RPC operations. It binds an OS-assigned loopback endpoint and waits for an **explicit test-only** `STOP` command on stdin. The server expects the final counter to equal five.

The separate `riesel-native-gui-host-research` executable is an **actual eframe/winit native window** with its own PID and main-thread event loop. It uses the generated typed Counter client on a dedicated background worker; the main thread only renders, handles keyboard/focus and dispatches commands through bounded channels. There is no native toolkit dependency in the headless app and no Manager dependency. The selected public Counter interface is not its internal Observer interface.

The first GUI reads count zero, performs increments, closes, and leaves the app alive. A newly launched GUI sees the same authoritative count, increments to five, then closes; the headless application only stops after the test driver explicitly writes `STOP`. A second run kills the backend while the GUI remains open, checks visible disconnection, and tests keyboard input while client I/O is delayed or unavailable.

**Trust and lifetime limitations:** this deliberately reuses the reference **unauthenticated loopback TCP** endpoint. The wire verifies the capability contract, **not** application identity, caller authorization, server session generation or a credential. The GUI is handed an explicit address by the test driver; it does not perform production discovery. Calls to the same address after backend replacement are unsafe without an independent external generation fence, as #87 demonstrates. Closing the GUI never sends a backend shutdown command. There is no automatic mutation retry; an error or timeout during mutation is conservatively surfaced as an **unknown outcome**, and periodic refresh is suppressed until the user explicitly requests reconciliation. A snapshot read can independently fail after a successful write; this prototype reports disconnection, rather than falsely treating that read failure as proof of rollback. No durable state or exactly-once guarantee is inferred.

## Reproduction

The isolated Linux job in [native-gui-research.yml](../../.github/workflows/native-gui-research.yml) installs the X11/OpenGL/Xvfb prerequisites, pins Rust 1.100.0, then runs:

```sh
cargo build -p riesel_counter_example --bin counter_resolved_facade_host
cargo build --manifest-path research/native-gui-host/Cargo.toml
LIBGL_ALWAYS_SOFTWARE=1 xvfb-run -a -s "-screen 0 1280x900x24" python3 research/native-gui-host/exercise.py
```

Requires `xdotool`, Xvfb, Mesa/OpenGL and Linux X11 development libraries. The script discovers the actual window, focuses it, sends Tab and Ctrl+I via X11, checks distinct process PIDs, observes snapshots, closes the window via X11, tests GUI reopening and backend survival, then tests visible backend failure and input processing. Its final marker is `NATIVE_GUI_RESEARCH_XVFB_PASS`; **do not assert success without an exact-head passing CI run and its log**.

Manual Linux desktop: start `cargo run -p riesel_counter_example --bin counter_resolved_facade_host`, copy the ephemeral `RESEARCH_FACADE_BOUND` address, then execute `cargo run --manifest-path research/native-gui-host/Cargo.toml -- <address>` in another terminal. Closing the GUI leaves the app running. The Counter fixture expects the total to equal five on test-only `STOP`. Its stdin control is not a general deployment interface.

## Comparable toolkit assessment (as of 2026-11-21)

All candidates use the same **separate-process / app-owned typed interface / GUI-only close** architecture. They are alternatives for a frontend, not alternative Riesel runtimes.

| Criterion | winit + separate renderer/widgets | egui / eframe | Iced | Slint | Tauri 2 |
| --- | --- | --- | --- | --- | --- |
| Integration and Rust authoring | Window/event APIs only; application must supply renderer, widgets and semantics | Immediate-mode Rust view code; fastest bounded test of event-loop/client isolation | Rust update/view, message/subscription architecture | Declarative Slint UI language, generated Rust bindings | Web frontend with Rust/webview host |
| Main thread and I/O | Own winit event loop, worker/proxy required | eframe owns winit loop; typed RPC worker in this experiment | Iced owns executor/event integration; test blocking boundaries | Slint event loop owns UI thread; Rust callbacks must not block | Webview/web frontend message bridge; separate Riesel app process still required |
| Renderer ownership | Most control, most implementation obligation | eframe glow or wgpu; backend selected by adapter | Toolkit rendering pipeline | Skia/software/Qt options depending backend/build | Platform webview; browser layout/render behavior |
| Keyboard, focus and accessibility | All widget navigation/assistive semantics must be implemented or integrated | Built-in widgets and AccessKit feature; **Linux AT-SPI/screen reader not certified by this test** | Keyboard UI supported; assistive technology coverage needs OS-specific verification | Built-in controls and accessibility backend support; validate Linux screen-reader behavior | Web accessibility semantics potentially reusable, but webview/OS integration must be tested |
| Cross-platform and packaging | Native window/renderer/libs vary by OS; high ownership | Native desktop targets with GL/GPU and platform dependencies; test on Win/macOS | Native desktop targets; check renderer/runtime/resource fit per platform | Desktop targets, possible Qt/backend dependencies, generated artifact build | Linux WebKitGTK (system packages), Windows WebView2, macOS WKWebView; JS/web assets plus Rust |
| Licensing | winit MIT/Apache-2.0; selected renderer/widgets independent | egui/eframe MIT/Apache-2.0 | Iced MIT | Slint licensing requires product/distribution review (GPL/commercial or other offered terms) | Tauri MIT/Apache-2.0; webview/platform notices apply |
| Long-term flexibility | Maximum custom render/UI control; expensive toolkit/platform maintenance | Strong tooling/inspector fit; immediate-mode ergonomics, larger UI/accessibility requirements need proof | Strong Rust state model; test complex native interaction/accessibility | Designed native UI language and tooling; build/license dependency | Strong if sharing Todo Svelte UI; web/runtime integration and browser security add layers |

Sources: [winit event-loop guidance](https://docs.rs/winit/latest/winit/event_loop/struct.EventLoop.html), [eframe 0.33.3 features](https://docs.rs/crate/eframe/0.33.3/features), [Iced guide](https://docs.iced.rs/), [Slint Rust/backends](https://docs.slint.dev/latest/docs/rust/slint/), [Slint licensing](https://slint.dev/pricing), [Tauri Linux prerequisites](https://v2.tauri.app/start/prerequisites/), [Tauri security capabilities](https://v2.tauri.app/security/capabilities/). Exact transitive platform compatibility is **not** proved by library documentation alone.

**Additional credible candidates:** GTK4 (strong Linux desktop accessibility but GTK runtime/packaging and portability obligations), Qt Rust bindings (mature native widgets, FFI/Qt licensing and distribution obligations). They do not change the app/GUI process boundary and are not justified as *second prototypes* by Counter alone.

### Test interpretation and decision recommendation

The prototype tests **event-loop feasibility** and native GUI lifetime independently of exporter choice. It is not a GUI platform approval.

- A passing Xvfb test proves a real Linux X11 window was created and received keyboard events, native eframe rendering was exercised, and the tested process/lifecycle behavior occurred. It is not evidence of macOS/Windows results, GPU/Wayland performance, native assistive screen-reader integration, safe authentication or standard-compliant accessible announcements.
- `Tab` focus and visible status/errors are automated; AccessKit is compiled as a feature. A manual AT-SPI screen-reader pass (e.g. Orca), zoom/contrast/focus review and Windows UIA/Narrator testing are still required before claiming usable accessible native UI.
- A worker-side `delay-ms` injection tests event-loop nonblocking during deliberate latency; **it is not a measured slow backend**. Backend kill tests real unavailable backend I/O.
- Exact branch-head CI, rustfmt/clippy, direct hardware/manual display behavior, and Windows pre-release verification must be separately recorded. The isolated Cargo resolver may update transitive versions unless a research lock is committed; capture/pin a validated research dependency closure before relying on repeated results.

**Proposed first integration vehicle for research:** eframe when the first application is a technical inspector, while retaining the same toolkit-neutral backend contract for Iced, Slint, winit/custom engine and optional Tauri. This is a choice of **experimental adapter only**, not the permanent native GUI product toolkit. If end-user accessibility and form-heavy workflows become primary, validate Slint/Iced or GTK/Qt on the same target usability requirements before selection.

**Still blocked for production:** externally verifiable application/session epoch (#75), exported-interface readiness, per-operation authorization, principal/peer authentication, trusted connection discovery, stale-session rejection, lost-reply reconciliation, installation/upgrades, and OS-specific desktop accessibility validation. No platform APIs, runtime transports, generated RPC code, roadmap milestones or production dependencies are changed in this research PR.
