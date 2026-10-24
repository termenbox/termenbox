// The configurable key row (U07c) in real Chrome, no server needed: the real KeyRow and Settings editor around a log of what the
// keys send. Checks, at 390 px on iPhone emulation and on Android emulation (emulated only, not a device): the row has today's
// 16 keys at least 44 pt tall and the same boxes on both; Tab reaches every key and Enter and Space each send one action; a touch
// tap and a pointerdown followed by a synthesized click send one action; modifiers expose aria-pressed; focus is visible; the
// editor changes, persists and resets the row; removed keys stay reachable in More; at 320 px nothing shrinks below the minimum
// and the rest goes to More. At 1024 px with a fine pointer the Auto row is compact (gains height), and Full restores the row.
// Usage: node e2e/keyrow.mjs   (needs `pnpm e2e:build`; SHOTS=dir for screenshots, CHROME_PATH for the browser)
import { launch } from './browser.mjs';
import { createServer } from 'node:http';
import { readFileSync, existsSync, mkdirSync } from 'node:fs';
import { join, extname } from 'node:path';

const here = import.meta.dirname;
const dist = join(here, 'dist');
const SHOTS = process.env.SHOTS ?? join(dist, 'keyrow-shots');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const server = createServer((req, res) => {
  const file = join(dist, req.url.split('?')[0]);
  if (!file.startsWith(dist) || !existsSync(file)) return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': mime[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}/keyrow.html`;

const problems = [];
const fail = (m) => { problems.push(m); console.error("FAIL " + m); };
const check = (ok, m) => { if (!ok) fail(m); };
const browser = await launch();
mkdirSync(SHOTS, { recursive: true });

const PHONES = {
  'iPhone (emulated Safari-like viewport; not a device)': { viewport: { width: 390, height: 844 }, deviceScaleFactor: 3, isMobile: true, hasTouch: true, userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 16_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.4 Mobile/15E148 Safari/604.1' },
  'Android: emulated only': { viewport: { width: 390, height: 844 }, deviceScaleFactor: 2.75, isMobile: true, hasTouch: true, userAgent: 'Mozilla/5.0 (Linux; Android 13; Pixel 7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/111.0.0.0 Mobile Safari/537.36' },
};

const keys = (page) => page.getByRole('toolbar', { name: 'Terminal keys' }).getByRole('button');
const boxes = async (page) => page.evaluate(() => [...document.querySelectorAll('[role=toolbar][aria-label="Terminal keys"] button')].map((b) => { const r = b.getBoundingClientRect(); return [b.getAttribute('aria-label'), Math.round(r.x * 10) / 10, Math.round(r.y * 10) / 10, Math.round(r.width * 10) / 10, Math.round(r.height * 10) / 10]; }));
const sent = (page) => page.evaluate(() => window.__sent.length);
const names = (page) => keys(page).evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')));
const open = async (page, query = '') => { await page.goto(base + query); await page.waitForSelector('[role=toolbar]'); await page.evaluate(() => document.fonts.ready); };

const TODAY = ['Slash', 'Control C', 'Escape', 'New line without sending (Alt+Enter)', 'Ctrl', 'Alt', 'Shift', 'Tab', 'Fn', 'Arrow up', 'Arrow down', 'Show or hide the keyboard', 'Dictate', 'Dictation language: follows what you type', 'Smaller text', 'Larger text'];
const layouts = [];

for (const [label, opts] of Object.entries(PHONES)) {
  const ctx = await browser.newContext(opts);
  const page = await ctx.newPage();
  const errors = [];
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  page.on('pageerror', (e) => errors.push(String(e)));
  await open(page);
  const tag = label.split(' ')[0];

  // 1. Today's row: names, order, size, no overflow, no More key.
  const nm = await names(page);
  check(JSON.stringify(nm) === JSON.stringify(TODAY), `${tag}: default row is not today's 16 keys: ${nm.join(' | ')}`);
  const bx = await boxes(page);
  layouts.push(JSON.stringify(bx));
  for (const [n, , , w, h] of bx) {
    check(h >= 43.5, `${tag}: ${n} is ${h} pt tall`);
    check(w >= 17.5, `${tag}: ${n} is ${w} pt wide, under the minimum`);
  }
  check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), `${tag}: horizontal overflow`);
  await page.screenshot({ path: join(SHOTS, `keyrow-${tag.toLowerCase()}-390-default.png`) });

  // 2. Tab to each key; Enter and Space send one action each; modifiers flip aria-pressed; focus is visible.
  const first = keys(page).first();
  await first.focus();
  for (let i = 0; i < TODAY.length; i++) {
    if (i > 0) await page.keyboard.press('Tab');
    const active = await page.evaluate(() => document.activeElement?.getAttribute('aria-label'));
    check(active === TODAY[i], `${tag}: Tab ${i} reached ${active}, wanted ${TODAY[i]}`);
    const outline = await page.evaluate(() => { const s = getComputedStyle(document.activeElement); return `${s.outlineStyle} ${s.outlineWidth}`; });
    check(!/^none|\b0px/.test(outline), `${tag}: no visible focus on ${TODAY[i]} (${outline})`);
    const modOnly = ['Ctrl', 'Alt', 'Shift'].includes(TODAY[i]);
    for (const key of ['Enter', 'Space']) {
      const before = await sent(page);
      const pressed = await page.evaluate(() => document.activeElement.getAttribute('aria-pressed'));
      await page.keyboard.press(key);
      await page.waitForTimeout(30);
      const delta = (await sent(page)) - before;
      const after = await page.evaluate(() => document.activeElement.getAttribute('aria-pressed'));
      if (modOnly) check(pressed !== after && delta === 0, `${tag}: ${key} on ${TODAY[i]} did not toggle exactly once (${pressed} -> ${after})`);
      else check(delta === 1, `${tag}: ${key} on ${TODAY[i]} sent ${delta} actions`);
    }
  }
  check(await page.evaluate(() => ['Ctrl', 'Alt', 'Shift'].every((n) => document.querySelector(`button[aria-label="${n}"]`).getAttribute('aria-pressed') === 'false')), `${tag}: modifiers should be back to unpressed after two toggles`);

  // 3. Touch tap = one action; pointerdown is default-prevented; pointerdown + synthesized click = one action.
  await page.evaluate(() => document.activeElement.blur());
  let n0 = await sent(page);
  const ctrlC = page.getByRole('button', { name: 'Control C', exact: true });
  await ctrlC.tap();
  await page.waitForTimeout(700);
  check((await sent(page)) - n0 === 1, `${tag}: a touch tap sent ${(await sent(page)) - n0} actions`);
  n0 = await sent(page);
  const prevented = await ctrlC.evaluate((el) => { const e = new PointerEvent('pointerdown', { bubbles: true, cancelable: true, pointerType: 'touch' }); el.dispatchEvent(e); el.click(); return e.defaultPrevented; });
  await page.waitForTimeout(50);
  check(prevented, `${tag}: pointerdown was not default-prevented, so the phone keyboard could change state`);
  check((await sent(page)) - n0 === 1, `${tag}: pointerdown plus a synthesized click sent ${(await sent(page)) - n0} actions`);

  // 4. The editor: remove, reorder, persist, More, reset, preset.
  await page.getByRole('button', { name: 'Remove Control C', exact: true }).click();
  let after = await names(page);
  check(after.length === 16 && after.at(-1) === 'More keys' && !after.includes('Control C'), `${tag}: after removing ^C the row should have 15 keys and a More key: ${after.join(' | ')}`);
  await page.getByRole('button', { name: 'Move Escape later', exact: true }).click();
  after = await names(page);
  check(after.indexOf('Escape') === 2 && after[1] === 'New line without sending (Alt+Enter)', `${tag}: Escape did not move later: ${after.join(' | ')}`);
  await page.screenshot({ path: join(SHOTS, `keyrow-${tag.toLowerCase()}-390-editor.png`), fullPage: false });
  await page.getByRole('button', { name: 'More keys', exact: true }).tap();
  const inMore = page.getByRole('group', { name: 'More keys' }).getByRole('button', { name: 'Control C', exact: true });
  check(await inMore.isVisible(), `${tag}: the removed ^C is not reachable in More`);
  await page.screenshot({ path: join(SHOTS, `keyrow-${tag.toLowerCase()}-390-more.png`) });
  n0 = await sent(page);
  await inMore.tap();
  await page.waitForTimeout(700);
  check((await sent(page)) - n0 === 1, `${tag}: ^C from More sent ${(await sent(page)) - n0} actions`);
  check(!(await page.getByRole('group', { name: 'More keys' }).isVisible()), `${tag}: More should close after a key`);
  const order = await names(page);
  await page.reload();
  await page.waitForSelector('[role=toolbar]');
  check(JSON.stringify(await names(page)) === JSON.stringify(order), `${tag}: the custom row did not survive a reload`);
  await page.getByRole('button', { name: "Reset to today's row", exact: true }).click();
  check(JSON.stringify(await names(page)) === JSON.stringify(TODAY), `${tag}: Reset did not restore today's row`);
  await page.getByLabel('Preset').selectOption('minimal');
  check((await names(page)).length === 7, `${tag}: the minimal preset should show 6 keys and a More key for the rest`);
  await page.getByLabel('Preset').selectOption('default');
  check((await names(page)).length === 16, `${tag}: the default preset should show 16 keys`);
  check(errors.length === 0, `${tag}: console errors: ${errors.join(' | ')}`);
  await ctx.close();
}
check(layouts.length === 2 && layouts[0] === layouts[1], 'iPhone and Android emulation should have identical key boxes at 390 px');

// 5. Narrow phone, tmux profile: 17 keys do not fit at 320 pt, so the tail goes to More and no key gets narrower than the minimum.
{
  const ctx = await browser.newContext({ ...PHONES['Android: emulated only'], viewport: { width: 320, height: 640 } });
  const page = await ctx.newPage();
  await open(page, '?profile=tmux');
  const bx = await boxes(page);
  check(bx.at(-1)[0] === 'More keys', `320 px tmux: the last key should be More, got ${bx.at(-1)[0]}`);
  for (const [n, , , w, h] of bx) { check(w >= 17.5, `320 px tmux: ${n} is ${w} pt wide`); check(h >= 43.5, `320 px tmux: ${n} is ${h} pt tall`); }
  check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), '320 px tmux: horizontal overflow');
  await page.getByRole('button', { name: 'More keys', exact: true }).tap();
  const many = await page.getByRole('group', { name: 'More keys' }).getByRole('button').count();
  check(many > 3, '320 px tmux: More should list the keys that did not fit');
  await ctx.close();
}

