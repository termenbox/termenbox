// GUARDRAIL errors-fit-one-screen: every Add Server failure state fits a 375x667 phone without scrolling, with one primary
// action and its detail behind a fold that starts closed. The states are the real step components in the real Dialog.
// Desktop Chromium with mobile emulation: not an iPhone or Android check.
// Usage: node e2e/failures.mjs   (needs the e2e harness built: pnpm e2e:build; CHROME_PATH for the browser)
import { launch, contextOptions } from './browser.mjs';
import { createServer } from 'node:http';
import { readFileSync, existsSync } from 'node:fs';
import { join, extname } from 'node:path';

const dist = join(import.meta.dirname, 'dist');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const fail = (m) => { console.error(`FAIL: ${m}`); process.exitCode = 1; };
const srv = createServer((req, res) => {
  const f = join(dist, req.url.split('?')[0]);
  if (!existsSync(f)) return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
});
await new Promise((r) => srv.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${srv.address().port}`;
const browser = await launch();
const STATES = ['expired_link', 'bad_link', 'not_found', 'udp_closed', 'announced_unreachable', 'udp_closed_no_fallback', 'udp_closed_with_fallback', 'permission_denied', 'wrong_code', 'burned_code', 'different_network', 'guest_isolation'];
try {
  const ctx = await browser.newContext(contextOptions({ viewport: { width: 375, height: 667 }, isMobile: true, hasTouch: true }));
  const scrolls = [];
  const opened = [];
  const actions = [];
  for (const s of STATES) {
    const page = await ctx.newPage();
    page.on('pageerror', (e) => fail(`${s}: page error ${e.message}`));
    page.on('console', (m) => { if (m.type() === 'error') fail(`${s}: console error ${m.text()}`); });
    await page.goto(`${base}/failures.html?s=${s}`);
    await page.getByRole('dialog').waitFor();
    await page.waitForTimeout(150);
    const m = await page.evaluate(() => {
      const d = document.querySelector('[role=dialog]');
      const rect = d.getBoundingClientRect();
      return {
        dialogScroll: d.scrollHeight - d.clientHeight,
        pageScroll: document.documentElement.scrollHeight - innerHeight,
        bottom: rect.bottom, height: innerHeight,
        open: [...document.querySelectorAll('details')].filter((x) => x.open).length,
        folds: document.querySelectorAll('details').length,
        primary: document.querySelectorAll('[data-testid=failure-action]').length,
      };
    });
    if (m.dialogScroll > 0 || m.pageScroll > 0 || m.bottom > m.height) scrolls.push(`${s} (dialog +${m.dialogScroll}px, page +${m.pageScroll}px, bottom ${Math.round(m.bottom)}/${m.height})`);
    if (m.open) opened.push(s);
    if (!m.folds) opened.push(`${s} (no fold at all)`);
    // One primary action in a failure; the Done panel has its own single Done button instead.
    if (!s.startsWith('udp_closed_') && m.primary !== 1) actions.push(`${s}: ${m.primary} primary actions`);
    await page.close();
  }
  if (scrolls.length) fail(`error_states_fit_375x667: these states scroll at 375x667: ${scrolls.join('; ')}`);
  else console.log(`ok  error_states_fit_375x667: ${STATES.length} failure states fit 375x667 with no scrolling`);
  if (opened.length) fail(`details_are_folded: open on first render, or missing: ${opened.join(', ')}`);
  else console.log(`ok  details_are_folded: every state's detail fold is closed on first render`);
  if (actions.length) fail(`one primary action: ${actions.join('; ')}`);
  else console.log('ok  each failure shows exactly one primary action');
} finally {
  await browser.close();
  srv.close();
}
