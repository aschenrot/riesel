import { test, expect, type APIRequestContext } from '@playwright/test';

const api = '/api/v1/tasks';
const origin = () => {
  if (!process.env.TODO_E2E_BASE_URL) throw new Error('Missing real Rust Todo host URL');
  return process.env.TODO_E2E_BASE_URL;
};

async function clean(request: APIRequestContext) {
  // The same isolated SQLite host runs the full Chromium suite.
  for (;;) {
    const result = await request.get(api + '?limit=32');
    expect(result.ok()).toBe(true);
    const rows = (await result.json()).records;
    if (rows.length === 0) break;
    for (const record of rows) {
      const removed = await request.delete(api + '/' + record.id, {
        headers: { Origin: origin() },
        data: { revision: record.revision }
      });
      expect(removed.status()).toBe(204);
    }
  }
}

async function seedSparse(request: APIRequestContext, count: number, openRanks: number[]) {
  const rows = [];
  for (let index = 0; index < count; index++) {
    const result = await request.post(api, {
      headers: { Origin: origin() },
      data: { title: 'Sparse browser record ' + index }
    });
    expect(result.status()).toBe(201);
    rows.push(await result.json());
  }
  rows.sort((a, b) => a.id.localeCompare(b.id));
  for (let position = 0; position < rows.length; position++) {
    if (openRanks.includes(position)) continue;
    const original = rows[position];
    const response = await request.patch(api + '/' + original.id, {
      headers: { Origin: origin() },
      data: { revision: original.revision, completed: true }
    });
    expect(response.status()).toBe(200);
  }
  return openRanks.map((rank) => rows[rank]);
}

test.beforeEach(async ({ request }) => {
  await clean(request);
});

test('33 records: last UUID Open is visible despite the legacy first page showing 32 Done', async ({ request, page }) => {
  const [lastOpen] = await seedSparse(request, 33, [32]);
  const legacy = await request.get(api + '?limit=32');
  expect(legacy.status()).toBe(200);
  expect((await legacy.json()).records.every((row: { completed: boolean }) => row.completed)).toBe(true);

  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Open tasks' })).toBeVisible();
  await expect(page.getByText(lastOpen.title, { exact: true })).toBeVisible();
  await expect(page.locator('.task-section').first().getByText('1 shown')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Completed', exact: true }))
    .toHaveAttribute('aria-expanded', 'false');
  await expect(page.getByText('No open tasks found')).toHaveCount(0);
});

test('100 records: Open at UUID ranks 70/98/99 remains independently discoverable', async ({ request, page }) => {
  const expected = await seedSparse(request, 100, [70, 98, 99]);
  await page.goto('/');
  for (const record of expected) {
    await expect(page.getByText(record.title, { exact: true })).toBeVisible();
  }
  await expect(page.locator('.task-section').first().getByText('3 shown')).toBeVisible();
  const completed = page.getByRole('button', { name: 'Completed', exact: true });
  await expect(completed).toHaveAttribute('aria-expanded', 'false');
  await completed.click();
  await expect(completed).toHaveAttribute('aria-expanded', 'true');
  await expect(page.locator('.completed-section li.task')).toHaveCount(32);
  await expect(page.locator('.task-section').first().getByText('3 shown')).toBeVisible();
  // Only currently displayed/loaded rows participate in explicit selection.
  await page.getByRole('button', { name: 'Select tasks' }).click();
  await page.getByRole('button', { name: 'Select all loaded' }).click();
  await expect(page.getByText('35 selected')).toBeVisible();
});