// 6. Desktop at 1024 px with a fine pointer: Auto is compact and gains height; the actions stay; Full restores the row.
{
  const ctx = await browser.newContext({ viewport: { width: 1024, height: 768 } });
  const page = await ctx.newPage();
  await open(page);
  const bar = page.getByRole('toolbar', { name: 'Terminal keys' });
  check((await bar.getAttribute('data-mode')) === 'compact', 'desktop: Auto with a fine pointer should be compact');
  const compactH = (await bar.boundingBox()).height;
  const bn = await names(page);
  for (const want of ['Paste', 'Copy', 'Select', 'Smaller text', 'Larger text', 'Mute bell', 'Dictate']) check(bn.includes(want) || (await bar.getByRole('button', { name: want, exact: true }).count()) > 0, `desktop compact: ${want} is not reachable`);
  check(await bar.getByText('go straight to the terminal').isVisible(), 'desktop compact: the shortcut hint is missing');
  for (const w of ['Paste', 'Copy', 'Select']) { const n0 = await sent(page); await bar.getByRole('button', { name: w, exact: true }).click(); check((await sent(page)) - n0 === 1, `desktop compact: ${w} sent ${(await sent(page)) - n0} actions`); }
  await page.screenshot({ path: join(SHOTS, 'keyrow-desktop-1024-compact.png') });
  await page.getByLabel('On a desktop').selectOption('full');
  check((await bar.getAttribute('data-mode')) === 'full', 'desktop: choosing Full should restore the row (a touch laptop)');
  check((await keys(page).count()) === 16, 'desktop Full: the row should have 16 keys');
  const fullH = (await bar.boundingBox()).height;
  console.log(`desktop 1024 px: compact bar ${compactH} px, full row ${fullH} px, so compact gains ${fullH - compactH} px (${((fullH - compactH) / 17).toFixed(1)} rows at a 17 px line)`);
  check(fullH - compactH >= 17, `desktop: compact gains only ${fullH - compactH} px, under one terminal row`);
  await page.screenshot({ path: join(SHOTS, 'keyrow-desktop-1024-full.png') });
  await page.reload();
  await page.waitForSelector('[role=toolbar]');
  check((await bar.getAttribute('data-mode')) === 'full', 'desktop: the Full choice did not persist');
  await ctx.close();
}

await browser.close();
server.close();
if (problems.length) process.exitCode = 1;
else console.log(`PASS key row: iPhone and Android (emulated only) at 390 px, 320 px tmux overflow, desktop 1024 px; screenshots in ${SHOTS}`);
