import { mkdir } from 'node:fs/promises';
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
  await expect(page.getByRole('heading', { name: 'Tasks' })).toBeVisible();
  await expect(page.getByText('No tasks yet')).toBeVisible();

  await page.getByRole('textbox', { name: 'New task' }).fill('Plan release');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByText('Plan release', { exact: true })).toBeVisible();

  await page.getByRole('button', { name: 'Actions for Plan release' }).click();
  await page.getByRole('button', { name: 'Edit task' }).click();
  await page.getByRole('textbox', { name: 'Edit task title' }).fill('Ship release');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Ship release', { exact: true })).toBeVisible();

  await page.getByRole('checkbox', { name: 'Mark Ship release complete' }).check();
  await expect(page.getByRole('status')).toContainText('Task completed');
  await page.getByRole('checkbox', { name: 'Mark Ship release incomplete' }).uncheck();

  await page.getByRole('button', { name: 'Actions for Ship release' }).click();
  await page.getByRole('button', { name: 'Remove task' }).click();
  await page.getByRole('button', { name: 'Delete' }).click();
  await expect(page.getByText('No tasks yet')).toBeVisible();
});

test('stale browser revisions require refresh; never silently overwrite', async ({ page, request }) => {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('Draft');
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
  const input = page.getByRole('textbox', { name: 'New task' });
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
  await page.getByRole('textbox', { name: 'New task' }).fill('Reconnect task');
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

  await page.getByRole('textbox', { name: 'New task' }).fill('   ');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByRole('alert')).toContainText('Enter a task title.');
  await expect(page.getByText('No tasks yet')).toBeVisible();

  await page.getByRole('textbox', { name: 'New task' }).fill('Valid after error');
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
  await expect(page.getByText('All tasks loaded')).toBeVisible();
  await expect(page.locator('li.task')).toHaveCount(33);
});

test('completed and open filters do not modify persisted state', async ({ page }) => {
  await page.goto('/');
  for (const title of ['Leave open', 'Mark done']) {
    await page.getByRole('textbox', { name: 'New task' }).fill(title);
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

test('success feedback uses a dismissible toast without moving the list', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('Toast example');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByRole('status')).toContainText('Task created');
  await expect(page.locator('.toast-region')).toHaveCSS('position', 'fixed');
  await page.getByRole('button', { name: 'Dismiss notification' }).click();
  await expect(page.locator('.toast')).toHaveCount(0);
  await expect(page.getByText('Toast example', { exact: true })).toBeVisible();
});

test('task actions are keyboard accessible; Escape returns focus to trigger', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('Keyboard actions');
  await page.getByRole('button', { name: 'Add task' }).click();
  const trigger = page.getByRole('button', { name: 'Actions for Keyboard actions' });
  await trigger.focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('button', { name: 'Edit task' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();

  await page.keyboard.press('Enter');
  await page.getByRole('button', { name: 'Remove task' }).click();
  await expect(page.getByRole('button', { name: 'Delete' })).toBeVisible();
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByText('Keyboard actions', { exact: true })).toBeVisible();
  await expect(trigger).toBeFocused();
});

test('mobile layout keeps task action on the row without horizontal overflow', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('Small-screen example');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByRole('button', { name: 'Actions for Small-screen example' })).toBeVisible();
  await expect(page.locator('li.task')).toHaveCSS('flex-wrap', 'nowrap');
  expect(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)).toBe(false);
});

test('inline title validation keeps the invalid input in place', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('   ');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.locator('#new-title-error')).toHaveText('Enter a task title.');
  await expect(page.getByText('No tasks yet')).toBeVisible();
});

