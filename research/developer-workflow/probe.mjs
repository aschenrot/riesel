// Research-only executable probe for issue #68; no general-purpose launcher is shipped.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile, access } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'node:net';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const web = path.join(root, 'examples/todo/web');
const sdk = path.join(root, 'crates/riesel');
const target = process.env.CARGO_TARGET_DIR
  ? path.resolve(process.env.CARGO_TARGET_DIR)
  : path.join(root, 'target');
const exeSuffix = process.platform === 'win32' ? '.exe' : '';
const todo = path.join(target, 'debug', 'todo_http' + exeSuffix);
const scratch = await mkdtemp(path.join(tmpdir(), 'riesel-dev68-'));
const owned = new Set();
const readyHeadless = /^DEV68_HEADLESS_READY$/;
const readyTodo = /^TODO_HTTP_READY (127\.0\.0\.1:[0-9]+)$/;

function execChecked(bin, args, options = {}) {
  const result = spawnSync(bin, args, {
    cwd: root, encoding: 'utf8', timeout: 180000,
    ...options
  });
  if (result.error) throw result.error;
  return result;
}

function spawnOwned(bin, args, options = {}) {
  const child = spawn(bin, args, {
    cwd: root,
    stdio: ['pipe', 'pipe', 'pipe'],
    ...options
  });
  child.dev68Stderr = '';
  child.stderr.on('data', chunk => {
    child.dev68Stderr = (child.dev68Stderr + chunk.toString()).slice(-8192);
  });
  owned.add(child);
  child.once('exit', () => owned.delete(child));
  return child;
}

function waitReady(child, matcher, duration = 12000) {
  return new Promise((resolve, reject) => {
    let pending = '';
    let settled = false;
    const timer = setTimeout(() => finish(new Error('readiness timeout')), duration);
    function finish(error, value) {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      child.stdout.off('data', onData);
      child.off('error', onError);
      child.off('exit', onExit);
      if (error) reject(error);
      else resolve(value);
    }
    function onError(error) { finish(error); }
    function onExit(code, signal) {
      finish(new Error('process exited before readiness: ' + code + '/' + signal +
        '; stderr: ' + child.dev68Stderr));
    }
    function onData(chunk) {
      pending += chunk.toString();
      if (pending.length > 4096) {
        finish(new Error('oversized readiness output'));
        return;
      }
      let index;
      while ((index = pending.indexOf('\n')) >= 0) {
        const line = pending.slice(0, index).trimEnd();
        pending = pending.slice(index + 1);
        const match = matcher.exec(line);
        if (match) return finish(null, match[1] ?? line);
        if (line.length > 2048) return finish(new Error('oversized readiness line'));
      }
    }
    child.stdout.on('data', onData);
    child.once('error', onError);
    child.once('exit', onExit);
  });
}

async function stopOwned(child, mode = 'signal') {
  if (child.exitCode !== null || child.signalCode !== null) {
    return { code: child.exitCode, signal: child.signalCode };
  }
  const completed = new Promise(resolve =>
    child.once('exit', (code, signal) => resolve({ code, signal }))
  );
  if (mode === 'stdin') child.stdin.write('stop\n');
  else child.kill('SIGINT');
  const result = await Promise.race([
    completed,
    new Promise(resolve => setTimeout(() => resolve(null), 8000))
  ]);
  if (result) return result;
  child.kill('SIGKILL');
  await completed;
  throw new Error('owned child failed to stop gracefully within eight seconds');
}

async function expectEarlyFailure(args, expected) {
  const output = execChecked(todo, args, { timeout: 20000 });
  assert.notEqual(output.status, 0, 'invalid backend startup unexpectedly succeeded');
  assert(!output.stdout.includes('TODO_HTTP_READY'), 'invalid backend advertised readiness');
  assert(output.stderr.includes('Todo HTTP startup failed'), 'missing startup diagnostic');
  console.log('PASS early failure: ' + expected);
}

async function todoStart(db) {
  const child = spawnOwned(todo, [
    '--db', db, '--port', '0', '--assets', path.join(web, 'build')
  ]);
  const authority = await waitReady(child, readyTodo);
  const url = 'http://' + authority;
  const api = await fetch(url + '/api/v1/tasks', { signal: AbortSignal.timeout(4000) });
  assert.equal(api.status, 200, 'ready announcement but API was unavailable');
  assert(Array.isArray((await api.json()).records));
  const staticPage = await fetch(url + '/', { signal: AbortSignal.timeout(4000) });
  assert.equal(staticPage.status, 200, 'ready announcement but static assets were unavailable');
  assert((await staticPage.text()).includes('<html'), 'static browser content missing');
  return { child, url };
}

