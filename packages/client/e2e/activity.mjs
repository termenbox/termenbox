// U12: the real Activity view in Chrome at 360 px (a small phone) and at 1280 px, over a fake server that answers from a fixture.
// The clock is fixed to the fixture's day. Desktop Chromium with mobile emulation: not an iPhone or Android check.
// Usage: node e2e/activity.mjs   (needs the e2e harness built: pnpm e2e:build; CHROME_PATH for the browser)
import { chromium } from 'playwright-core';
import { createServer } from 'node:http';
import { readFileSync, existsSync } from 'node:fs';
import { join, extname } from 'node:path';

const dist = join(import.meta.dirname, 'dist');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const fail = (m) => { console.error(`FAIL: ${m}`); process.exitCode = 1; };
const check = (cond, m) => { if (!cond) fail(m); return !!cond; };
const srv = createServer((req, res) => {
  const f = join(dist, req.url.split('?')[0]);
  if (!f.startsWith(dist) || !existsSync(f)) return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
});
await new Promise((r) => srv.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${srv.address().port}`;
const browser = await chromium.launch({
  ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: 'chrome' }),
  args: ['--no-sandbox'],
});
const NOW = new Date(2026, 9, 14, 18, 0);
const overflow = (page) => page.evaluate(() => document.documentElement.scrollWidth - innerWidth);
const small = (page) => page.$$eval('[data-activity] button, [data-activity] [role=radio]', (els) => els.filter((e) => { const r = e.getBoundingClientRect(); return r.width > 0 && (r.height < 43.5 || r.width < 43.5); }).map((e) => e.getAttribute('aria-label') || e.textContent.trim()));
const tiny = (page) => page.evaluate(() => [...document.querySelectorAll('[data-activity] *')].filter((e) => [...e.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim()) && e.getBoundingClientRect().width > 0 && parseFloat(getComputedStyle(e).fontSize) < 12).map((e) => e.textContent.trim().slice(0, 20)));
const clipped = (page) => page.$$eval('[data-digest]', (els) => els.filter((e) => e.scrollWidth > e.clientWidth).map((e) => e.textContent));
async function open(ctx, query = '') {
  const page = await ctx.newPage();
  page.on('pageerror', (e) => fail(`page error: ${e.message}`));
  page.on('console', (m) => { if (m.type() === 'error') fail(`console error: ${m.text()}`); });
  await page.clock.install({ time: NOW });
  await page.goto(`${base}/activity.html${query}`);
  return page;
}
try {
  for (const [name, opts] of [['360 px phone', { viewport: { width: 360, height: 740 }, isMobile: true, hasTouch: true }], ['1280 px desktop', { viewport: { width: 1280, height: 800 } }]]) {
    const ctx = await browser.newContext(opts);
    const page = await open(ctx);
    await page.waitForSelector('[data-activity=ready]');

    // The digest and the timeline of the day.
    const digest = await page.$$eval('[data-digest]', (els) => els.map((e) => e.textContent));
    check(JSON.stringify(digest) === JSON.stringify(['2', '1', '14m']), `digest: ${digest}`);
    const timeline = await page.locator('[data-activity-timeline] li').allInnerTexts();
    check(timeline.length === 5 && timeline[0].includes('Review the deploy script') && timeline[0].includes('Running') && timeline[4].includes('Fix the flaky login test') && timeline[4].includes('waited 4 min'), `timeline: ${JSON.stringify(timeline)}`);
    check((await page.locator('[data-bar-chart]').count()) === 3, 'the three charts are missing');
    for (const label of await page.locator('[data-bar-chart]').evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')))) check(/: .+/.test(label ?? '') && !label.endsWith('nothing yet'), `a chart has no spoken summary: ${label}`);
    check((await overflow(page)) <= 0, `${name}: overflows sideways by ${await overflow(page)} px`);
    check((await small(page)).length === 0, `${name}: targets under 44 px: ${await small(page)}`);
    check((await tiny(page)).length === 0, `${name}: text under 12 px: ${await tiny(page)}`);
    check((await clipped(page)).length === 0, `${name}: a digest value is clipped: ${await clipped(page)}`);
    console.log(`ok  ${name}: day digest 2 finished, 1 failed, 14m waited; 5 tasks newest first; 3 charts with a spoken summary; no sideways scroll; targets >= 44 px; text >= 12 px`);

    // Usage and cost: counts where agents reported them, the honest "not available" where they did not, nothing invented.
    const usage = await page.locator('[data-activity-usage]').innerText();
    const claude = await page.locator('[data-usage-agent="claude"]').innerText();
    const codex = await page.locator('[data-usage-agent="codex"]').innerText();
    check(claude.includes('18k tokens') && claude.includes('cache read 150k') && claude.includes('Rate limited once'), `claude usage: ${claude}`);
    check(claude.includes('Cost: not available from claude') && claude.includes('R19'), `claude cost state: ${claude}`);
    check(codex.includes('Tokens: not available from codex') && codex.includes('notify hook carries no usage') && !/\b0 tokens|\$0/.test(codex), `codex state: ${codex}`);
    const projects = await page.$$eval('[data-usage-project]', (els) => els.map((e) => e.dataset.usageProject));
    check(projects.join('|') === 'api|No folder reported|web', `projects ranked by tokens, those with no counts by name: ${projects}`);
    check(!usage.includes('Prompt cache now') && !usage.includes('$'), 'a cost or a cache estimate appears with no source for it');
    console.log(`ok  ${name}: usage per agent and project from the fixture; cost and tokens "not available" for codex and the unnamed agent, never zeros`);
    const rep = await open(ctx, '?reported');
    await rep.waitForSelector('[data-activity=ready]');
    const kilo = await rep.locator('[data-usage-agent="kilo"]').innerText();
    check(kilo.includes('$0.25') && kilo.includes('reported by kilo'), `reported cost: ${kilo}`);
    check((await rep.locator('[data-cache]').allInnerTexts()).join() === 'about 3 min', 'the warm cache estimate is missing or not marked as about');
    check((await overflow(rep)) <= 0, `${name}: usage overflows sideways by ${await overflow(rep)} px`);
    check((await small(rep)).length === 0, `${name}: usage targets under 44 px: ${await small(rep)}`);
    check((await tiny(rep)).length === 0, `${name}: usage text under 12 px: ${await tiny(rep)}`);
    console.log(`ok  ${name}: a cost the agent reported shows as its own, with a labelled cache estimate; no sideways scroll; text >= 12 px`);
    await rep.close();

    // The hub publishes a new servers array on every agent event; that must not send a new query to every server.
    const asked = await page.evaluate(() => window.__activity.asks.length);
    for (let i = 0; i < 3; i++) await page.evaluate(() => window.__activity.rebuild());
    await page.waitForTimeout(150);
    check((await page.evaluate(() => window.__activity.asks.length)) === asked, `${name}: a new servers array with the same connected servers re-asked for the history`);

    // Dense bars: no two labels of a chart may run into each other (a busy morning, long wait labels).
    const dense = await open(ctx, '?dense');
    await dense.waitForSelector('[data-activity=ready]');
    // The value texts above the bars are one row, the axis labels another: within each row, no text may touch the next.
    const rows = await dense.$$eval('[data-bar-chart]', (charts) => charts.flatMap((c) => [c.querySelectorAll('[data-bar] > span'), c.querySelectorAll(':scope > div:last-child > span')].map((spans) => [...spans].filter((s) => s.textContent.trim()).map((s) => { const r = document.createRange(); r.selectNodeContents(s); const b = r.getBoundingClientRect(); return [b.left, b.right, s.textContent]; }))));
    for (const [i, rects] of rows.entries()) {
      const sorted = rects.sort((a, b) => a[0] - b[0]);
      const touching = sorted.filter((r, k) => k && sorted[k - 1][1] > r[0] - 1).map((r) => r[2]);
      check(touching.length === 0, `${name}: row ${i} of the charts has overlapping labels: ${touching}`);
    }
    check(rows[0].length > 1 && rows[2].length > 1, 'the dense probe shows no labels at all');
    await dense.close();

    // Week: another range is asked for, 7 bars, tasks of two days.
    await page.getByRole('radio', { name: 'Week' }).click();
    await page.waitForFunction(() => document.querySelectorAll('[data-bar-chart] [data-bar]').length === 21);
    const asks = await page.evaluate(() => window.__activity.asks);
    check(asks.length >= 2 && asks.at(-1).to - asks.at(-1).from > 6 * 82_800_000, `the week range was not asked for: ${JSON.stringify(asks)}`);
    check((await page.locator('[data-activity-timeline] h3').allInnerTexts()).join('|') === 'Today|Yesterday', 'the week timeline lacks Today and Yesterday');
    check((await overflow(page)) <= 0, `${name}: week overflows sideways`);
    await page.getByRole('radio', { name: 'Day' }).click();
    await page.getByRole('button', { name: 'Previous day' }).click();
    await page.waitForFunction(() => document.querySelector('[data-activity-range]')?.textContent === 'Yesterday');
    check(await page.getByRole('button', { name: 'Next day' }).isEnabled(), 'Next day is disabled the day after');
    check((await page.locator('[data-activity-timeline] li').count()) === 1, 'yesterday should hold one task');
    await page.getByRole('button', { name: 'Next day' }).click();
    check(await page.getByRole('button', { name: 'Next day' }).isDisabled(), 'Next day is enabled on today');
    console.log(`ok  ${name}: week and day ranges, previous and next day, nothing after today`);

    // Keyboard: the range is one tab stop and the arrow keys move it.
    await page.getByRole('radio', { name: 'Day' }).focus();
    await page.keyboard.press('ArrowRight');
    check((await page.getByRole('radio', { name: 'Week' }).getAttribute('aria-checked')) === 'true', 'ArrowRight did not select Week');
    console.log(`ok  ${name}: the range control works by keyboard`);
    await ctx.close();
  }

  // A server older than the feature never answers: the view says so instead of loading forever.
  const ctx = await browser.newContext({ viewport: { width: 360, height: 740 }, isMobile: true, hasTouch: true });
  const page = await open(ctx, '?silent');
  await page.waitForSelector('[data-activity=loading]');
  await page.clock.fastForward(9000);
  await page.evaluate(() => window.__activity.tick());
  await page.waitForSelector('[data-activity=unavailable]');
  check((await page.innerText('[data-activity]')).includes('alpha did not answer'), 'the unavailable state does not name the server');
  console.log('ok  a server that never answers is reported after 8 s, not left loading');
  await ctx.close();
} finally {
  await browser.close();
  srv.close();
}
