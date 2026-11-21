/** Research-only #107: prove the exact browser symptom against real Todo HTTP+SQLite. */
import { mkdir } from 'node:fs/promises';
import { expect, test, type APIRequestContext } from '@playwright/test';

const api = '/api/v1/tasks';
const origin = (): string => {
  const value = process.env.TODO_E2E_BASE_URL;
  if (!value) throw new Error('Todo browser research requires the Rust host URL');
  return value;
};

async function clean(request: APIRequestContext): Promise<void> {
  for (;;) {
    const response = await request.get(api + '?limit=32');
    expect(response.status()).toBe(200);
    const batch = (await response.json()) as {
      records: { id: string; revision: string }[];
    };
    if (batch.records.length === 0) return;
    for (const record of batch.records) {
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
test.afterEach(async ({ request }) => {
  await clean(request);
});

test('Open filter is empty on first page even with an Open record on the next', async ({ page, request }) => {
  const tasks: { id: string; revision: string; title: string }[] = [];
  for (let index = 0; index < 33; index++) {
    const response = await request.post(api, {
      headers: { Origin: origin() },
      data: { title: 'Research task ' + index }
    });
    expect(response.status()).toBe(201);
    tasks.push(await response.json());
  }
  tasks.sort((a, b) => a.id.localeCompare(b.id));
  for (const task of tasks.slice(0, 32)) {
    const response = await request.patch(api + '/' + task.id, {
      headers: { Origin: origin() },
      data: { revision: task.revision, completed: true }
    });
    expect(response.status()).toBe(200);
  }
  const onlyOpen = tasks[32];

  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Tasks' })).toBeVisible();
  await expect(page.getByText(/32 loaded/)).toBeVisible();
  await page.getByRole('button', { name: 'Open', exact: true }).click();

  // The current R2 UI is intentionally honest about *loaded* state only.
  await expect(page.getByText('Nothing in this view')).toBeVisible();
  await expect(page.getByText('Switch filters to see other loaded tasks.')).toBeVisible();
  await expect(page.getByRole('button', { name: 'Load more tasks' })).toBeVisible();
  await mkdir('test-results/visual-review', { recursive: true });
  await page.screenshot({
    path: 'test-results/visual-review/todo-research-hidden-open-33.png',
    fullPage: true,
    animations: 'disabled'
  });

  await page.getByRole('button', { name: 'Load more tasks' }).click();
  await expect(page.getByText(onlyOpen.title, { exact: true })).toBeVisible();
  await expect(page.locator('li.task')).toHaveCount(1);
  await expect(page.getByText('All tasks loaded')).toBeVisible();
  // This demonstrates the semantic gap; it does not authorize UI grouping.
});