async function verifyHeadless() {
  const outside = path.join(scratch, 'external-headless');
  await mkdir(path.join(outside, 'src'), { recursive: true });
  await copyFile(path.join(root, 'research/developer-workflow/headless/src/main.rs'),
    path.join(outside, 'src/main.rs'));
  const template = await readFile(
    path.join(root, 'research/developer-workflow/headless/Cargo.toml'), 'utf8');
  const needle = '"../../../crates/riesel"';
  assert(template.includes(needle), 'SDK path substitution anchor changed');
  await writeFile(path.join(outside, 'Cargo.toml'),
    template.replace(needle, JSON.stringify(sdk)));
  const manifest = path.join(outside, 'Cargo.toml');
  const env = { ...process.env, CARGO_TARGET_DIR: target };

  const metadata = execChecked('cargo', ['metadata', '--no-deps', '--format-version', '1',
    '--manifest-path', manifest], { cwd: outside, env });
  assert.equal(metadata.status, 0, metadata.stderr);
  const parsed = JSON.parse(metadata.stdout);
  assert.equal(path.resolve(parsed.workspace_root), outside);
  assert.equal(parsed.workspace_members.length, 1);
  assert.equal(parsed.packages.length, 1);
  assert(!parsed.packages.some(pkg => pkg.name === 'xtask'));
  console.log('PASS external Cargo metadata: own workspace, no copied xtask');

  const build = execChecked('cargo', ['build', '--offline', '--manifest-path', manifest],
    { cwd: outside, env });
  assert.equal(build.status, 0, build.stderr);
  const bin = path.join(target, 'debug', 'riesel-developer-workflow-headless-probe' + exeSuffix);
  await access(bin);
  const setup = path.join(scratch, 'setup.marker');
  const handler = path.join(scratch, 'handler.marker');
  const markers = { ...env, DEV68_SETUP_MARKER: setup, DEV68_HANDLER_MARKER: handler };

  const inspect = execChecked(bin, ['--print-definition'], { cwd: outside, env: markers });
  assert.equal(inspect.status, 0, inspect.stderr);
  assert.equal(JSON.parse(inspect.stdout).app.name, 'DeveloperWorkflowHeadlessProbe');
  await access(setup);
  await assert.rejects(access(handler));
  const invalid = execChecked(bin, ['--invalid-flag'], { cwd: outside, env: markers });
  assert.notEqual(invalid.status, 0);
  await assert.rejects(access(handler));
  const missingDeployment = execChecked(bin, [
    '--inspect-deployment', path.join(scratch, 'missing-deployment.json')
  ], { cwd: outside, env: markers });
  assert.notEqual(missingDeployment.status, 0);
  await assert.rejects(access(handler));
  console.log('PASS executable inspection executes setup, not handler; invalid args fail');

  const headless = spawnOwned(bin, [], { cwd: outside, env: markers });
  await waitReady(headless, readyHeadless);
  await access(handler);
  assert.equal((await stopOwned(headless, 'stdin')).code, 0);
  console.log('PASS headless foreground launch and application-specific stdin stop');

  const interrupted = spawnOwned(bin, [], { cwd: outside, env: markers });
  await waitReady(interrupted, readyHeadless);
  const stopped = await stopOwned(interrupted, 'signal');
  console.log('OBSERVED headless SIGINT exit=' + JSON.stringify(stopped) +
    ' (not evidence of generic graceful shutdown)');
}

async function verifyReadinessNegatives() {
  const bad = spawnOwned(process.execPath, ['-e', 'console.log("NOT_READY");']);
  await assert.rejects(waitReady(bad, readyTodo, 1500), /before readiness/);
  const tooLarge = spawnOwned(process.execPath, [
    '-e', 'process.stdout.write("X".repeat(5000));setTimeout(()=>{},5000);'
  ]);
  await assert.rejects(waitReady(tooLarge, readyTodo, 1500), /oversized/);
  await stopOwned(tooLarge);
  const stalled = spawnOwned(process.execPath, ['-e', 'setTimeout(()=>{},5000);']);
  await assert.rejects(waitReady(stalled, readyTodo, 150), /timeout/);
  await stopOwned(stalled);
  // A real TCP listener deliberately refuses to speak HTTP: no hard-coded
  // assumption about which ports are free or unreachable on the CI host.
  const falseInterface = createServer(socket => socket.destroy());
  await new Promise((resolve, reject) =>
    falseInterface.once('error', reject).listen(0, '127.0.0.1', resolve));
  try {
    const fakeAddress = '127.0.0.1:' + falseInterface.address().port;
    const falseReady = spawnOwned(process.execPath, [
      '-e', 'console.log("TODO_HTTP_READY " + process.argv[1]);setTimeout(()=>{},5000);',
      fakeAddress
    ]);
    const fakeAuthority = await waitReady(falseReady, readyTodo);
    assert.equal(fakeAuthority, fakeAddress);
    await assert.rejects(fetch('http://' + fakeAuthority + '/api/v1/tasks',
      { signal: AbortSignal.timeout(1000) }));
    await stopOwned(falseReady);
  } finally {
    await new Promise(resolve => falseInterface.close(resolve));
  }
  console.log('PASS probe rejects early exit, oversized, timeout and false interface readiness');
}

