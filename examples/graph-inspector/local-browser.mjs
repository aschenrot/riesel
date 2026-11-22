// Optional independent browser host for the accepted local T1 graph reader.
// It runs in a separate process and consumes only the published read protocol;
// neither this client nor the browser can admit, pause or execute graph work.
import assert from "node:assert/strict";
import fs from "node:fs/promises";
import http from "node:http";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const socketPath = process.argv[2];
if (!socketPath || !path.isAbsolute(socketPath)) {
  console.error("usage: node local-browser.mjs /absolute/app-owned/read.sock");
  process.exit(2);
}
const MAX_WIRE = 2 * 1024 * 1024;
const LOCAL_TIMEOUT = 3000;
const SCHEMA = "riesel.graph-inspection.v1";

function queryOwner(message) {
  return new Promise((resolve, reject) => {
    let done = false;
    let chunks = [];
    let count = 0;
    const connection = net.createConnection({ path: socketPath });
    const settle = (error, value) => {
      if (done) return;
      done = true;
      connection.destroy();
      error ? reject(error) : resolve(value);
    };
    connection.setTimeout(LOCAL_TIMEOUT, () => settle(new Error("owner request timeout")));
    connection.on("connect", () => {
      const body = Buffer.from(JSON.stringify({ wire_version: 1, ...message }), "utf8");
      if (body.byteLength > 128 * 1024) {
        settle(new Error("owner request too large"));
      } else {
        connection.end(body);
      }
    });
    connection.on("data", (data) => {
      count += data.byteLength;
      if (count > MAX_WIRE) settle(new Error("owner reply over budget"));
      else chunks.push(data);
    });
    connection.on("error", (error) => settle(error));
    connection.on("end", () => {
      if (done) return;
      try {
        const response = JSON.parse(Buffer.concat(chunks).toString("utf8"));
        if (response.wire_version !== 1) throw new Error("unsupported owner wire version");
        settle(null, response);
      } catch (error) { settle(error); }
    });
  });
}

function typedResult(reply, field) {
  if (reply.status !== "ok" || !reply.value || typeof reply.value !== "object") {
    throw new Error("owner read not currently available: " + String(reply.code));
  }
  if (!Object.hasOwn(reply.value, "Ok")) {
    throw new Error("owner query refused: " + JSON.stringify(reply.value.Err));
  }
  const payload = reply.value.Ok;
  if (!payload || payload.schema_version !== 1 || !payload.identity) {
    throw new Error("unsupported graph " + field + " schema");
  }
  return payload;
}

async function currentFrame() {
  const announcement = await queryOwner({ action: "describe" });
  if (announcement.status !== "descriptor") throw new Error("owner descriptor unavailable");
  const descriptor = announcement.descriptor;
  if (
    descriptor?.contract?.id !== "riesel.execution.graph_read" ||
    descriptor.contract.version !== 1 ||
    descriptor.exposure !== "non_sensitive_shared_user" ||
    !Array.isArray(descriptor.allowed_queries) ||
    descriptor.allowed_queries.join(",") !== "changes,snapshot"
  ) throw new Error("owner did not explicitly publish graph read v1");

  const snapshot = typedResult(await queryOwner({
    action: "query", descriptor, operation: "snapshot", arguments: {}
  }), "snapshot");
  if (!Array.isArray(snapshot.nodes) || snapshot.nodes.length > 256) {
    throw new Error("graph node projection exceeds supported budget");
  }
  const changeSet = typedResult(await queryOwner({
    action: "query", descriptor, operation: "changes",
    arguments: {
      session_generation: snapshot.identity.session_generation,
      run_sequence: snapshot.identity.run_sequence, cursor: 0
    }
  }), "changes");
  if (
    JSON.stringify(snapshot.identity) !== JSON.stringify(changeSet.identity) ||
    changeSet.latest_sequence < snapshot.latest_sequence ||
    !Array.isArray(changeSet.events) ||
    changeSet.events.length > 128
  ) throw new Error("incoherent or oversized graph read");

  const view = {
    schema: SCHEMA,
    plan_id: snapshot.identity.plan_id,
    revision: snapshot.identity.revision,
    session_generation: snapshot.identity.session_generation,
    run_sequence: snapshot.identity.run_sequence,
    publication_id: descriptor.publication_id,
    app_generation: descriptor.generation,
    phase: snapshot.phase,
    session_expired: snapshot.session_expired,
    latest_sequence: snapshot.latest_sequence,
    trace_truncated: changeSet.truncated,
    nodes: snapshot.nodes.map((node) => ({
      id: node.id, depends_on: node.depends_on,
      operation: node.binding, operation_version: node.binding_version,
      state: node.state, attempt: node.attempt,
      error: node.failure, confirmed_prefixes: node.confirmed_prefixes
    })),
    transitions: changeSet.truncated ? [] : changeSet.events
      .filter((event) => event.sequence <= snapshot.latest_sequence)
      .map((event) => ({
        sequence: event.sequence,
        change: [event.kind, event.operation_id, event.state, event.phase,
          event.failure, event.stream, event.cumulative]
          .filter((item) => item !== null && item !== undefined).join(" · ")
      }))
  };
  const serialized = JSON.stringify(view);
  if (Buffer.byteLength(serialized) > MAX_WIRE) throw new Error("view over budget");
  return serialized;
}

const html = await fs.readFile(path.join(here, "inspector.html"));
const script = await fs.readFile(path.join(here, "inspector.js"));
const server = http.createServer(async (request, response) => {
  const expectedHost = "127.0.0.1:" + server.address().port;
  const allowedOrigin = "http://" + expectedHost;
  response.setHeader("Cache-Control", "no-store, max-age=0");
  response.setHeader("X-Content-Type-Options", "nosniff");
  response.setHeader("Content-Security-Policy",
    "default-src 'none'; script-src 'self'; connect-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'");
  if (request.headers.host !== expectedHost) {
    response.writeHead(421).end();
    return;
  }
  if (request.headers.origin && request.headers.origin !== allowedOrigin) {
    response.writeHead(403).end();
    return;
  }
  if (request.method !== "GET" && request.method !== "HEAD") {
    response.writeHead(405).end();
    return;
  }
  const url = new URL(request.url, allowedOrigin);
  let body, type;
  if (url.pathname === "/" && !url.search) {
    body = html; type = "text/html; charset=utf-8";
  } else if (url.pathname === "/inspector.js" && !url.search) {
    body = script; type = "text/javascript; charset=utf-8";
  } else if (url.pathname === "/snapshot" && !url.search) {
    try { body = Buffer.from(await currentFrame()); }
    catch (error) {
      response.writeHead(503, { "Content-Type": "text/plain; charset=utf-8" });
      response.end("App read unavailable; reconnect and resnapshot");
      return;
    }
    type = "application/json; charset=utf-8";
  } else {
    response.writeHead(404).end();
    return;
  }
  response.writeHead(200, { "Content-Type": type, "Content-Length": body.byteLength });
  response.end(request.method === "HEAD" ? undefined : body);
});
server.listen(0, "127.0.0.1", () => {
  assert(server.address()?.port > 0);
  console.log("GRAPH_INSPECTOR_URL=http://127.0.0.1:" + server.address().port + "/");
  console.log("Local T1 read-only browser. Same-user programs are trusted, not authenticated.");
});
