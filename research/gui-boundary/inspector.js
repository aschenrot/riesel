"use strict";
// Research-only renderer. There is no endpoint, command transport, credential,
// executable document parser or assumed stable Riesel GUI schema.
const $ = (id) => document.getElementById(id);
const NS = "http://www.w3.org/2000/svg";
const KNOWN = new Set(["pending", "running", "completed", "failed", "cancelled", "unknown", "blocked"]);
let current = null;
let checkpoint = "final";
const liveMode = location.protocol === "http:" && location.hostname === "127.0.0.1";
let liveRequestInFlight = false;
let connectionText = "";
let selectedNodeId = null;
function connectionStatus(message) {
  if (message === connectionText) return;
  connectionText = message;
  $("connection").textContent = message;
}

function element(tag, text, cls) {
  const result = document.createElement(tag);
  if (text !== undefined) result.textContent = String(text);
  if (cls) result.className = cls;
  return result;
}
function svg(tag, attrs, text) {
  const node = document.createElementNS(NS, tag);
  for (const [key, value] of Object.entries(attrs || {})) node.setAttribute(key, String(value));
  if (text !== undefined) node.textContent = String(text);
  return node;
}
function validate(doc) {
  if (!doc || doc.schema !== "riesel.research.graph-inspection.v1" ||
      typeof doc.plan_id !== "string" || !Number.isSafeInteger(doc.revision) ||
      typeof doc.session_generation !== "string" || !Number.isSafeInteger(doc.run_sequence) ||
      typeof doc.session_expired !== "boolean" || typeof doc.trace_truncated !== "boolean" ||
      !["Created", "Running", "Finished", "Abandoned"].includes(doc.phase) ||
      !Number.isSafeInteger(doc.initial_sequence) || doc.initial_sequence < 0 ||
      !Number.isSafeInteger(doc.latest_sequence) || doc.latest_sequence < doc.initial_sequence ||
      !Array.isArray(doc.nodes) || doc.nodes.length < 1 || doc.nodes.length > 256 ||
      !Array.isArray(doc.transitions) || doc.transitions.length > 1024 ||
      doc.transitions.some((event) => !event || !Number.isSafeInteger(event.sequence) ||
        event.sequence <= 0 || event.sequence > doc.latest_sequence ||
        typeof event.change !== "string")) {
    throw new Error("Unsupported or oversized research inspection document");
  }
  const ids = new Set();
  for (const n of doc.nodes) {
    if (!n || typeof n.id !== "string" || !n.id || ids.has(n.id) ||
        typeof n.operation !== "string" || !Array.isArray(n.depends_on) ||
        n.depends_on.length > 256 || !KNOWN.has(n.initial) || !KNOWN.has(n.final) ||
        !Number.isSafeInteger(n.attempt) || n.attempt < 0) {
      throw new Error("Invalid operation or duplicated operation ID");
    }
    ids.add(n.id);
  }
  for (const n of doc.nodes) {
    if (n.depends_on.some((dep) => typeof dep !== "string" || !ids.has(dep) || dep === n.id)) {
      throw new Error("Unrecognized or self-referencing dependency");
    }
  }
  const depth = new Map(), visiting = new Set(), byId = new Map(doc.nodes.map((n) => [n.id, n]));
  function level(id) {
    if (depth.has(id)) return depth.get(id);
    if (visiting.has(id)) throw new Error("Cyclic graph");
    visiting.add(id);
    const value = Math.max(0, ...byId.get(id).depends_on.map((dep) => level(dep) + 1));
    visiting.delete(id);
    depth.set(id, value);
    return value;
  }
  for (const n of doc.nodes) level(n.id);
  return depth;
}
function stateOf(n) { return n[checkpoint]; }
function showNode(n) {
  selectedNodeId = n.id;
  for (const button of $("node-list").querySelectorAll("button")) {
    button.setAttribute("aria-pressed", String(button.dataset.operationId === n.id));
  }
  const target = $("detail");
  target.replaceChildren();
  const rows = [
    ["ID", n.id],
    ["Handler", n.operation + " · v" + n.operation_version],
    ["State", stateOf(n)],
    ["Attempt", n.attempt],
    ["Dependencies", n.depends_on.join(", ") || "none"],
    ["Confirmed effects", JSON.stringify(n.confirmed_prefixes || {})],
  ];
  if (n.error) rows.push(["Failure", n.error]);
  for (const [label, value] of rows) {
    const line = element("div");
    line.append(element("strong", label + ": "), document.createTextNode(String(value)));
    target.append(line);
  }
}
function graph(doc, depth) {
  const root = $("graph");
  root.replaceChildren();
  const columns = new Map();
  for (const n of doc.nodes) {
    const level = depth.get(n.id);
    if (!columns.has(level)) columns.set(level, []);
    columns.get(level).push(n);
  }
  const count = Math.max(...[...columns.values()].map((items) => items.length));
  const levels = Math.max(...depth.values()) + 1;
  const width = Math.max(410, levels * 260 + 60);
  const height = Math.max(290, count * 112 + 90);
  root.setAttribute("viewBox", "0 0 " + width + " " + height);
  const coords = new Map();
  for (const [level, nodes] of columns) {
    nodes.forEach((n, i) => coords.set(n.id, {
      x: 35 + 260 * level,
      y: (height - nodes.length * 112) / 2 + i * 112 + 14
    }));
  }
  const defs = svg("defs");
  const marker = svg("marker", { id: "arrow", markerWidth: 8, markerHeight: 8, refX: 7, refY: 4, orient: "auto" });
  marker.append(svg("path", { d: "M0 0 L8 4 L0 8 Z", fill: "#8fa3ba" }));
  defs.append(marker);
  root.append(defs);
  for (const n of doc.nodes) {
    const end = coords.get(n.id);
    for (const dependency of n.depends_on) {
      const start = coords.get(dependency);
      const x1 = start.x + 200, y1 = start.y + 34, x2 = end.x, y2 = end.y + 34;
      root.append(svg("path", {
        class: "edge",
        d: "M " + x1 + " " + y1 + " C " + (x1 + 34) + " " + y1 + ", " + (x2 - 34) + " " + y2 + ", " + x2 + " " + y2,
        "marker-end": "url(#arrow)"
      }));
    }
  }
  for (const n of doc.nodes) {
    const at = coords.get(n.id);
    const group = svg("g", { class: "node " + stateOf(n), "data-node": n.id });
    group.append(svg("rect", { x: at.x, y: at.y, width: 200, height: 68, rx: 10 }));
    group.append(svg("text", { x: at.x + 14, y: at.y + 27 }, n.id));
    group.append(svg("text", { x: at.x + 14, y: at.y + 49, class: "secondary" },
      stateOf(n) + " · " + n.operation));
    group.append(svg("title", {}, n.id + ": " + stateOf(n)));
    root.append(group);
  }
}
function render() {
  if (!current) return;
  const { doc, depth } = current;
  $("before").disabled = false;
  $("after").disabled = false;
  $("before").setAttribute("aria-pressed", String(checkpoint === "initial"));
  $("after").setAttribute("aria-pressed", String(checkpoint === "final"));
  const meta = $("meta");
  meta.replaceChildren();
  for (const [label, value] of [
    ["Plan", doc.plan_id], ["Revision", doc.revision], ["Run", doc.run_sequence], ["Phase", doc.phase],
    ["Session", doc.session_generation.slice(0, 8) + "…"], ["Latest sequence", doc.latest_sequence]
  ]) {
    const item = element("span");
    item.append(element("strong", label + " "), document.createTextNode(String(value)));
    meta.append(item);
  }
  graph(doc, depth);
  const listing = $("node-list");
  const focusedId = listing.contains(document.activeElement)
    ? document.activeElement?.dataset?.operationId : null;
  listing.replaceChildren();
  for (const n of doc.nodes) {
    const button = element("button");
    const state = element("span", stateOf(n), "status");
    button.dataset.operationId = n.id;
    button.append(element("span", n.id), state);
    button.setAttribute("aria-label", n.id + ", " + stateOf(n) + "; show details");
    button.addEventListener("click", () => showNode(n));
    listing.append(button);
  }
  showNode(doc.nodes.find((node) => node.id === selectedNodeId) || doc.nodes[0]);
  if (focusedId) {
    const focused = [...listing.querySelectorAll("button")]
      .find((button) => button.dataset.operationId === focusedId);
    focused?.focus({ preventScroll: true });
  }
  $("trace-warning").textContent = doc.trace_truncated
    ? "Transition history is incomplete: authoritative resnapshot is required. This file contains only the snapshot at export."
    : doc.session_expired
      ? "The observed application session expired; do not assume later state."
      : "Retained transition history. Missing acknowledgments must not be inferred.";
  const timeline = $("timeline");
  timeline.replaceChildren();
  for (const event of doc.transitions) {
    timeline.append(element("li", "#" + event.sequence + "  " + event.change));
  }
}
function fail(message) {
  current = null;
  selectedNodeId = null;
  $("graph").replaceChildren();
  $("meta").replaceChildren();
  $("timeline").replaceChildren();
  $("node-list").replaceChildren(element("p", message, "error"));
  $("detail").textContent = "No validated graph loaded.";
  $("before").disabled = true;
  $("after").disabled = true;
}
$("file").addEventListener("change", async (event) => {
  const file = event.target.files && event.target.files[0];
  if (!file) return;
  if (file.size > 2 * 1024 * 1024) { fail("File exceeds the 2 MiB inspection budget."); return; }
  try {
    const doc = JSON.parse(await file.text());
    current = { doc, depth: validate(doc) };
    checkpoint = "final";
    render();
  } catch (error) {
    fail("Cannot open inspection file: " + error.message);
  }
});
$("before").addEventListener("click", () => { checkpoint = "initial"; render(); });
$("after").addEventListener("click", () => { checkpoint = "final"; render(); });

