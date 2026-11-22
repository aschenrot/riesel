//! #158 research: independent Node client against the actual Rust Todo host.
//! Real loopback route and browser-origin observations are NOT client identity.
import assert from 'node:assert/strict';
import { request } from 'node:http';
import { setTimeout as delay } from 'node:timers/promises';

const MAX_RESPONSE_BYTES = 64 * 1024;
const REQUEST_DEADLINE_MS = 2000;
const READINESS_DEADLINE_MS = 6000;

function boundedRequest(baseURL, path, { method = 'GET', headers = {}, body } = {}) {
  const url = new URL(path, baseURL);
  assert.equal(url.hostname, '127.0.0.1', 'never probe non-numeric or remote hosts');
  const bytes = body === undefined ? undefined : Buffer.from(body, 'utf8');
  if (bytes && bytes.length > 16 * 1024) throw new Error('probe body exceeds 16 KiB');
  return new Promise((resolve, reject) => {
    const headerValues = { connection: 'close', ...headers };
    if (bytes) headerValues['content-length'] = String(bytes.length);
    const outgoing = request(url, { method, headers: headerValues, agent: false }, (response) => {
      let total = 0;
      const chunks = [];
      response.on('data', (chunk) => {
        total += chunk.length;
        if (total > MAX_RESPONSE_BYTES) {
          outgoing.destroy(new Error('probe response exceeds 64 KiB'));
          return;
        }
        chunks.push(chunk);
      });
      response.on('error', reject);
      response.on('end', () => resolve({
        status: response.statusCode,
        body: Buffer.concat(chunks).toString('utf8'),
      }));
    });
    const deadline = setTimeout(() => {
      outgoing.destroy(new Error('bounded Todo probe request timed out'));
    }, REQUEST_DEADLINE_MS);
    outgoing.once('close', () => clearTimeout(deadline));
    outgoing.on('error', reject);
    outgoing.end(bytes);
  });
}

function json(response) {
  try {
    return JSON.parse(response.body);
  } catch {
    throw new Error('expected JSON from Todo HTTP status ' + response.status);
  }
}

// TODO_HTTP_READY is a bind announcement, not proof of actual route handling.
// Poll only a read request, with a finite window and no mutation retries.
export async function verifyTrustedLocalBeforeBrowser(baseURL) {
  let response;
  let lastError;
  const deadline = Date.now() + READINESS_DEADLINE_MS;
  do {
    try {
      response = await boundedRequest(baseURL, '/api/v1/tasks');
      break;
    } catch (error) {
      lastError = error;
      if (Date.now() + 50 >= deadline) break;
      await delay(50);
    }
  } while (Date.now() < deadline);
  if (!response) {
    throw new Error('bound Todo socket never served a GET within six seconds: ' + lastError);
  }
  assert.equal(response.status, 200, 'listener bound is not route ready');
  assert.deepEqual(json(response).records, [], 'isolated SQLite starts empty');

  // Wrong Host and browser-origin evidence reject before domain dispatch.
  const wrongHost = await boundedRequest(baseURL, '/api/v1/tasks', {
    headers: { host: 'untrusted.example.invalid' },
  });
  assert.equal(wrongHost.status, 403);
  assert.equal(json(wrongHost).error.code, 'forbidden');

  const crossSite = await boundedRequest(baseURL, '/api/v1/tasks', {
    headers: { 'sec-fetch-site': 'cross-site' },
  });
  assert.equal(crossSite.status, 403);
  assert.equal(json(crossSite).error.code, 'forbidden');

  const deniedMutation = await boundedRequest(baseURL, '/api/v1/tasks', {
    method: 'POST',
    headers: {
      origin: 'http://untrusted.example.invalid',
      'content-type': 'application/json',
    },
    body: JSON.stringify({ title: 'Must not persist' }),
  });
  assert.equal(deniedMutation.status, 403);
  assert.equal(json(deniedMutation).error.code, 'forbidden');

  const afterDenial = await boundedRequest(baseURL, '/api/v1/tasks');
  assert.equal(afterDenial.status, 200);
  assert.deepEqual(json(afterDenial).records, [], 'denied write had no persisted domain effect');
  console.log('RESEARCH_I158_REAL_ROUTE_AND_BROWSER_DENIAL_PASS');
}

// Negative authorization control: this is a separate Node OS process without
// a verified app-client identity or individual operation grant. It can freely
// forge the permitted Host/Origin headers under the trusted-local profile.
export async function verifyHeadersDoNotAuthenticateClient(baseURL) {
  const title = 'Independent client has no verified individual grant';
  const created = await boundedRequest(baseURL, '/api/v1/tasks', {
    method: 'POST',
    headers: { origin: baseURL, 'content-type': 'application/json' },
    body: JSON.stringify({ title }),
  });
  assert.equal(created.status, 201, 'trusted-local policy permits another local client');
  const task = json(created);
  assert.equal(task.title, title);
  assert.equal(task.completed, false);
  assert.equal(typeof task.id, 'string');
  console.log('RESEARCH_I158_FORGED_ALLOWED_ORIGIN_IS_NOT_APP_CLIENT_AUTH_PASS');
}

// After the owning Rust child exits, a new TCP request must not report ready.
// The failed request proves transport unavailable, NOT who killed the process.
export async function verifyWithdrawnListener(baseURL) {
  let response;
  let failure;
  try {
    response = await boundedRequest(baseURL, '/api/v1/tasks');
  } catch (error) {
    failure = error;
  }
  assert.equal(response, undefined, 'closed backend must not serve cached readiness');
  assert.ok(failure, 'dead backend must yield a transport failure');
  console.log('RESEARCH_I158_DEAD_BACKEND_TRANSPORT_UNAVAILABLE_PASS');
}
