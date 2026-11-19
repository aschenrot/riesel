import assert from 'node:assert/strict';
import { test } from 'node:test';
import { trustedDevOrigin } from './vite-proxy-security.ts';

const local = {
  host: '127.0.0.1:5173',
  origin: 'http://127.0.0.1:5173',
  'sec-fetch-site': 'same-origin'
};

test('trusted Vite origin can be forwarded to the Rust API', () => {
  assert.equal(trustedDevOrigin(local), true);
  assert.equal(trustedDevOrigin({ ...local, 'sec-fetch-site': 'none' }), true);
});

test('external, missing, or ambiguous browser origins must never be translated', () => {
  for (const candidate of [
    { ...local, origin: 'https://evil.test' },
    { ...local, origin: 'null' },
    { ...local, origin: undefined },
    { ...local, 'sec-fetch-site': 'cross-site' },
    { ...local, 'sec-fetch-site': 'same-site' },
    { ...local, 'sec-fetch-site': ['same-origin', 'cross-site'] },
    { ...local, host: 'attacker.test:5173' },
    { ...local, host: '127.0.0.1:5173,evil.test' },
    { ...local, host: undefined }
  ]) {
    assert.equal(trustedDevOrigin(candidate), false, JSON.stringify(candidate));
  }
});