if (liveMode) {
  $("file-label").hidden = true;
  $("connection").hidden = false;
  $("before").textContent = "Initial state";
  $("after").textContent = "Latest observed";
  $("input-help").textContent =
    "Read-only local research endpoint. Refreshes from the real application observer; " +
    "not authenticated against another local program and not a supported public interface. " +
    "Closing this browser does not stop the application.";
  connectionStatus("Connecting to local inspection source…");

  async function refreshLive() {
    if (liveRequestInFlight) return;
    liveRequestInFlight = true;
    try {
      const response = await fetch("/snapshot", {
        cache: "no-store",
        signal: AbortSignal.timeout(2000)
      });
      if (!response.ok) throw new Error("HTTP " + response.status);
      const text = await response.text();
      if (text.length > 2 * 1024 * 1024) throw new Error("Snapshot exceeds the 2 MiB budget");
      const doc = JSON.parse(text);
      const depth = validate(doc);
      const old = current?.doc;
      const sameRun = old && old.session_generation === doc.session_generation &&
        old.run_sequence === doc.run_sequence && old.plan_id === doc.plan_id &&
        old.revision === doc.revision;
      if (sameRun && doc.latest_sequence < old.latest_sequence) {
        throw new Error("Source returned a stale sequence");
      }
      const changed = !sameRun || old.latest_sequence !== doc.latest_sequence ||
        old.phase !== doc.phase || old.session_expired !== doc.session_expired ||
        old.trace_truncated !== doc.trace_truncated;
      current = { doc, depth };
      if (changed) render();
      connectionStatus(doc.session_expired
        ? "Session expired · last authoritative snapshot is not current"
        : "Connected · local read-only research · " + doc.phase);
    } catch (error) {
      // Preserve the last validated graph but never label it current.
      connectionStatus("Disconnected or invalid source · cached graph is stale (" +
        error.message + ")");
      if (!current) fail("No live inspection snapshot available.");
    } finally {
      liveRequestInFlight = false;
    }
  }
  refreshLive();
  setInterval(refreshLive, 500);
}
