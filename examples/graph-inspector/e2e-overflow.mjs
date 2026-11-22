// Real 80-node graph overflow and backend disappearance in Chromium.
// No mocked graph state, event loss or endpoint replacement.
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.resolve(here, "../todo/web/package.json"));
const { chromium } = require("@playwright/test");
const log = path.resolve(process.argv[2] || "");
const ownerPid = Number(process.env.GRAPH_OWNER_PID);
assert(fs.existsSync(log) && Number.isSafeInteger(ownerPid) && ownerPid > 1);
let url;
for (let i = 0; i < 300; i++) {
  url = fs.readFileSync(log, "utf8")
    .match(/GRAPH_INSPECTOR_URL=(http:\/\/127\.0\.0\.1:\d+\/)/)?.[1];
  if (url) break;
  await delay(100);
}
assert(url, "independent browser server not ready");
const browser = await chromium.launch({ headless: true });
try {
  const view = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await view.goto(url);
  await view.waitForFunction(() =>
    document.getElementById("trace-warning")?.textContent?.includes("History was truncated") &&
    document.querySelectorAll("#node-list button").length === 80 &&
    document.querySelector('[data-node="node_079"]')?.getAttribute("class") === "node completed",
    null, { timeout: 15_000 });
  assert.match(await view.locator("#trace-warning").innerText(), /recovered full authoritative snapshot/);
  assert.equal(await view.locator("#node-list button").count(), 80);
  assert.equal(await view.locator('#timeline li').count(), 0,
    "truncated history must not be presented as a complete event replay");
  assert.equal(await view.locator('#node-list button[data-operation-id="node_079"]').innerText()
    .then(x => x.includes("completed")), true);
  // Simulate a genuine owner process disappearing after a known snapshot.
  // Cached nodes may remain visible, but must no longer be described as current.
  process.kill(ownerPid, "SIGTERM");
  await view.waitForFunction(() =>
    document.getElementById("connection")?.textContent?.includes("Disconnected") &&
    document.getElementById("connection")?.textContent?.includes("stale"),
    null, { timeout: 10_000 });
  assert.equal(await view.locator("#node-list button").count(), 80,
    "keep accessible last-known graph while marking it stale");
  console.log("PASS: real graph trace overflow, full resnapshot and browser-visible disconnect");
} finally {
  await browser.close();
}
