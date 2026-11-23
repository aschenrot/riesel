// Research-only Rust-server / independent Node-client IPC probe, no Riesel SDK APIs.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { mkdtemp, rm } from 'node:fs/promises';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const executable = path.join(here, 'target', 'debug', `riesel-ipc-272${process.platform === 'win32' ? '.exe' : ''}`);
const MAX_RESPONSE = 1024 * 1024 + 4096;

async function start(kind, name, generation) {
  const child = spawn(executable, [kind, name, generation], { stdio: ['ignore', 'pipe', 'pipe'] });
  let stderr = '';
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', text => { stderr += text; });
  const endpoint = await new Promise((resolve, reject) => {
    let output = '';
    const timeout = setTimeout(() => reject(new Error(`server ready timeout: ${stderr}`)), 10000);
    const failed = (error) => { clearTimeout(timeout); reject(error); };
    child.on('error', failed);
    child.once('exit', code => failed(new Error(`server exited ${code}: ${stderr}`)));
    child.stdout.setEncoding('utf8');
    child.stdout.on('data', chunk => {
      output += chunk;
      const match = output.match(/^READY (.+)\r?$/m);
      if (match) { clearTimeout(timeout); resolve(match[1]); }
    });
  });
  return { child, endpoint };
}

function query(endpoint, payload, { fragmented = false, declaredLength = null } = {}) {
  return new Promise((resolve, reject) => {
    let settled = false;
    let received = Buffer.alloc(0);
    const socket = net.createConnection({ path: endpoint });
    const timeout = setTimeout(() => finish(new Error('request timeout')), 8000);
    function finish(error, value) {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      socket.destroy();
      if (error) reject(error); else resolve(value);
    }
    socket.on('error', finish);
    socket.on('connect', () => {
      const body = Buffer.from(JSON.stringify(payload), 'utf8');
      const header = Buffer.alloc(4);
      header.writeUInt32BE(declaredLength ?? body.length);
      const wire = Buffer.concat([header, body]);
      if (fragmented) {
        socket.write(wire.subarray(0, 1));
        setTimeout(() => socket.write(wire.subarray(1, 3)), 10);
        setTimeout(() => socket.write(wire.subarray(3)), 20);
      } else socket.write(wire);
      // Deliberately never call socket.end(). Framing must not rely on half-close.
    });
    socket.on('data', chunk => {
      received = Buffer.concat([received, chunk]);
      if (received.length < 4) return;
      const length = received.readUInt32BE(0);
      if (length > MAX_RESPONSE) return finish(new Error('reply exceeds budget'));
      if (received.length >= length + 4) {
        try { finish(null, JSON.parse(received.subarray(4, length + 4).toString('utf8'))); }
        catch (error) { finish(error); }
      }
    });
    socket.on('end', () => finish(new Error('disconnected before complete frame')));
  });
}

async function stop(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error('server did not exit')), 6000);
    child.once('exit', () => { clearTimeout(timeout); resolve(); });
    child.kill();
  });
}

async function probe(kind) {
  const dir = await mkdtemp(path.join(os.tmpdir(), 'riesel-272-'));
  const id = `riesel-272-${randomUUID()}`;
  const name = kind === 'tipsy' ? id : process.platform === 'win32'
    ? `\\\\.\\pipe\\${id}` : path.join(dir, 'read.sock');
  let first, second;
  try {
    first = await start(kind, name, 'run-A');
    const endpoint = first.endpoint;
    const descriptor = await query(endpoint, { wire_version: 1, action: 'describe' }, { fragmented: true });
    assert.equal(descriptor.status, 'descriptor');
    assert.equal(descriptor.generation, 'run-A');
    const clients = await Promise.all(Array.from({ length: 8 }, () =>
      query(endpoint, { wire_version: 1, action: 'query', generation: 'run-A' })));
    assert.ok(clients.every(r => r.status === 'ok' && r.value.generation === 'run-A'));
    assert.equal((await query(endpoint, {wire_version: 1, action: 'mutate'})).code, 'unpublished_operation');
    assert.equal((await query(endpoint, {wire_version: 7, action: 'describe'})).code, 'version_mismatch');
    // An oversized declared frame is rejected before allocation/read; a disconnected client is expected.
    await assert.rejects(query(endpoint, {wire_version: 1, action: 'describe'}, {declaredLength: 128 * 1024 + 1}));
    await stop(first.child);
    first = null;
    if (process.platform !== 'win32') {
      await rm(endpoint, {force: true}); // Test-only endpoint teardown AFTER owner death.
    }
    second = await start(kind, name, 'run-B');
    const stale = await query(second.endpoint, { wire_version: 1, action: 'query', generation: 'run-A' });
    assert.equal(stale.code, 'stale_generation');
    const fresh = await query(second.endpoint, { wire_version: 1, action: 'query', generation: 'run-B' });
    assert.equal(fresh.status, 'ok');
    console.log(`${kind}: cross-process Node interoperability, fragmentation, concurrency, denial, oversize, restart passed`);
  } finally {
    if (first) await stop(first.child);
    if (second) await stop(second.child);
    await rm(dir, {force: true, recursive: true});
  }
}

for (const kind of ['interprocess', 'tipsy']) await probe(kind);
