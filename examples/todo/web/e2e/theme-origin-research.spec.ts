/** #106 research fixture; no Todo product cookie is implemented or authorized. */
import { test, expect } from '@playwright/test';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';

const name = 'riesel_todo_theme_research_probe';
type Host = { server: Server; url: string; seen: string[] };
const host = (): Promise<Host> => new Promise((resolve, reject) => {
  const seen: string[] = [];
  const server = createServer((req, res) => {
    seen.push(req.headers.cookie ?? '');
    res.writeHead(200, { 'Content-Type': 'text/html', 'Cache-Control': 'no-store' });
    res.end('<!doctype html><html><head><title>Research</title></head><body>Research</body></html>');
  });
  server.once('error', reject);
  server.listen(0, '127.0.0.1', () => {
    server.removeListener('error', reject);
    const address = server.address() as AddressInfo;
    resolve({ server, url: 'http://127.0.0.1:' + address.port, seen });
  });
});
const stop = async (...hosts: Host[]): Promise<void> => {
  await Promise.all(hosts.map(h => new Promise<void>((resolve, reject) =>
    h.server.close(error => error ? reject(error) : resolve())
  )));
};

test('theme cookie crosses ports; localStorage does not; other ports receive cookie', async ({ page }) => {
  const first = await host(), second = await host();
  try {
    expect(first.url).not.toBe(second.url);
    await page.goto(first.url);
    await page.evaluate(key => {
      document.cookie = key + '=dark; Path=/; SameSite=Strict; Max-Age=120';
      localStorage.setItem(key, 'dark');
    }, name);
    expect(await page.evaluate(key => document.cookie.includes(key + '=dark'), name)).toBe(true);
    await page.goto(second.url);
    expect(await page.evaluate(key => document.cookie.includes(key + '=dark'), name)).toBe(true);
    expect(await page.evaluate(key => localStorage.getItem(key), name)).toBeNull();
    expect(second.seen.some(s => s.includes(name + '=dark'))).toBe(true);
    await page.evaluate(key => {
      document.cookie = key + '=; Path=/; SameSite=Strict; Max-Age=0';
    }, name);
    await page.goto(first.url);
    expect(await page.evaluate(key => document.cookie.includes(key + '=dark'), name)).toBe(false);
    expect(await page.evaluate(key => localStorage.getItem(key), name)).toBe('dark');
  } finally {
    await stop(first, second);
  }
});

test('appearance cookie remains isolated across browser profiles', async ({ browser }) => {
  const server = await host();
  const first = await browser.newContext(), second = await browser.newContext();
  try {
    const p1 = await first.newPage();
    await p1.goto(server.url);
    await p1.evaluate(key => {
      document.cookie = key + '=light; Path=/; SameSite=Strict; Max-Age=120';
    }, name);
    const p2 = await second.newPage();
    await p2.goto(server.url);
    expect(await p1.evaluate(key => document.cookie.includes(key + '=light'), name)).toBe(true);
    expect(await p2.evaluate(key => document.cookie.includes(key + '='), name)).toBe(false);
  } finally {
    await first.close();
    await second.close();
    await stop(server);
  }
});