test('capture desktop and mobile UI for visual review', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  const title = page.getByRole('textbox', { name: 'New task' });
  for (const text of ['Review architecture', 'Refine Todo layout', 'Verify accessibility']) {
    await title.fill(text);
    await page.getByRole('button', { name: 'Add task' }).click();
  }
  await page.getByRole('checkbox', { name: 'Mark Verify accessibility complete' }).check();
  await mkdir('test-results/visual-review', { recursive: true });
  await expect(page.getByRole('status')).toContainText('Task completed');
  await page.screenshot({
    path: 'test-results/visual-review/todo-success-toast-1280.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.getByRole('button', { name: 'Dismiss notification' }).click();
  await page.getByRole('button', { name: 'Actions for Refine Todo layout' }).click();
  await page.screenshot({
    path: 'test-results/visual-review/todo-action-menu-1280.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.keyboard.press('Escape');
  await page.screenshot({
    path: 'test-results/visual-review/todo-desktop-1280.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.setViewportSize({ width: 375, height: 812 });
  await expect(page.getByRole('heading', { name: 'Tasks' })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)).toBe(false);
  await page.screenshot({
    path: 'test-results/visual-review/todo-mobile-375.png',
    fullPage: true,
    animations: 'disabled'
  });
});

test('refresh control stays at top-left, spins during GET and records successful checks', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.refresh-details time')).toHaveAttribute('datetime', /\d{4}-\d{2}-\d{2}T/);
  await page.getByRole('textbox', { name: 'New task' }).fill('Keep visible');
  await page.getByRole('button', { name: 'Add task' }).click();
  const refresh = page.getByRole('button', { name: 'Refresh tasks' });
  const toolbar = page.locator('.toolbar');
  const icon = refresh.locator('svg');
  expect(await toolbar.evaluate((node) =>
    node.querySelector('.refresh-group') === node.firstElementChild)).toBe(true);

  await page.route('**/api/v1/tasks*', async (route) => {
    if (route.request().method() === 'GET') {
      await new Promise((resolve) => setTimeout(resolve, 900));
    }
    await route.continue();
  });
  await refresh.click();
  await expect(refresh).toBeDisabled();
  await expect(page.getByText('Checking…')).toBeVisible();
  await expect(icon).toHaveClass(/spinning/);
  await expect(page.getByText('Keep visible', { exact: true })).toBeVisible();
  await expect(refresh).toBeEnabled();
  await expect(icon).not.toHaveClass(/spinning/);
  await expect(page.locator('.refresh-details time')).toHaveAttribute('datetime', /\d{4}-\d{2}-\d{2}T/);
});

test('failed refresh does not falsely advance last checked timestamp', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('.refresh-details time')).toHaveAttribute('datetime', /\d{4}-\d{2}-\d{2}T/);
  const previous = await page.locator('.refresh-details time').getAttribute('datetime');
  await page.route('**/api/v1/tasks*', (route) => route.abort());
  await page.getByRole('button', { name: 'Refresh tasks' }).click();
  await expect(page.getByRole('alert')).toContainText('Cannot connect to the Todo backend.');
  await expect(page.locator('.refresh-details time')).toHaveAttribute('datetime', previous!);
  await page.unroute('**/api/v1/tasks*');
  await page.getByRole('button', { name: 'Try again' }).click();
  await expect(page.getByRole('alert')).toHaveCount(0);
  await expect(page.locator('.refresh-details time')).toHaveAttribute('datetime', /\d{4}-\d{2}-\d{2}T/);
});

test('reduced motion preference disables refresh spin while keeping the state clear', async ({ page }) => {
  await page.emulateMedia({ reducedMotion: 'reduce' });
  await page.goto('/');
  await page.route('**/api/v1/tasks*', async (route) => {
    if (route.request().method() === 'GET') {
      await new Promise((resolve) => setTimeout(resolve, 900));
    }
    await route.continue();
  });
  await page.getByRole('button', { name: 'Refresh tasks' }).click();
  await expect(page.locator('.refresh-icon')).toHaveClass(/spinning/);
  await expect(page.locator('.refresh-icon')).toHaveCSS('animation-name', 'none');
  await expect(page.getByText('Checking…')).toBeVisible();
});

test('narrow 320px viewport keeps refresh and filters accessible', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 740 });
  await page.goto('/');
  await expect(page.getByRole('button', { name: 'Refresh tasks' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Done', exact: true })).toBeVisible();
  expect(await page.evaluate(() =>
    document.documentElement.scrollWidth > window.innerWidth + 1)).toBe(false);
});

test('connection failure while editing preserves the draft until explicit cancellation', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('Original title');
  await page.getByRole('button', { name: 'Add task' }).click();
  await page.getByRole('button', { name: 'Actions for Original title' }).click();
  await page.getByRole('button', { name: 'Edit task' }).click();
  const title = page.getByRole('textbox', { name: 'Edit task title' });
  await title.fill('Unsaved title');
  await page.route('**/api/v1/tasks/*', (route) => {
    if (route.request().method() === 'PATCH') return route.abort();
    return route.continue();
  });
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('alert')).toContainText('Cannot connect to the Todo backend.');
  await expect(title).toHaveValue('Unsaved title');
  await expect(page.getByRole('button', { name: 'Refresh tasks' })).toBeDisabled();
  await page.getByRole('button', { name: 'Dismiss' }).click();
  await expect(title).toHaveValue('Unsaved title');
  await expect(page.getByRole('alert')).toHaveCount(0);
  await page.unroute('**/api/v1/tasks/*');
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByRole('button', { name: 'Refresh tasks' })).toBeEnabled();
});

test('revision conflict explicitly permits reloading a stale edit', async ({ page, request }) => {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('Stale title');
  await page.getByRole('button', { name: 'Add task' }).click();
  await page.getByRole('button', { name: 'Actions for Stale title' }).click();
  await page.getByRole('button', { name: 'Edit task' }).click();
  await page.getByRole('textbox', { name: 'Edit task title' }).fill('My version');

  const task = (await (await request.get(api)).json()).records[0];
  const outside = await request.patch(api + '/' + task.id, {
    headers: { Origin: origin() },
    data: { revision: task.revision, title: 'External version' }
  });
  expect(outside.status()).toBe(200);

  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByRole('alert')).toContainText('Task revision is stale');
  await expect(page.getByRole('button', { name: 'Refresh tasks' })).toBeEnabled();
  await page.getByRole('button', { name: 'Reload current tasks' }).click();
  await expect(page.getByText('External version', { exact: true })).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Edit task title' })).toHaveCount(0);
});