async function verifyTodo() {
  await access(path.join(web, 'build/200.html'));
  await access(todo);

  const missingFrontend = path.join(scratch, 'frontend-without-deps');
  await mkdir(missingFrontend);
  await copyFile(path.join(web, 'package.json'), path.join(missingFrontend, 'package.json'));
  const missingNpmDeps = execChecked('npm', ['run', 'build'],
    { cwd: missingFrontend, timeout: 15000 });
  assert.notEqual(missingNpmDeps.status, 0, 'frontend build succeeded without installed dependencies');
  console.log('PASS missing frontend dependencies fail without auto-installation (npm baseline)');

  const db = path.join(scratch, 'retained-tasks.db');
  await expectEarlyFailure(['--db'], 'missing database argument');
  await expectEarlyFailure(['--db', path.join(scratch, 'no-parent', 'db.sqlite')],
    'unavailable persistent directory');
  await expectEarlyFailure(['--db', path.join(scratch, 'bad-assets.db'), '--assets',
    path.join(scratch, 'missing-assets')], 'missing static assets');

  const occupied = createServer();
  await new Promise((resolve, reject) =>
    occupied.once('error', reject).listen(0, '127.0.0.1', resolve));
  try {
    await expectEarlyFailure(['--db', path.join(scratch, 'occupied-port.db'), '--port',
      String(occupied.address().port)], 'occupied explicit port');
  } finally {
    await new Promise(resolve => occupied.close(resolve));
  }

  const initial = await todoStart(db);
  const { chromium } = createRequire(path.join(web, 'package.json'))('@playwright/test');
  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    await page.goto(initial.url, { waitUntil: 'domcontentloaded' });
    await page.waitForSelector('body');
    assert((await page.textContent('body')).includes('Todo'));
    await page.close();
  } finally {
    await browser.close();
  }
  assert.equal((await fetch(initial.url + '/api/v1/tasks')).status, 200);
  console.log('PASS real Chromium opened and closed without stopping Rust backend');

  const created = await fetch(initial.url + '/api/v1/tasks', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Origin': initial.url,
      'Sec-Fetch-Site': 'same-origin'
    },
    body: JSON.stringify({ title: 'Retained development record' })
  });
  assert.equal(created.status, 201, 'Todo POST failed with ' + created.status);
  const record = await created.json();
  assert.equal(record.title, 'Retained development record');
  const id = record.id;

  let duplicate = 'unknown';
  const second = spawnOwned(todo, ['--db', db, '--port', '0', '--assets',
    path.join(web, 'build')]);
  try {
    const authority = await waitReady(second, readyTodo, 4000);
    duplicate = 'second process became ready at ' + authority;
    await stopOwned(second);
  } catch (error) {
    duplicate = 'second start rejected or unresolved: ' + error.message;
    if (second.exitCode === null && second.signalCode === null) {
      await stopOwned(second);
    }
  }
  console.log('OBSERVED simultaneous same-DB startup: ' + duplicate +
    ' (no accepted duplicate/adoption policy)');

  const stopped = await stopOwned(initial.child);
  assert.equal(stopped.code, 0, 'Todo Ctrl-C was not graceful on Linux');
  const restarted = await todoStart(db);
  try {
    const read = await fetch(restarted.url + '/api/v1/tasks/' + id);
    assert.equal(read.status, 200);
    assert.equal((await read.json()).title, 'Retained development record');
    console.log('PASS Todo real static assets, dynamic endpoint, graceful stop/restart, retained SQLite');
  } finally {
    assert.equal((await stopOwned(restarted.child)).code, 0);
  }
}

try {
  await verifyHeadless();
  await verifyReadinessNegatives();
  await verifyTodo();
  console.log('RESEARCH_68_PASS (test harness, not a shipped developer CLI)');
} finally {
  for (const child of [...owned]) {
    if (child.exitCode === null && child.signalCode === null) {
      child.kill('SIGKILL');
      await new Promise(resolve => child.once('exit', resolve));
    }
  }
  await rm(scratch, { recursive: true, force: true });
}
