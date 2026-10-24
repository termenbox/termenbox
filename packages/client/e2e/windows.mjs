// Tabs that speak for a window, the windows dropdown, Detach vs Close, and the needs-you banner, in real Chrome at
// phone width on the fake-server harness. Usage: node e2e/windows.mjs (after: pnpm run e2e:build)
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
const browser = await launch();
try {
  const ctx = await browser.newContext(contextOptions({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true }));
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  await page.goto(`http://127.0.0.1:${srv.address().port}/navigation.html`);
  await page.getByRole('tablist').waitFor();
  const win = (index, command, path, extra = {}) => ({ index, name: `w${index}`, command, path, active: false, ...extra });
  const feed = (server, session, windows, rep) => page.evaluate(([s, ses, w, r]) => window.__navigation.windows(s, ses, w, r), [server, session, windows, rep]);

  // Two sessions in one repo, one elsewhere: the repo's tabs are told apart by session name.
  await feed('alpha', 'session-0', [win(0, 'bash', '/home/u/app', { active: true }), win(1, 'claude', '/home/u/app/api', { agent: 'claude' })], 1);
  await feed('alpha', 'session-2', [win(0, 'lazygit', '/home/u/app', { active: true })], 0);
  await feed('alpha', 'session-3', [win(0, 'bash', '/srv/other', { active: true })], 0);
  await page.waitForTimeout(2300); // labels follow the shell once no finger is down
  const tab = (key) => page.locator(`[role=tab][data-key="${key}"]`);
  const text = async (key) => (await tab(key).innerText()).replace(/\s+/g, ' ');
  if (!(await text('alpha/session-0')).includes('…/app/api') && !(await text('alpha/session-0')).includes('~/app/api')) fail(`tab 0 does not show its agent window path: ${await text('alpha/session-0')}`);
  if (!(await text('alpha/session-3')).includes('/srv/other')) fail(`tab 3 does not show its path: ${await text('alpha/session-3')}`);
  if ((await tab('alpha/session-0').locator('[role=img]').getAttribute('aria-label')) !== 'Claude Code') fail('tab 0 has no agent icon from its representative window');
  if ((await tab('alpha/session-2').locator('[role=img]').getAttribute('aria-label')) !== 'lazygit') fail('tab 2 has no program icon');
  console.log('ok  a tab shows the path and the icon of its representative window');

  // Same path twice: the session name is added.
  await feed('alpha', 'session-2', [win(0, 'bash', '/home/u/app/api', { active: true })], 0);
  await page.waitForTimeout(2300);
  const t0 = await text('alpha/session-0'), t2 = await text('alpha/session-2');
  if (!t0.includes('session-0') || !t2.includes('session-2')) fail(`equal paths were not told apart: ${t0} | ${t2}`);
  console.log('ok  tabs that would read the same are told apart by session name');

  // Tapping the open tab lists its windows; ✕ asks before closing a running window and closes only that window.
  await page.evaluate(() => window.__navigation.select('alpha/session-0'));
  await tab('alpha/session-0').click();
  const menu = page.getByRole('dialog', { name: 'Windows of session-0' });
  await menu.waitFor();
  const sent = () => page.evaluate(() => window.__navigation.sent().map((m) => m.message));
  if (!(await sent()).some((m) => m.type === 'windows' && m.session === 'session-0')) fail('opening the menu did not ask for the windows');
  await menu.getByText('claude', { exact: true }).waitFor();
  await menu.getByRole('button', { name: 'Close window 1' }).click();
  await menu.getByRole('button', { name: 'Confirm closing window 1' }).waitFor(); // an agent is running: it asks first
  if ((await sent()).some((m) => m.type === 'close_window')) fail('a running window closed without asking');
  await menu.getByRole('button', { name: 'Confirm closing window 1' }).click();
  const closed = (await sent()).filter((m) => m.type === 'close_window');
  if (closed.length !== 1 || closed[0].index !== 1 || closed[0].session !== 'session-0') fail(`wrong close_window: ${JSON.stringify(closed)}`);
  await page.keyboard.press('Escape'); // the menu stays open after a window closes, so several can be closed in a row
  await menu.waitFor({ state: 'hidden' });
  console.log('ok  the windows dropdown lists windows; closing a running one asks first and closes only that window');

  // #107: a focused terminal stays focused through opening the dropdown and closing it by tapping outside or Escape.
  const focusedTerminal = () => page.evaluate(() => document.activeElement?.classList.contains('xterm-helper-textarea'));
  for (const close of ['tap outside', 'Escape']) {
    await page.locator('.xterm-helper-textarea').focus();
    await tab('alpha/session-0').click();
    await menu.waitFor();
    if (close === 'Escape') await page.keyboard.press('Escape');
    else await page.mouse.click(5, 700);
    await menu.waitFor({ state: 'hidden' });
    if (!(await focusedTerminal())) fail(`the terminal lost focus after the windows menu closed by ${close}`);
  }
  console.log('ok  the terminal keeps focus through the windows menu (#107)');

  // + New window asks the server for one in this session and closes the menu.
  await tab('alpha/session-0').click();
  await menu.waitFor();
  await menu.getByRole('button', { name: '+ New window' }).click();
  await menu.waitFor({ state: 'hidden' });
  const opened = (await sent()).filter((m) => m.type === 'new_window');
  if (opened.length !== 1 || opened[0].session !== 'session-0') fail(`wrong new_window: ${JSON.stringify(opened)}`);
  console.log('ok  + New window in the dropdown asks the server for a window of this session');

  // Detach closes the tab and keeps the session; Close session asks, then kills.
  await tab('alpha/session-0').click();
  await menu.waitFor();
  await menu.getByRole('button', { name: 'Detach', exact: true }).click();
  await tab('alpha/session-0').waitFor({ state: 'detached' });
  if ((await sent()).some((m) => m.type === 'close_session')) fail('detach closed the session');
  await tab('alpha/session-2').dispatchEvent('pointerdown', { pointerId: 5, pointerType: 'touch', clientX: 150, clientY: 20 });
  const actions = page.getByRole('dialog', { name: 'Session actions' });
  await actions.waitFor();
  await tab('alpha/session-2').dispatchEvent('pointerup', { pointerId: 5, pointerType: 'touch' });
  await actions.getByRole('button', { name: 'Close session…' }).click();
  await actions.getByRole('button', { name: 'Close session', exact: true }).click();
  const killed = (await sent()).filter((m) => m.type === 'close_session');
  if (killed.length !== 1 || killed[0].session !== 'session-2') fail(`wrong close_session: ${JSON.stringify(killed)}`);
  console.log('ok  Detach closes the tab and keeps the session; Close session confirms, then closes it');

  // The banner shows who needs you, never changes the open session, and offers confirmed answers.
  const active = await page.evaluate(() => window.__navigation.active());
  const banner = page.getByRole('status').filter({ hasText: 'need you' }).or(page.getByRole('status').filter({ hasText: 'needs you' }));
  await banner.first().waitFor();
  if ((await page.evaluate(() => window.__navigation.active())) !== active) fail('the banner changed the open session');
  await banner.first().getByRole('button', { name: 'Yes', exact: true }).click();
  const answer = (await sent()).filter((m) => m.type === 'answer').at(-1);
  if (!answer || answer.choice !== 0) fail(`banner answer not sent: ${JSON.stringify(answer)}`);
  console.log('ok  the needs-you banner answers a confirmed prompt without switching sessions');

  // Chrome budget: the fixed bars above the terminal and the key row, measured at phone width.
  const bars = await page.evaluate(() => {
    const top = document.querySelector('.topbar').getBoundingClientRect().height;
    const keys = document.querySelector('[role=toolbar]').getBoundingClientRect().height;
    return { top, keys, small: [...document.querySelectorAll('.topbar button, [role=toolbar] button')].filter((b) => { const r = b.getBoundingClientRect(); return r.height < 44 && getComputedStyle(b, '::after').content === 'none'; }).length };
  });
  console.log(`    top bar ${bars.top}px + key row ${bars.keys}px = ${bars.top + bars.keys}px of fixed chrome (was ~235px before this change)`);
  if (bars.top + bars.keys > 110) fail(`fixed chrome is ${bars.top + bars.keys}px, over 110px`);
  if (bars.small) fail(`${bars.small} controls under 44px without an extended touch area`);
  if (errors.length) fail(`page errors: ${errors.join('; ')}`);
} catch (e) {
  fail(String(e));
} finally {
  await browser.close();
  srv.close();
  process.exit(process.exitCode ?? 0);
}
