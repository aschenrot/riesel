import { test, expect, type APIRequestContext } from '@playwright/test';

const api = '/api/v1/tasks';
const origin = () => {
  const url = process.env.TODO_E2E_BASE_URL;
  if (!url) throw new Error('E2E requires the running Rust server URL');
  return url;
};

async function clean(request: APIRequestContext) {
  // Avoid state leakage between browser tests using the same real SQLite file.
  for (;;) {
    const result = await request.get(api + '?limit=32');
    expect(result.status()).toBe(200);
    const page = await result.json();
    if (page.records.length === 0) return;
    for (const record of page.records) {
      const deleted = await request.delete(api + '/' + record.id, {
        headers: { Origin: origin() },
        data: { revision: record.revision }
      });
      expect(deleted.status()).toBe(204);
    }
  }
}

test.beforeEach(async ({ request }) => {
  await clean(request);
});

test('browser creates, edits, completes, reopens and deletes a task', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Stay on top of what matters.' })).toBeVisible();
  await expect(page.getByText('No tasks yet')).toBeVisible();

  await page.getByRole('textbox', { name: 'Add a task' }).fill('Plan release');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByText('Plan release', { exact: true })).toBeVisible();

  await page.getByRole('button', { name: 'Edit Plan release' }).click();
  await page.getByRole('textbox', { name: 'Edit task title' }).fill('Ship release');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Ship release', { exact: true })).toBeVisible();

  await page.getByRole('checkbox', { name: 'Mark Ship release complete' }).check();
  await expect(page.getByRole('status')).toContainText('Task completed.');
  await page.getByRole('checkbox', { name: 'Mark Ship release incomplete' }).uncheck();

  await page.getByRole('button', { name: 'Remove Ship release' }).click();
  await page.getByRole('button', { name: 'Confirm' }).click();
  await expect(page.getByText('No tasks yet')).toBeVisible();
});

test('stale browser revisions require refresh; never silently overwrite', async ({ page, request }) => {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'Add a task' }).fill('Draft');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByText('Draft', { exact: true })).toBeVisible();

  const data = await (await request.get(api)).json();
  expect(data.records).toHaveLength(1);
  const task = data.records[0];
  const outside = await request.patch(api + '/' + task.id, {
    headers: { Origin: origin() },
    data: { revision: task.revision, title: 'Changed externally' }
  });
  expect(outside.status()).toBe(200);

  await page.getByRole('checkbox', { name: 'Mark Draft complete' }).check();
  await expect(page.getByRole('alert')).toContainText('Task revision is stale');
  await page.getByRole('button', { name: 'Reload current tasks' }).click();
  await expect(page.getByText('Changed externally', { exact: true })).toBeVisible();

  const current = (await (await request.get(api)).json()).records[0];
  expect(current.completed).toBe(false);
});

test('mobile keyboard, reload and reconnect retain task state', async ({ page, context }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto('/');
  const input = page.getByRole('textbox', { name: 'Add a task' });
  await input.focus();
  await page.keyboard.type('Keyboard task');
  await page.keyboard.press('Enter');
  await expect(page.getByText('Keyboard task', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)).toBe(false);

  await page.close();
  const reopened = await context.newPage();
  await reopened.goto('/');
  await expect(reopened.getByText('Keyboard task', { exact: true })).toBeVisible();
});

test('connection errors are visible and allow retry without losing tasks', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'Add a task' }).fill('Reconnect task');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByText('Reconnect task', { exact: true })).toBeVisible();

  await page.route('**/api/v1/tasks*', (route) => route.abort());
  await page.getByRole('button', { name: 'Refresh tasks' }).click();
  await expect(page.getByRole('alert')).toContainText('Cannot connect to the Todo backend.');
  await page.unroute('**/api/v1/tasks*');
  await page.getByRole('button', { name: 'Try again' }).click();
  await expect(page.getByText('Reconnect task', { exact: true })).toBeVisible();
});


test('loading and domain validation are visible before accepting corrected input', async ({ page }) => {
  await page.route('**/api/v1/tasks*', async (route) => {
    if (route.request().method() === 'GET') {
      await new Promise((resolve) => setTimeout(resolve, 900));
    }
    await route.continue();
  });
  await page.goto('/');
  await expect(page.getByText('Loading tasks…')).toBeVisible();
  await expect(page.getByText('No tasks yet')).toBeVisible();
  await page.unroute('**/api/v1/tasks*');

  await page.getByRole('textbox', { name: 'Add a task' }).fill('   ');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByRole('alert')).toContainText('Invalid request');
  await expect(page.getByText('No tasks yet')).toBeVisible();

  await page.getByRole('textbox', { name: 'Add a task' }).fill('Valid after error');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByText('Valid after error', { exact: true })).toBeVisible();
});

test('cursor pagination loads the complete collection without duplicating rows', async ({ page, request }) => {
  for (let index = 0; index < 33; index++) {
    const result = await request.post(api, {
      headers: { Origin: origin() },
      data: { title: 'Paged ' + index }
    });
    expect(result.status()).toBe(201);
  }
  await page.goto('/');
  await expect(page.getByText('32 loaded', { exact: false })).toBeVisible();
  await page.getByRole('button', { name: 'Load more tasks' }).click();
  await expect(page.getByText('33 loaded', { exact: false })).toBeVisible();
  await expect(page.getByText('All available pages loaded')).toBeVisible();
  await expect(page.locator('li.task')).toHaveCount(33);
});

test('completed and open filters do not modify persisted state', async ({ page }) => {
  await page.goto('/');
  for (const title of ['Leave open', 'Mark done']) {
    await page.getByRole('textbox', { name: 'Add a task' }).fill(title);
    await page.getByRole('button', { name: 'Add task' }).click();
    await expect(page.getByText(title, { exact: true })).toBeVisible();
  }
  await page.getByRole('checkbox', { name: 'Mark Mark done complete' }).check();
  await page.getByRole('button', { name: 'Done', exact: true }).click();
  await expect(page.getByText('Mark done', { exact: true })).toBeVisible();
  await expect(page.getByText('Leave open', { exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'Open', exact: true }).click();
  await expect(page.getByText('Leave open', { exact: true })).toBeVisible();
  await expect(page.getByText('Mark done', { exact: true })).toHaveCount(0);
  await page.getByRole('button', { name: 'All', exact: true }).click();
  await expect(page.locator('li.task')).toHaveCount(2);
});
