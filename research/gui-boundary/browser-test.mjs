// Run using the pinned browser dependency from examples/todo/web:
// node research/gui-boundary/browser-test.mjs /path/to/export.json
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath, pathToFileURL } from 'node:url';
const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(path.resolve(here, '../../examples/todo/web/package.json'));
const { chromium } = require('@playwright/test');
const input = path.resolve(process.argv[2] || '');
assert(fs.existsSync(input), 'pass the Rust-produced inspection JSON as argument');
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'riesel-gui-readonly-'));
const browser = await chromium.launch({ headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 900 } });
  await page.goto(pathToFileURL(path.join(here, 'inspector.html')).href);
  await page.locator('#file').setInputFiles(input);
  await page.locator('[data-node="join"]').waitFor();
  assert.equal(await page.locator('#node-list button').count(), 3);
  assert.equal(await page.locator('[data-node="join"]').getAttribute('class'), 'node completed');
  await page.locator('#before').click();
  assert.equal(await page.locator('[data-node="join"]').getAttribute('class'), 'node pending');
  await page.locator('#after').click();
  await page.getByRole('button', { name: /join, completed/ }).click();
  assert.match(await page.locator('#detail').innerText(), /left, right/);
  assert((await page.locator('#timeline li').count()) > 0);
  await page.screenshot({ path: path.resolve(process.env.GUI_SCREENSHOT || path.join(tmp, 'inspector.png')), fullPage: true });

  // A GUI cannot turn an uncertain state into successful green completion.
  const original = JSON.parse(fs.readFileSync(input, 'utf8'));
  const uncertain = structuredClone(original);
  uncertain.nodes.find(n => n.id === 'join').final = 'unknown';
  uncertain.trace_truncated = true;
  uncertain.nodes.find(n => n.id === 'join').operation = '<img src=x onerror=alert(1)>';
  const changed = path.join(tmp, 'uncertain.json');
  fs.writeFileSync(changed, JSON.stringify(uncertain));
  await page.locator('#file').setInputFiles(changed);
  await page.locator('[data-node="join"]').waitFor();
  assert.equal(await page.locator('[data-node="join"]').getAttribute('class'), 'node unknown');
  assert.match(await page.locator('#trace-warning').innerText(), /incomplete/);
  assert.equal(await page.locator('#graph img').count(), 0);
  assert.equal(await page.locator('#detail img').count(), 0);

  // Missing source evidence cannot be silently presented as an intact
  // transition history or a current application session.
  const missingEvidence = structuredClone(original);
  delete missingEvidence.trace_truncated;
  const missing = path.join(tmp, 'missing-evidence.json');
  fs.writeFileSync(missing, JSON.stringify(missingEvidence));
  await page.locator('#file').setInputFiles(missing);
  assert.match(await page.locator('#node-list').innerText(), /Unsupported/);

  // Unsupported schema fails closed; it cannot masquerade as an executable
  // S2 document or an accepted public GUI connection format.
  uncertain.schema = 'riesel.future.execution.v99';
  const invalid = path.join(tmp, 'invalid.json');
  fs.writeFileSync(invalid, JSON.stringify(uncertain));
  await page.locator('#file').setInputFiles(invalid);
  assert.match(await page.locator('#node-list').innerText(), /Unsupported/);
  console.log('PASS: real snapshot, dependencies, phases, keyboard list, uncertainty, loss, safe text and invalid schema');
} finally {
  await browser.close();
  fs.rmSync(tmp, { recursive: true, force: true });
}
