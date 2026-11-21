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

  await page.getByRole('button', { name: 'Edit Plan release', exact: true }).click();
  await page.getByRole('textbox', { name: 'Edit task title' }).fill('Ship release');
  await page.getByRole('button', { name: 'Save' }).click();
  await expect(page.getByText('Ship release', { exact: true })).toBeVisible();

  await page.getByRole('checkbox', { name: 'Mark Ship release complete' }).check();
  await expect(page.getByRole('status')).toContainText('Task completed');
  await page.getByRole('checkbox', { name: 'Mark Ship release incomplete' }).uncheck();

  await page.getByRole('button', { name: 'Delete Ship release' }).click();
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

test('direct task edit/delete controls are keyboard accessible and Escape restores focus', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('Keyboard actions');
  await page.getByRole('button', { name: 'Add task' }).click();
  const trigger = page.getByRole('button', { name: 'Edit Keyboard actions', exact: true });
  await trigger.focus();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('textbox', { name: 'Edit task title' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();

  const deleteAction = page.getByRole('button', { name: 'Delete Keyboard actions' });
  await deleteAction.click();
  await expect(page.getByRole('button', { name: 'Delete' })).toBeVisible();
  await page.getByRole('button', { name: 'Cancel' }).click();
  await expect(page.getByText('Keyboard actions', { exact: true })).toBeVisible();
  await expect(deleteAction).toBeFocused();
});

test('mobile layout keeps task action on the row without horizontal overflow', async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 812 });
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('Small-screen example');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByRole('button', { name: 'Delete Small-screen example' })).toBeVisible();
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
    // Click completion is not proof that the asynchronous create was accepted.
    await expect(page.getByText(text, { exact: true })).toBeVisible();
  }
  await expect(page.locator('li.task')).toHaveCount(3);
  await page.getByRole('checkbox', { name: 'Mark Verify accessibility complete' }).check();
  await mkdir('test-results/visual-review', { recursive: true });
  await expect(page.getByRole('status')).toContainText('Task completed');
  await page.screenshot({
    path: 'test-results/visual-review/todo-success-toast-1280.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.getByRole('button', { name: 'Dismiss notification' }).click();
  await page.getByRole('button', { name: 'Delete Refine Todo layout' }).click();
  await page.screenshot({
    path: 'test-results/visual-review/todo-delete-confirm-1280.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.keyboard.press('Escape');
  await page.screenshot({
    path: 'test-results/visual-review/todo-desktop-1280.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.getByRole('button', { name: 'Select tasks' }).click();
  await page.getByRole('button', { name: 'Select all loaded' }).click();
  await page.screenshot({
    path: 'test-results/visual-review/todo-selection-desktop-1280.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.getByRole('button', { name: 'Cancel selection' }).click();
  await page.setViewportSize({ width: 375, height: 812 });
  await expect(page.getByRole('heading', { name: 'Tasks' })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > window.innerWidth + 1)).toBe(false);
  await page.screenshot({
    path: 'test-results/visual-review/todo-mobile-375.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.getByRole('button', { name: 'Select tasks' }).click();
  await page.getByRole('checkbox', { name: 'Select Review architecture' }).check();
  await page.screenshot({
    path: 'test-results/visual-review/todo-selection-mobile-375.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.getByRole('button', { name: 'Cancel selection' }).click();
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.screenshot({
    path: 'test-results/visual-review/todo-dark-desktop-1280.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.setViewportSize({ width: 375, height: 812 });
  await page.screenshot({
    path: 'test-results/visual-review/todo-dark-mobile-375.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.setViewportSize({ width: 1280, height: 800 });
  await title.fill('Dark visual feedback');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByText('Dark visual feedback', { exact: true })).toBeVisible();
  await expect(page.getByRole('status')).toContainText('Task created');
  await page.screenshot({
    path: 'test-results/visual-review/todo-dark-toast-1280.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.getByRole('button', { name: 'Dismiss notification' }).click();
  await page.getByRole('button', { name: 'Delete Dark visual feedback' }).click();
  await expect(page.getByRole('button', { name: 'Delete', exact: true })).toBeVisible();
  await page.screenshot({
    path: 'test-results/visual-review/todo-dark-delete-confirm-1280.png',
    fullPage: true,
    animations: 'disabled'
  });
  await page.keyboard.press('Escape');
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
  await page.getByRole('button', { name: 'Edit Original title', exact: true }).click();
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
  await page.getByRole('button', { name: 'Edit Stale title', exact: true }).click();
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

test('draft entered during in-flight creation is preserved after the first task is saved', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('No tasks yet')).toBeVisible();
  const input = page.getByRole('textbox', { name: 'New task' });
  // Gate the first POST explicitly instead of racing against a fixed timeout.
  let firstRequestStarted!: () => void;
  const firstStarted = new Promise<void>((resolve) => { firstRequestStarted = resolve; });
  let releaseFirstRequest!: () => void;
  const firstReleased = new Promise<void>((resolve) => { releaseFirstRequest = resolve; });
  let firstPost = true;
  await page.route('**/api/v1/tasks', async (route) => {
    if (route.request().method() === 'POST' && firstPost) {
      firstPost = false;
      firstRequestStarted();
      await firstReleased;
    }
    await route.continue();
  });

  try {
    await input.fill('First task');
    await page.getByRole('button', { name: 'Add task' }).click();
    await firstStarted;
    await expect(page.getByRole('button', { name: 'Adding…' })).toBeDisabled();
    await input.fill('Second task');
    await expect(input).toHaveValue('Second task');

    releaseFirstRequest();
    await expect(page.getByText('First task', { exact: true })).toBeVisible();
    await expect(input).toHaveValue('Second task');
    await page.getByRole('button', { name: 'Add task' }).click();
    await expect(page.getByText('Second task', { exact: true })).toBeVisible();
    await expect(page.locator('li.task')).toHaveCount(2);
    await expect(input).toHaveValue('');
  } finally {
    releaseFirstRequest();
  }
});

test('system light and dark themes track live browser preference with readable states', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'light' });
  await page.goto('/');
  const panel = page.locator('.panel');
  // Static SvelteKit bootstraps client-side; wait for the rendered app first.
  await expect(panel).toBeVisible();
  await expect(page.getByRole('button', { name: 'Add task' })).toBeVisible();
  const colors = async () => await page.evaluate(() => {
    const style = (selector: string) => getComputedStyle(document.querySelector(selector)!);
    return {
      surface: style('.panel').backgroundColor,
      canvas: style('.app-shell').backgroundColor,
      primary: style('.new-task button.primary').backgroundColor,
      primaryInk: style('.new-task button.primary').color,
      ink: style('h1').color,
      errorSurface: getComputedStyle(document.documentElement).getPropertyValue('--error-surface').trim(),
      userAgent: style('html').colorScheme
    };
  });
  const light = await colors();
  expect(light.surface).toBe('rgb(255, 255, 255)');
  expect(light.canvas).toBe('rgb(245, 247, 245)');
  expect(light.userAgent).toContain('dark');

  await page.emulateMedia({ colorScheme: 'dark' });
  await expect(panel).toHaveCSS('background-color', 'rgb(24, 39, 34)');
  const dark = await colors();
  expect(dark.canvas).toBe('rgb(16, 27, 23)');
  expect(dark.primary).not.toEqual(light.primary);
  expect(dark.ink).not.toEqual(light.ink);
  expect(dark.errorSurface).not.toEqual(light.errorSurface);

  const contrast = (text: string, background: string): number => {
    const channels = (rgb: string) => rgb.match(/[\d.]+/g)!.slice(0, 3).map(Number).map((x) => {
      const c = x / 255;
      return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
    });
    const luminance = (rgb: string) => {
      const [r, g, b] = channels(rgb);
      return r * 0.2126 + g * 0.7152 + b * 0.0722;
    };
    const a = luminance(text);
    const b = luminance(background);
    return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
  };
  expect(contrast(dark.ink, dark.surface)).toBeGreaterThanOrEqual(4.5);
  expect(contrast(dark.primaryInk, dark.primary)).toBeGreaterThanOrEqual(4.5);

  await page.emulateMedia({ colorScheme: 'light' });
  await expect(panel).toHaveCSS('background-color', 'rgb(255, 255, 255)');
});

test('system dark theme preserves readable completed rows, feedback, and focus', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.goto('/');
  const title = page.getByRole('textbox', { name: 'New task' });
  await title.fill('Dark task');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByRole('status')).toContainText('Task created');
  await page.getByRole('checkbox', { name: 'Mark Dark task complete' }).check();
  await expect(page.locator('.done .task-title')).toHaveCSS('text-decoration-line', 'line-through');
  const trigger = page.getByRole('button', { name: 'Edit Dark task', exact: true });
  await trigger.focus();
  await expect(trigger).toBeFocused();
  await page.keyboard.press('Enter');
  await expect(page.getByRole('textbox', { name: 'Edit task title' })).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(trigger).toBeFocused();
});

test('dark color scheme respects mobile widths without horizontal overflow', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.setViewportSize({ width: 320, height: 740 });
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Tasks' })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1)).toBe(false);
  await page.setViewportSize({ width: 375, height: 812 });
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1)).toBe(false);
});

test('system theme tokens meet sampled text and control boundary contrast in both schemes', async ({ page }) => {
  for (const scheme of ['light', 'dark'] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.goto('/');
    await expect(page.locator('.panel')).toBeVisible();
    const colors = await page.evaluate(() => {
      const style = getComputedStyle(document.documentElement);
      const names = [
        'canvas', 'surface', 'ink', 'muted', 'completed-ink', 'placeholder-ink',
        'accent', 'primary-ink', 'button-ink', 'input-border', 'button-border',
        'error', 'error-surface', 'success', 'success-surface'
      ];
      return Object.fromEntries(names.map((name) =>
        [name, style.getPropertyValue('--' + name).trim()]
      ));
    });
    const luminosity = (hex: string) => {
      // Vite minification may compact #ffffff to #fff in production CSS.
      const expanded = /^#[0-9a-f]{3}$/i.test(hex)
        ? '#' + [...hex.slice(1)].map((digit) => digit + digit).join('')
        : hex;
      expect(expanded, scheme + ': valid six-digit theme token').toMatch(/^#[0-9a-f]{6}$/i);
      const values = [1, 3, 5].map((index) =>
        parseInt(expanded.slice(index, index + 2), 16) / 255
      );
      const [r, g, b] = values.map((v) =>
        v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4
      );
      return r * 0.2126 + g * 0.7152 + b * 0.0722;
    };
    const contrast = (a: string, b: string) => {
      const first = luminosity(colors[a]);
      const second = luminosity(colors[b]);
      return (Math.max(first, second) + 0.05) / (Math.min(first, second) + 0.05);
    };
    for (const [foreground, background] of [
      ['ink', 'canvas'], ['ink', 'surface'], ['muted', 'surface'],
      ['completed-ink', 'surface'], ['placeholder-ink', 'surface'],
      ['accent', 'surface'], ['button-ink', 'surface'],
      ['primary-ink', 'accent'], ['error', 'error-surface'], ['success', 'success-surface']
    ]) {
      expect(contrast(foreground, background), scheme + ': ' + foreground + '/' + background)
        .toBeGreaterThanOrEqual(4.5);
    }
    for (const border of ['input-border', 'button-border']) {
      expect(contrast(border, 'surface'), scheme + ': ' + border + '/surface')
        .toBeGreaterThanOrEqual(3);
    }
  }
});

test('forced-colors mode retains visible focus and native color adjustment', async ({ page }) => {
  await page.emulateMedia({ colorScheme: 'dark', forcedColors: 'active' });
  await page.goto('/');
  const input = page.getByRole('textbox', { name: 'New task' });
  await expect(input).toBeVisible();
  await input.focus();
  await expect(input).toHaveCSS('outline-style', 'solid');
  const button = page.getByRole('button', { name: 'Add task' });
  await button.focus();
  await expect(button).toHaveCSS('outline-style', 'solid');
  await expect(button).toHaveCSS('forced-color-adjust', 'auto');
});

test('selection is distinct from completion and filters clear selected task IDs', async ({ page, request }) => {
  await page.goto('/');
  for (const title of ['Selection first', 'Selection second', 'Selection third']) {
    await page.getByRole('textbox', { name: 'New task' }).fill(title);
    await page.getByRole('button', { name: 'Add task' }).click();
    await expect(page.getByText(title, { exact: true })).toBeVisible();
  }
  await page.getByRole('button', { name: 'Select tasks' }).click();
  await expect(page.getByRole('checkbox', { name: 'Mark Selection first complete' })).toHaveCount(0);
  const selectionBox = page.getByRole('checkbox', { name: 'Select Selection first' });
  await expect(selectionBox).toHaveCSS('border-radius', '50%');
  await selectionBox.check();
  await expect(page.getByText('1 selected')).toBeVisible();
  const state = (await (await request.get(api)).json()).records;
  expect(state.every((t: { completed: boolean }) => !t.completed)).toBe(true);
  await page.getByRole('button', { name: 'Open', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Select tasks' })).toBeVisible();
  await page.getByRole('checkbox', { name: 'Mark Selection first complete' }).check();
  await expect(page.getByRole('status')).toContainText('Task completed');
});

test('Shift-click range and Ctrl/Cmd-click title use explicit loaded selection', async ({ page }) => {
  await page.goto('/');
  for (const name of ['Range one', 'Range two', 'Range three', 'Range four', 'Range five']) {
    await page.getByRole('textbox', { name: 'New task' }).fill(name);
    await page.getByRole('button', { name: 'Add task' }).click();
    await expect(page.getByText(name, { exact: true })).toBeVisible();
  }
  const titles = await page.locator('li.task .task-title-button').allTextContents();
  await page.getByRole('button', { name: 'Edit ' + titles[0].trim() + ' title' })
    .click({ modifiers: ['Control'] });
  await expect(page.getByText('1 selected')).toBeVisible();
  await page.getByRole('checkbox', { name: 'Select ' + titles[3].trim() })
    .click({ modifiers: ['Shift'] });
  await expect(page.getByText('4 selected')).toBeVisible();
  await expect(page.getByRole('textbox', { name: 'Edit task title' })).toHaveCount(0);
  const input = page.getByRole('textbox', { name: 'New task' });
  await input.fill('Text field should remain editable');
  await input.focus();
  await page.keyboard.press('Control+a');
  await expect(page.getByText('4 selected')).toBeVisible();
  await page.getByRole('checkbox', { name: 'Select ' + titles[3].trim() }).focus();
  await page.keyboard.press('Control+a');
  await expect(page.getByText('5 selected')).toBeVisible();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: 'Select tasks' })).toBeVisible();
  await expect(page.getByRole('checkbox', { name: 'Mark ' + titles[0].trim() + ' complete' })).toBeVisible();
});

test('Select all loaded does not silently select unseen cursor pages', async ({ page, request }) => {
  for (let index = 0; index < 33; index++) {
    const result = await request.post(api, {
      headers: { Origin: origin() }, data: { title: 'Selection page ' + index }
    });
    expect(result.status()).toBe(201);
  }
  await page.goto('/');
  await expect(page.getByText('32 loaded', { exact: false })).toBeVisible();
  await page.getByRole('button', { name: 'Select tasks' }).click();
  await page.getByRole('button', { name: 'Select all loaded' }).click();
  await expect(page.getByText('32 selected')).toBeVisible();
  await page.getByRole('button', { name: 'Load more tasks' }).click();
  await expect(page.getByText('33 loaded', { exact: false })).toBeVisible();
  await expect(page.getByText('32 selected')).toBeVisible();
  await page.getByRole('button', { name: 'Select all loaded' }).click();
  await expect(page.getByText('33 selected')).toBeVisible();
});

test('batch Mark done and Reopen distinguish changed from already matching records', async ({ page, request }) => {
  await page.goto('/');
  for (const name of ['Batch one', 'Batch two', 'Batch three']) {
    await page.getByRole('textbox', { name: 'New task' }).fill(name);
    await page.getByRole('button', { name: 'Add task' }).click();
    await expect(page.getByText(name, { exact: true })).toBeVisible();
  }
  await page.getByRole('checkbox', { name: 'Mark Batch one complete' }).check();
  await page.getByRole('button', { name: 'Select tasks' }).click();
  await page.getByRole('button', { name: 'Select all loaded' }).click();
  await page.getByRole('button', { name: 'Mark done' }).click();
  await expect(page.getByRole('status')).toContainText('2 tasks marked done');
  let items = (await (await request.get(api)).json()).records;
  expect(items.every((r: { completed: boolean }) => r.completed)).toBe(true);
  await page.getByRole('button', { name: 'Select tasks' }).click();
  await page.getByRole('button', { name: 'Select all loaded' }).click();
  await page.getByRole('button', { name: 'Reopen' }).click();
  await expect(page.getByRole('status')).toContainText('3 tasks reopened');
  items = (await (await request.get(api)).json()).records;
  expect(items.every((r: { completed: boolean }) => !r.completed)).toBe(true);
});

test('batch partial conflict keeps failed items selected and never overwrites newer revisions', async ({ page, request }) => {
  await page.goto('/');
  for (const name of ['Batch stale', 'Batch safe']) {
    await page.getByRole('textbox', { name: 'New task' }).fill(name);
    await page.getByRole('button', { name: 'Add task' }).click();
    await expect(page.getByText(name, { exact: true })).toBeVisible();
  }
  await page.getByRole('button', { name: 'Select tasks' }).click();
  await page.getByRole('button', { name: 'Select all loaded' }).click();
  const items = (await (await request.get(api)).json()).records;
  const stale = items.find((record: { title: string }) => record.title === 'Batch stale');
  const external = await request.patch(api + '/' + stale.id, {
    headers: { Origin: origin() },
    data: { revision: stale.revision, title: 'Externally changed' }
  });
  expect(external.status()).toBe(200);
  await page.getByRole('button', { name: 'Mark done' }).click();
  await expect(page.getByRole('alert')).toContainText('1 failed');
  await expect(page.getByText('1 selected')).toBeVisible();
  await expect(page.getByRole('checkbox', { name: 'Select Batch stale' })).toBeChecked();
  const finalItems = (await (await request.get(api)).json()).records;
  expect(finalItems.find((r: { title: string }) => r.title === 'Externally changed').completed).toBe(false);
  expect(finalItems.find((r: { title: string }) => r.title === 'Batch safe').completed).toBe(true);
  await page.getByRole('button', { name: 'Refresh tasks' }).last().click();
  await expect(page.getByRole('button', { name: 'Select tasks' })).toBeVisible();
  await expect(page.getByText('Externally changed', { exact: true })).toBeVisible();
});

test('batch network failure reports partial success without blindly retrying', async ({ page, request }) => {
  const originals = [];
  for (const name of ['Network A', 'Network B']) {
    const response = await request.post(api, {
      headers: { Origin: origin() }, data: { title: name }
    });
    originals.push(await response.json());
  }
  await page.goto('/');
  await page.getByRole('button', { name: 'Select tasks' }).click();
  await page.getByRole('button', { name: 'Select all loaded' }).click();
  await page.route('**/api/v1/tasks/' + originals[0].id, (route) => {
    if (route.request().method() === 'PATCH') return route.abort();
    return route.continue();
  });
  await page.getByRole('button', { name: 'Mark done' }).click();
  await expect(page.getByRole('alert')).toContainText('1 failed');
  const records = (await (await request.get(api)).json()).records;
  expect(records.find((r: { id: string }) => r.id === originals[0].id).completed).toBe(false);
  expect(records.find((r: { id: string }) => r.id === originals[1].id).completed).toBe(true);
  await page.unroute('**/api/v1/tasks/' + originals[0].id);
});

test('selection and direct task controls have no 320px horizontal overflow', async ({ page }) => {
  await page.setViewportSize({ width: 320, height: 740 });
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('Narrow controls');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByRole('button', { name: 'Delete Narrow controls' })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1)).toBe(false);
  await page.getByRole('button', { name: 'Delete Narrow controls' }).click();
  await expect(page.getByRole('button', { name: 'Delete', exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1)).toBe(false);
  await page.getByRole('button', { name: 'Cancel', exact: true }).click();
  await page.getByRole('button', { name: 'Select tasks' }).click();
  await page.getByRole('checkbox', { name: 'Select Narrow controls' }).check();
  await expect(page.getByRole('button', { name: 'Mark done' })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth > innerWidth + 1)).toBe(false);
});

test('selection indicator retains native semantics in forced-colors mode', async ({ page }) => {
  await page.emulateMedia({ forcedColors: 'active' });
  await page.goto('/');
  await page.getByRole('textbox', { name: 'New task' }).fill('Accessible selection');
  await page.getByRole('button', { name: 'Add task' }).click();
  await expect(page.getByText('Accessible selection', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Select tasks' }).click();
  const selection = page.getByRole('checkbox', { name: 'Select Accessible selection' });
  await expect(selection).toHaveCSS('appearance', 'auto');
  await selection.focus();
  await expect(selection).toHaveCSS('outline-style', 'solid');
  await selection.check();
  await expect(selection).toBeChecked();
});
