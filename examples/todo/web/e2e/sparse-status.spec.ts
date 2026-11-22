import { test, expect } from '@playwright/test';

const queryPattern = '**/api/v1/tasks/query?*';

test('real SQLite 10,000-task sparse Open is discovered without repeated manual clicks', async ({ page }) => {
  let requests = 0;
  let inspected = 0;
  let matches = 0;
  // Observe the *real Rust HTTP responses* rather than replaying a mocked
  // Query or assuming a fixed number of network/page transitions.
  await page.route(queryPattern, async (route) => {
    if (route.request().method() !== 'GET') return route.continue();
    requests += 1;
    const response = await route.fetch();
    const payload = await response.json();
    if (response.ok()) {
      inspected += payload.inspected;
      matches += payload.records.length;
    }
    await route.fulfill({ response });
  });

  const started = performance.now();
  await page.goto('/');
  await expect(page.getByText('Sparse Open at UUID end', { exact: true }))
    .toBeVisible({ timeout: 45_000 });
  const elapsedMs = Math.round(performance.now() - started);

  await expect(page.getByRole('button', { name: 'Completed', exact: true }))
    .toHaveAttribute('aria-expanded', 'false');
  await expect(page.locator('.completed-section li.task')).toHaveCount(0);
  await expect(page.locator('.task-section').first().getByText('1 shown')).toBeVisible();

  expect(requests).toBeGreaterThan(0);
  expect(requests).toBeLessThanOrEqual(10);
  expect(inspected).toBe(10_000);
  expect(matches).toBe(1);
  console.log('TODO_REAL_SPARSE_10K_HTTP_CHROMIUM ' +
    JSON.stringify({ requests, inspected, matched: matches, elapsed_ms: elapsedMs }));

  // The scan is *live*; a second client could reclassify rows behind a
  // cursor. The completed scan only certifies the observed live traversal.
  await expect(page.getByText('Open scan complete for now', { exact: false }))
    .toBeVisible();
});
