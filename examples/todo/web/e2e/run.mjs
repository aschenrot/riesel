//! Run browser E2E against a real local Rust process and an isolated SQLite DB.
import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import {
  verifyTrustedLocalBeforeBrowser,
  verifyHeadersDoNotAuthenticateClient,
  verifyWithdrawnListener,
} from './trusted-local-probe.mjs';

const webDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const root = path.resolve(webDir, '../../..');
const executable = path.join(root, 'target', 'debug', process.platform === 'win32' ? 'todo_http.exe' : 'todo_http');

function awaitReady(child, errors) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('Todo backend did not announce readiness')), 20000);
    const output = createInterface({ input: child.stdout });
    const cleanup = () => {
      clearTimeout(timeout);
      child.off('error', failed);
      child.off('exit', exited);
    };
    const failed = (error) => { cleanup(); reject(error); };
    const exited = (code) => { cleanup(); reject(new Error('Todo backend exited before readiness (' + code + '): ' + errors.join(''))); };
    child.once('error', failed);
    child.once('exit', exited);
    output.on('line', (line) => {
      const match = /^TODO_HTTP_READY (127\.0\.0\.1:[0-9]+)$/.exec(line);
      if (match) {
        cleanup();
        resolve('http://' + match[1]);
      }
    });
  });
}

async function stop(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((resolve) => child.once('exit', resolve));
  child.kill('SIGINT');
  await Promise.race([exited, new Promise((resolve) => setTimeout(resolve, 5000))]);
  if (child.exitCode === null && child.signalCode === null) {
    child.kill('SIGKILL');
    await exited;
  }
}

const working = await mkdtemp(path.join(tmpdir(), 'riesel-todo-web-e2e-'));
let backend;
let baseURL;
let result = 1;
let errors = [];

try {
  backend = spawn(executable, [
    '--db', path.join(working, 'tasks.db'),
    '--port', '0',
    '--assets', path.join(webDir, 'build')
  ], { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] });
  backend.stderr.on('data', (chunk) => {
    errors.push(chunk.toString());
    if (errors.length > 40) errors.shift();
  });
  baseURL = await awaitReady(backend, errors);
  // The app-specific readiness line proves only a historical socket bind.
  // A separately running client must observe actual bounded route behavior.
  await verifyTrustedLocalBeforeBrowser(baseURL);
  console.log('Browser E2E using ' + baseURL);

  const command = process.platform === 'win32' ? 'npx.cmd' : 'npx';
  const tests = spawn(command, ['playwright', 'test', '--config', 'playwright.config.ts'], {
    cwd: webDir,
    stdio: 'inherit',
    env: { ...process.env, TODO_E2E_BASE_URL: baseURL }
  });
  result = await new Promise((resolve, reject) => {
    tests.once('error', reject);
    tests.once('close', (code) => resolve(code ?? 1));
  });
  if (result === 0) {
    // Crucial negative identity control AFTER browser assertions, so this
    // independent HTTP mutation cannot affect the UI test's isolated data.
    await verifyHeadersDoNotAuthenticateClient(baseURL);
  }
} catch (error) {
  result = 1;
  console.error('Todo browser E2E could not run:', error);
  if (errors.length) console.error(errors.join(''));
} finally {
  await stop(backend);
  if (result === 0 && baseURL) {
    try {
      await verifyWithdrawnListener(baseURL);
    } catch (error) {
      console.error('Todo client-side dead-backend probe failed:', error);
      result = 1;
    }
  }
  await rm(working, { recursive: true, force: true });
}
process.exitCode = result;
