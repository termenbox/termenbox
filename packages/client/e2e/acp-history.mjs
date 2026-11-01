// R78 step 9: the real AcpView with older turns from the agent's file, in Chrome at 360 px and 1280 px, over a scripted
// server that answers `window` from a fixture (AcpHistoryProbe.svelte). A hit older than the live list shows above it,
// the view scrolls to it and marks the match; a window that arrives later does not move what the reader sees; where the
// file and the live list overlap the live list wins; and many windows keep one in memory.
// Desktop Chromium with mobile emulation: this is NOT an iPhone or Android check (guardrail a-phone-check-names-its-platform).
// Usage: node e2e/acp-history.mjs   (needs the e2e harness built: pnpm e2e:build; CHROME_PATH for the browser)
import { launch, contextOptions } from './browser.mjs';
import { createServer } from 'node:http';
import { readFileSync, existsSync } from 'node:fs';
import { join, extname } from 'node:path';

const dist = join(import.meta.dirname, 'dist');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const fail = (m) => { console.error(`FAIL: ${m}`); process.exitCode = 1; };
const check = (cond, m) => { if (!cond) fail(m); else console.log(`ok  ${m}`); return !!cond; };
const srv = createServer((req, res) => {
  const f = join(dist, req.url.split('?')[0]);
  if (!f.startsWith(dist) || !existsSync(f)) return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
});
await new Promise((r) => srv.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${srv.address().port}`;
const browser = await launch();
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// Where the scroller's visible area is, and where an element is inside it.
const inView = (page, sel) => page.evaluate((s) => {
  const el = document.querySelector(s), box = document.querySelector('[data-testid=stream]').getBoundingClientRect();
  if (!el) return null;
  const r = el.getBoundingClientRect();
  return { top: r.top - box.top, visible: r.bottom > box.top && r.top < box.bottom };
}, sel);
const firstVisible = (page) => page.evaluate(() => {
  const box = document.querySelector('[data-testid=stream]').getBoundingClientRect();
  for (const row of document.querySelectorAll('[data-entry]')) { const r = row.getBoundingClientRect(); if (r.bottom > box.top + 1) return { id: row.dataset.entry, top: r.top - box.top }; }
  return null;
});

try {
  for (const [name, opts] of [['360 px phone', { viewport: { width: 360, height: 740 }, isMobile: true, hasTouch: true }], ['1280 px desktop', { viewport: { width: 1280, height: 800 } }]]) {
    const ctx = await browser.newContext(contextOptions(opts));
    const page = await ctx.newPage();
    page.on('pageerror', (e) => fail(`${name}: page error: ${e.message}`));
    page.on('console', (m) => { if (m.type() === 'error') fail(`${name}: console error: ${m.text()}`); });
    await page.goto(`${base}/acp-history.html`);
    await page.waitForSelector('[data-testid=stream]');
    await sleep(300);
    check(await page.locator('[data-testid=history-note]').count() === 0, `${name}: no older turns before a hit is tapped`);

    // 1. A hit older than the live list: shown above it, scrolled to, marked.
    await page.evaluate(() => window.__acpHistory.land('h5', 'needle'));
    await page.waitForSelector('[data-turn=h5]');
    await sleep(300);
    const t = await inView(page, '[data-turn=h5]');
    check(t?.visible, `${name}: the tapped turn is on screen`);
    const mark = await page.locator('[data-turn=h5] mark[data-hit]').first().textContent();
    check(mark === 'needle', `${name}: the match is marked (${mark})`);
    const order = await page.evaluate(() => [...document.querySelectorAll('[data-entry]')].map((e) => Number(e.dataset.entry)));
    check(order.slice(0, 12).every((id) => id < 0) && order.slice(12).every((id) => id >= 100), `${name}: the file turns sit above the live ones`);
    check(await page.evaluate(() => window.__acpHistory.asks.join()) === 'h5', `${name}: the server was asked once for the turn`);

    // 2. A window that arrives while the reader is elsewhere does not move what they see.
    await page.evaluate(() => { const s = document.querySelector('[data-testid=stream]'); s.scrollTop = s.scrollHeight / 2; });
    await sleep(500);
    const before = await firstVisible(page);
    await page.evaluate(() => window.__acpHistory.push(30));
    await sleep(700);
    const after = await firstVisible(page);
    check(before && after && before.id === after.id && Math.abs(before.top - after.top) <= 1.5, `${name}: a prepend leaves the visible entry where it was (${before?.id}@${before?.top} -> ${after?.id}@${after?.top})`);

    // 3. Many windows: one is held, and the page draws at most the cap.
    for (let i = 0; i < 25; i++) { await page.evaluate((n) => window.__acpHistory.push(n), 10 + i); }
    await sleep(500);
    check(await page.evaluate(() => window.__acpHistory.held()) === 1, `${name}: one window is held for the workspace`);
    check(await page.evaluate(() => document.querySelectorAll('[data-turn]').length) === 34, `${name}: only the last window is drawn`);

    // 4. Where the file and the live list overlap, the live list wins: no turn is drawn twice, and the hit lands on the live entry.
    await page.evaluate(() => window.__acpHistory.land('joined', 'question 0'));
    await sleep(800);
    const dup = await page.evaluate(() => [...document.querySelectorAll('[data-entry]')].filter((e) => e.textContent.includes('live question 0') && !/live question 0\d/.test(e.textContent)).length);
    check(dup === 1, `${name}: the overlapping turn is drawn once (${dup})`);
    const liveMark = await page.locator('[data-entry="100"] mark[data-hit]').count();
    check(liveMark > 0, `${name}: the match is marked in the live entry`);
    await ctx.close();
  }
} catch (e) {
  fail(String(e));
} finally {
  await browser.close();
  srv.close();
}
