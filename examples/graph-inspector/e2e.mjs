// Chromium against two real processes: Riesel graph owner and independent
// Node browser client, connected only through app-owned Unix T1 read queries.
import assert from "node:assert/strict";
import fs from "node:fs";
import http from "node:http";
import path from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.resolve(here, "../todo/web/package.json"));
const { chromium } = require("@playwright/test");
const log = path.resolve(process.argv[2] || "");
assert(fs.existsSync(log), "expected independent browser log");

let url;
for (let i = 0; i < 300; i++) {
  const output = fs.readFileSync(log, "utf8");
  url = output.match(/GRAPH_INSPECTOR_URL=(http:\/\/127\.0\.0\.1:\d+\/)/)?.[1];
  if (url) break;
  await delay(100);
}
assert(url, "no actual independent browser listener");
const unwantedHost = await new Promise((resolve, reject) => {
  const request = http.get(url, { headers: { Host: "nonlocal.attacker.test" } }, (response) => {
    response.resume();
    response.on("end", () => resolve(response.statusCode));
  });
  request.on("error", reject);
});
assert.equal(unwantedHost, 421);
assert.equal((await fetch(url + "snapshot", { method: "POST" })).status, 405);
assert.equal((await fetch(url + "activate")).status, 404);
const direct = await fetch(url + "snapshot");
assert.equal(direct.status, 200);
const graph = await direct.json();
assert.equal(graph.schema, "riesel.graph-inspection.v1");
assert.equal(graph.nodes.length, 3);
assert(graph.nodes.some((node) => node.id === "join" && node.depends_on.join() === "left,right"));

const browser = await chromium.launch({ headless: true });
try {
  const first = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await first.goto(url);
  await first.locator('[data-node="join"]').waitFor({ timeout: 10_000 });
  assert.match(await first.locator("#connection").innerText(), /Connected/);
  assert.equal(await first.locator('#node-list button').count(), 3);
  const select = first.locator('#node-list button[data-operation-id="join"]');
  await select.focus();
  await select.click();
  assert.equal(await select.getAttribute("aria-pressed"), "true");
  await first.waitForFunction(() =>
    document.querySelector('[data-node="join"]')?.getAttribute("class") === "node completed" &&
    document.getElementById("meta")?.textContent?.includes("finished"),
    null, { timeout: 15_000 });
  assert.equal(await first.evaluate(() => document.activeElement?.dataset?.operationId), "join");
  assert.match(await first.locator("#detail").innerText(), /Dependencies: left, right/);
  assert.equal(await first.locator("#node-list button").count(), 3);
  assert.equal(await first.locator("button").count(), 3, "no mutation/editor controls");
  await first.screenshot({
    path: path.resolve(process.env.GRAPH_GUI_SCREENSHOT || "/tmp/riesel-graph-inspector.png"),
    fullPage: true
  });
  await first.close();
  assert.equal((await fetch(url + "snapshot")).status, 200,
    "closing the browser must not stop the independent headless application");
  const second = await browser.newPage();
  await second.goto(url);
  await second.locator('[data-node="join"]').waitFor({ timeout: 10_000 });
  assert.equal(await second.locator('[data-node="join"]').getAttribute("class"),
    "node completed");
  const third = await browser.newPage();
  await third.goto(url);
  await third.locator('[data-node="join"]').waitFor({ timeout: 10_000 });
  assert.equal(await third.locator("#node-list button").count(), 3);
  console.log("PASS: independent headless app -> Unix T1 -> independent browser -> Chromium");
} finally {
  await browser.close();
}
