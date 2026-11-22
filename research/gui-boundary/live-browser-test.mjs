// Execute against the actual running Riesel example, never a mocked server.
// node research/gui-boundary/live-browser-test.mjs /path/to/live.log
import assert from 'node:assert/strict';
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.resolve(here, '../../examples/todo/web/package.json'));
const { chromium } = require('@playwright/test');
const logPath = path.resolve(process.argv[2] || '');
assert(logPath && fs.existsSync(logPath), 'expected a live example log file');

let origin;
const deadline = Date.now() + 30_000;
while (Date.now() < deadline) {
  const output = fs.readFileSync(logPath, 'utf8');
  const match = output.match(/RESEARCH LIVE INSPECTOR: (http:\/\/127\.0\.0\.1:\d+\/)/);
  if (match) { origin = match[1]; break; }
  await delay(100);
}
assert(origin, 'running Riesel example did not print its real bound loopback URL');

const checkHost = () => new Promise((resolve, reject) => {
  const req = http.get(origin, { headers: { Host: 'not-loopback.example' } },
    (res) => { res.resume(); res.on('end', () => resolve(res.statusCode)); });
  req.on('error', reject);
});
assert.equal(await checkHost(), 421, 'DNS rebinding / unexpected Host must be rejected');
assert.equal((await fetch(origin + 'snapshot', { method: 'POST' })).status, 405,
  'no HTTP mutation endpoint');

const browser = await chromium.launch({ headless: true });
try {
  const first = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await first.goto(origin);
  await first.locator('[data-node="left"]').waitFor({ timeout: 10_000 });
  assert.equal(await first.locator('#file-label').isVisible(), false,
    'live mode must not request an offline JSON import');
  assert.match(await first.locator('#connection').innerText(), /Connected/);
  const selected = first.locator('#node-list button[data-operation-id="join"]');
  await selected.click();
  const initial = await first.locator('#meta').innerText();
  assert.match(initial, /Session|Run/);
  await first.waitForFunction(() =>
    document.querySelector('[data-node="join"]')?.getAttribute('class') === 'node completed' &&
    document.getElementById('meta')?.textContent?.includes('Finished'),
    null, { timeout: 15_000 });
  assert.equal(await selected.getAttribute('aria-pressed'), 'true',
    'selected operation must survive live state changes');
  assert.equal(await first.evaluate(() => document.activeElement?.dataset?.operationId), 'join',
    'keyboard focus must survive live state changes');
  await first.locator('#after').click();
  assert.equal(await first.locator('[data-node="join"]').getAttribute('class'), 'node completed');
  assert.match(await first.locator('#meta').innerText(), /Finished/);
  await first.screenshot({
    path: path.resolve(process.env.GUI_LIVE_SCREENSHOT || '/tmp/gui-boundary-live.png'),
    fullPage: true
  });

  // Closing one GUI client cannot stop the independently running app.
  await first.close();
  assert.equal((await fetch(origin + 'snapshot')).status, 200);
  const second = await browser.newPage();
  await second.goto(origin);
  await second.locator('[data-node="join"]').waitFor({ timeout: 10_000 });
  assert.equal(await second.locator('[data-node="join"]').getAttribute('class'), 'node completed');

  // Two independent readers may coexist; neither has graph control authority.
  const third = await browser.newPage();
  await third.goto(origin);
  await third.locator('[data-node="join"]').waitFor({ timeout: 10_000 });
  assert.equal(await third.locator('#node-list button').count(), 3);
  assert.equal(await third.locator('button').count(), 5, 'only view and inspect controls');
  console.log('PASS: authoritative live poll, real execution, reconnect, concurrent clients, Host defense, no mutation');
} finally {
  await browser.close();
}
