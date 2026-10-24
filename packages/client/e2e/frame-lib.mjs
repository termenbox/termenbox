// Shared by appshell.mjs (phone, 390 px) and desktop.mjs (1280 px): serves e2e/dist, opens the frame probe in
// the selected browser (browser.mjs), and holds the checks both widths make. The probe is the real AppFrame, bottom bar, sidebar, palette, NavigationBar,
// SessionSheet and SessionStrip on the real ServerHub with two fake servers; the terminal is a stand-in textarea, so this
// proves what the page does with a key, not what xterm or a shell does with it.
import { launch, contextOptions, trace, keepIfFailed, ENGINE } from './browser.mjs';
import { createServer } from 'node:http';
import { readFileSync, existsSync } from 'node:fs';
import { join, extname, basename } from 'node:path';

const dist = join(import.meta.dirname, 'dist');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };

export async function start(options) {
  const srv = createServer((req, res) => {
    const f = join(dist, req.url.split('?')[0]);
    if (!f.startsWith(dist) || !existsSync(f)) return res.writeHead(404).end();
    res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
  });
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  const browser = await launch();
  const ctx = await browser.newContext(contextOptions(options));
  const label = basename(process.argv[1], '.mjs');
  await trace(ctx, label);
  const page = await ctx.newPage();
  if (process.env.E2E_CPU_THROTTLE && ENGINE === 'chromium') await (await ctx.newCDPSession(page)).send('Emulation.setCPUThrottlingRate', { rate: Number(process.env.E2E_CPU_THROTTLE) });
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  const url = `http://127.0.0.1:${srv.address().port}/frame.html`;
  let failed = false;
  const t = {
    page, errors, url,
    fail: (m) => { failed = true; console.error(`FAIL: ${m}`); },
    ok: (m) => console.log(`ok  ${m}`),
    check(cond, m) { if (!cond) t.fail(m); return !!cond; },
    async open(hash = '') {
      await page.goto(url + hash);
      await page.getByRole('navigation', { name: 'Main' }).first().waitFor();
      await page.waitForFunction(() => document.querySelectorAll('[data-shell=frame]').length === 1 && window.__frame);
    },
    api: (fn, ...args) => page.evaluate(([f, a]) => window.__frame[f](...a), [fn, args]),
    async destination() { return t.api('destination'); },
    /** Visible interactive controls inside a selector that are smaller than 44 x 44. */
    async smallTargets(selector) {
      return page.evaluate((sel) => [...document.querySelectorAll(`${sel} button, ${sel} [role=option], ${sel} [role=tab], ${sel} input, ${sel} a[href]`)]
        .filter((e) => { const r = e.getBoundingClientRect(); return r.width > 0 && r.height > 0; })
        .filter((e) => { const r = e.getBoundingClientRect(); return r.width < 43.5 || r.height < 43.5; })
        .map((e) => `${(e.getAttribute('aria-label') || e.textContent).trim().slice(0, 24)} ${Math.round(e.getBoundingClientRect().width)}x${Math.round(e.getBoundingClientRect().height)}`), selector);
    },
    async overflow() { return page.evaluate(() => document.documentElement.scrollWidth - innerWidth); },
    async rects(selector) {
      return page.evaluate((sel) => [...document.querySelectorAll(sel)].map((e) => { const r = e.getBoundingClientRect(); return `${e.dataset.key ?? e.textContent.trim().slice(0, 20)}:${Math.round(r.x)},${Math.round(r.y)},${Math.round(r.width)},${Math.round(r.height)}`; }), selector);
    },
    async terminalKeys(keys) {
      await t.api('clearKeys');
      await page.locator('textarea[aria-label="Terminal input"]').focus();
      for (const k of keys) await page.keyboard.press(k);
      return t.api('keys');
    },
    /** Ctrl+C, Tab and Escape are ordinary keys: with no palette open they must reach the terminal untouched. */
    async ordinaryKeysReachTerminal() {
      const got = await t.terminalKeys(['Control+c', 'Escape', 'Tab']);
      const find = (key) => got.find((k) => k.key === key);
      t.check(find('c')?.ctrl && find('c').prevented === false, 'Ctrl+C did not reach the terminal untouched');
      t.check(find('Tab') && find('Tab').prevented === false, 'Tab did not reach the terminal untouched');
      t.check(find('Escape') && find('Escape').prevented === false, 'Escape did not reach the terminal untouched');
      t.ok('Ctrl+C, Tab and Escape reach the terminal untouched while the palette is closed');
    },
    /** Ctrl+K from the terminal opens the palette, the "k" does not reach the terminal, a name finds a session of the other server, Enter opens it. */
    async paletteOpensSessionByName() {
      await page.locator('textarea[aria-label="Terminal input"]').focus();
      await t.api('clearKeys');
      await page.keyboard.press('Control+k');
      const input = page.getByRole('combobox', { name: 'Search' });
      await input.waitFor();
      t.check(!(await t.api('keys')).some((k) => k.key.toLowerCase() === 'k'), 'Ctrl+K reached the terminal');
      await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'combobox', null, { timeout: 5000 })
        .catch(() => t.check(false, 'the palette input did not take focus'));
      await page.keyboard.type('needle');
      const options = page.getByRole('option');
      await options.first().waitFor();
      t.check((await options.count()) === 1, `searching "needle" among 20 sessions gave ${await options.count()} results`);
      t.check((await options.first().innerText()).includes('beta'), 'the result does not say which server it is on');
      await page.keyboard.press('Enter');
      await input.waitFor({ state: 'detached' });
      t.check((await t.api('active')) === 'beta/needle', `Enter opened ${await t.api('active')}, not beta/needle`);
      t.ok('the palette found "needle" among 20 sessions on two servers and opened it');
    },
    /** Escape closes the palette and focus returns to where it was. */
    async escapeRestoresFocus() {
      const ta = page.locator('textarea[aria-label="Terminal input"]');
      await ta.focus();
      await page.keyboard.press('Control+k');
      await page.getByRole('combobox', { name: 'Search' }).waitFor();
      await page.keyboard.press('Escape');
      await page.getByRole('combobox', { name: 'Search' }).waitFor({ state: 'detached' });
      t.check(await ta.evaluate((e) => document.activeElement === e), 'focus did not return to the terminal after Escape');
      t.ok('Escape closes the palette and returns focus');
    },
    async stripKeyboard() {
      const strip = page.getByRole('tablist', { name: 'Sessions' }).last();
      const tabs = strip.getByRole('tab');
      const n = await tabs.count();
      t.check(n >= 10, `the strip shows ${n} tabs`);
      t.check((await strip.locator('[role=tab][tabindex="0"]').count()) === 1, 'the strip is not one tab stop');
      const selected = strip.locator('[role=tab][aria-selected=true]');
      await selected.focus();
      await t.api('clearKeys');
      const focusedId = () => page.evaluate(() => document.activeElement?.dataset?.id ?? null);
      const first = await focusedId();
      await page.keyboard.press('ArrowRight');
      const second = await focusedId();
      t.check(second && second !== first, 'ArrowRight did not move focus to the next tab');
      await page.keyboard.press('ArrowLeft');
      t.check((await focusedId()) === first, 'ArrowLeft did not move back');
      await page.keyboard.press('End');
      const last = await tabs.nth(n - 1).getAttribute('data-id');
      t.check((await focusedId()) === last, 'End did not go to the last tab');
      await page.keyboard.press('Home');
      t.check((await focusedId()) === (await tabs.nth(0).getAttribute('data-id')), 'Home did not go to the first tab');
      await page.keyboard.press('ArrowLeft');
      t.check((await focusedId()) === last, 'ArrowLeft from the first tab did not wrap');
      t.check((await strip.locator('[role=tab][tabindex="0"]').count()) === 1, 'the strip has more than one tab stop after moving');
      await page.keyboard.press('Space');
      t.check((await t.api('active')) === last, `Space did not open the focused tab (${await t.api('active')})`);
      t.check((await t.api('keys')).length === 0, 'a strip key reached the terminal');
      const inView = await page.evaluate((id) => { const el = document.querySelector(`[role=tab][data-id="${id}"]`); const p = el.parentElement.getBoundingClientRect(), r = el.getBoundingClientRect(); return r.left >= p.left - 1 && r.right <= p.right + 1; }, last);
      t.check(inView, 'the focused tab was not scrolled into view');
      // The focused tab goes away: focus must land on a tab, not on the page.
      await page.keyboard.press('Home');
      const goner = await focusedId();
      const [server, id] = goner.split('/');
      await t.api('remove', server, id);
      await page.waitForFunction((g) => !document.querySelector(`[role=tab][data-id="${g}"]`), goner);
      await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'tab', null, { timeout: 2000 }).catch(() => {});
      t.check(await page.evaluate(() => document.activeElement?.getAttribute('role') === 'tab'), 'focus was lost when the focused tab was removed');
      t.ok('keyboard-only navigation of the session strip: one tab stop, arrows, Home/End, Space, scroll into view, focus kept on removal, no key reaches the terminal');
    },
    async inSessionTabSwitch() {
      const tab = (key) => page.locator(`[role=tab][data-key="${key}"]`);
      await tab('alpha/svc-0').click();
      t.check((await t.api('active')) === 'alpha/svc-0', 'could not open a session from the in-session tab strip');
      await tab('alpha/svc-3').click();
      t.check((await t.api('active')) === 'alpha/svc-3', 'the in-session tab switch no longer works');
      t.ok('the in-session tab strip (TabStrip) still switches sessions');
    },
    /** Closes the browser and says whether every check passed; for a script that runs more after this one. */
    async end() {
      t.check(errors.length === 0, `console/page errors: ${errors.slice(0, 3).join(' | ')}`);
      await keepIfFailed(ctx, label, failed);
      await browser.close(); srv.close();
      return !failed;
    },
    async finish() {
      t.check(errors.length === 0, `console/page errors: ${errors.slice(0, 3).join(' | ')}`);
      await keepIfFailed(ctx, label, failed);
      await browser.close(); srv.close();
      if (failed) process.exit(1);
      console.log('PASS');
    },
    browser,
  };
  return t;
}
