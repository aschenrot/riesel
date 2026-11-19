import assert from 'node:assert/strict';
import { test } from 'node:test';
import { ApiError, listPage, createTask, updateTask, removeTask } from './api.ts';

const record = { id: 'task-1', title: 'Write tests', completed: false, revision: '18446744073709551615' };

test('list preserves bounded cursor contract', async () => {
  let observed;
  const mock = async (url, init) => {
    observed = { url, init };
    return Response.json({ records: [record], next_cursor: null });
  };
  const result = await listPage('next-id', mock);
  assert.deepEqual(result.records, [record]);
  assert.equal(observed.url, '/api/v1/tasks?limit=32&after=next-id');
  assert.equal(observed.init.method, 'GET');
  assert.equal(observed.init.credentials, 'omit');
  assert.equal(observed.init.mode, 'same-origin');
});

test('create sends exactly one domain field', async () => {
  let sent;
  const mock = async (url, init) => {
    sent = { url, init };
    return Response.json(record, { status: 201 });
  };
  await createTask('Write tests', mock);
  assert.equal(sent.url, '/api/v1/tasks');
  assert.deepEqual(JSON.parse(sent.init.body), { title: 'Write tests' });
  assert.equal(sent.init.headers['Content-Type'], 'application/json');
});

test('edits and deletion preserve the caller-observed revision', async () => {
  const requests = [];
  const mock = async (url, init) => {
    requests.push({ url, method: init.method, data: JSON.parse(init.body) });
    return init.method === 'DELETE' ? new Response(null, { status: 204 }) : Response.json(record);
  };
  await updateTask(record, { completed: true }, mock);
  await updateTask(record, { title: 'Revised' }, mock);
  await removeTask(record, mock);
  assert.deepEqual(requests.map((x) => x.data), [
    { revision: '18446744073709551615', completed: true },
    { revision: '18446744073709551615', title: 'Revised' },
    { revision: '18446744073709551615' }
  ]);
  assert.deepEqual(requests.map((x) => x.method), ['PATCH', 'PATCH', 'DELETE']);
});

test('conflict is not retried or silently refreshed', async () => {
  let calls = 0;
  const mock = async () => {
    calls++;
    return Response.json(
      { error: { code: 'revision_conflict', message: 'Task revision is stale' } },
      { status: 409 }
    );
  };
  await assert.rejects(
    updateTask(record, { completed: true }, mock),
    (error) => error instanceof ApiError &&
      error.code === 'revision_conflict' &&
      error.status === 409
  );
  assert.equal(calls, 1);
});

test('network failures remain distinct from domain rejection', async () => {
  const mock = async () => { throw new TypeError('network failed'); };
  await assert.rejects(
    listPage(null, mock),
    (error) => error instanceof ApiError && error.code === 'disconnected'
  );
});

test('response body is optional only for successful no-content responses', async () => {
  const mock = async () => new Response(null, { status: 204 });
  assert.equal(await removeTask(record, mock), undefined);
});
