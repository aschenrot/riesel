# Riesel — independent local execution graph inspector

This is a **read-only browser frontend** for the accepted P3/T1 app-owned
publication boundary. It runs in its own process and reads the same
versioned `riesel.execution.graph_read@1` capability as a native
`UnixReadClient`; it does not own or create the graph runner, a provider
resolver, a second scheduler, a Manager, or the application's lifecycle.

## Try the real graph (Unix, Rust 1.100.0, Node 22+)

In one terminal, run the headless application:

```sh
cargo run --locked -p riesel --features app-publication-unix --example graph_read_host -- 60
```

It prints an absolute `GRAPH_SOCKET=...` path in a private temporary
directory. In another terminal, use that **exact path** (the example
uses a fresh ephemeral one on each invocation):

```sh
node examples/graph-inspector/local-browser.mjs /path/from/GRAPH_SOCKET/read.sock
```

Open the displayed `GRAPH_INSPECTOR_URL=http://127.0.0.1:<port>/`.
The first executable starts a real validated three-node dependency graph,
publishes only `snapshot` and `changes` queries, and retains its
terminal state for the requested bounded number of seconds. The separate
Node process is a *frontend-specific browser adapter*, not a required
Riesel SDK implementation or universal GUI stack.

The viewer displays the validated nodes and dependencies, current
authoritative operation attempts/states, failure and effect-prefix
information when present, run/session/plan/revision, and bounded
retained changes. A missed event ring forces authoritative resnapshot;
unknown effects cannot be promoted to success. A replaced application
session or publication is treated as a distinct source, and failure to
reach the source shows a stale/disconnected status instead of calling
cached data current. The app does **not** terminate when a browser
window or the browser host disconnects.

Keyboard users have a skip link and focus-preserving operation list.
SVG edges are visual duplication of the textual dependency metadata,
not the sole accessible means of inspection. The frontend does not
offer mutation, pause, activation, authoring, or arbitrary RPC controls.

## Trust, boundaries, future adapters

This is **T1 only**: the application deliberately labels the selected
read model non-sensitive and trusts processes running under the same OS
user. The Unix path, 0600 permissions, runtime generation, HTTP Host
check, loopback binding, same-origin policy and CSP help define the
declared *local* and browser-origin scope. None authenticates the
application or excludes a malicious program with the same OS user,
which can connect to or impersonate local endpoints. **Never expose
private data, privileged operations or mutations through this profile.**
No protected individual client identity, remote exposure, Manager
authority, or Windows/macOS parity is asserted.

The browser host is replaceable. Native/Bevy/custom frontends can
consume the same app-owned versioned publication through their own
permitted adapters. Graph semantic and GUI layout revisions remain
separate; the browser does not rewrite the executable source.

## Verified workflow

The [#114 browser workflow](../../.github/workflows/graph-browser-client.yml)
runs the **real headless Rust example**, starts this separate browser
adapter, and uses actual headless Chromium for SVG nodes, retained
selection/focus, live terminal updates, Host/method defenses,
independent detach/reopen and concurrent clients. The screenshot
artifact captures rendered UI rather than a mock of the backend.
Publication and graph tests also run under the normal Linux validation.

This increment does **not** finish #114 or #258 until the complete
accepted inspector access, lifecycle, recovery, cross-platform and
reusable application read-model acceptance is evidenced. Visual plan
editing, separately authorized publication and activation remain #196/#195.
