import assert from 'node:assert/strict';
import { test } from 'node:test';
import { collectUntilMatch, ScanCancelled, MAX_STATUS_REQUESTS_PER_BURST } from './status-scan.ts';

const task = { id: 'open-at-end', revision: '18446744073709551615', title: 'Last open', completed: false };

test('automatically discovers the 10,000th sparse Open within a finite burst', async () => {
  const observed = [];
  let calls = 0;
  const answer = await collectUntilMatch(false, null, async (completed, cursor) => {
    observed.push({ completed, cursor });
    calls += 1;
    return {
      records: calls === 10 ? [task] : [],
      next_cursor: calls === 10 ? null : String(calls),
      exhausted: calls === 10,
      inspected: calls === 10 ? 784 : 1024
    };
  });
  assert.equal(calls, 10);
  assert.equal(observed[1].cursor, '1');
  assert.equal(answer.inspected, 10_000);
  assert.deepEqual(answer.records, [task]);
  assert.equal(answer.exhausted, true);
});

test('stops after ten empty partial requests without falsely reporting an empty list', async () => {
  let calls = 0;
  const result = await collectUntilMatch(false, null, async () => ({
    records: [],
    next_cursor: String(++calls),
    exhausted: false,
    inspected: 1024
  }));
  assert.equal(calls, MAX_STATUS_REQUESTS_PER_BURST);
  assert.equal(result.exhausted, false);
  assert.equal(result.inspected, 10_240);
  assert.equal(result.nextCursor, '10');
});

test('reports true exhausted zero-match scan and does not run another request', async () => {
  let calls = 0;
  const result = await collectUntilMatch(true, null, async () => {
    calls += 1;
    return { records: [], next_cursor: null, exhausted: true, inspected: 0 };
  });
  assert.equal(calls, 1);
  assert.equal(result.exhausted, true);
  assert.deepEqual(result.records, []);
});

test('rejects malformed partial pages and incorrect status records rather than inventing success', async () => {
  const bad = [
    { records: [], next_cursor: null, exhausted: false, inspected: 256 },
    { records: [], next_cursor: 'x', exhausted: false, inspected: 0 },
    { records: [], next_cursor: 'x', exhausted: false, inspected: 1025 },
    { records: [task], next_cursor: null, exhausted: true, inspected: 1 }
  ];
  for (const response of bad) {
    await assert.rejects(
      collectUntilMatch(true, null, async () => response),
      /inconsistent status Query/
    );
  }
});

test('stops a stale generation and never replays a possibly consumed cursor', async () => {
  let active = true;
  let calls = 0;
  await assert.rejects(
    collectUntilMatch(false, 'one-use', async () => {
      calls += 1;
      active = false;
      return { records: [], next_cursor: 'new-token', exhausted: false, inspected: 256 };
    }, () => {}, () => active),
    ScanCancelled
  );
  assert.equal(calls, 1);
});
