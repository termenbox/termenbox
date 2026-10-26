// The user's own order of workspaces and the rename sheet, in real Chrome at phone width (390 px, touch): the order stays where the
// user put it whatever the agents do, a handle drags a row with a finger while the rest of the list still scrolls, the list
// scrolls itself near its edges, Move up / Move down work without dragging, the order survives a reload, and a workspace is
// renamed with the soft keyboard up (field in view, 16 px text, 44 px targets, Enter and Save commit, Escape and Cancel do not).
// Desktop Chromium emulating a phone: not an iPhone or an Android phone, and its keyboard is a smaller window.
// Usage: node e2e/reorder.mjs   (needs the e2e harness built: vite build -c e2e/vite.config.ts)
import { launch, contextOptions, tap, ENGINE } from './browser.mjs';
import { createServer } from 'node:http';
import { readFileSync, existsSync } from 'node:fs';
import { join, extname } from 'node:path';

const dist = join(import.meta.dirname, 'dist');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const fail = (m) => { console.error(`FAIL: ${m}`); process.exitCode = 1; };
const ok = (m) => console.log(`ok  ${m}`);
const check = (c, m) => { if (!c) fail(m); return !!c; };
const srv = createServer((req, res) => {
  const f = join(dist, req.url.split('?')[0]);
  if (!existsSync(f)) return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
});
await new Promise((r) => srv.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${srv.address().port}`;
const browser = await launch();
try {
  if (ENGINE !== 'chromium') { console.log(`ok  skipped on ${ENGINE}: dragging with a finger needs CDP touch events; Chromium covers it`); process.exit(0); }
  const ctx = await browser.newContext(contextOptions({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true }));
  const page = await ctx.newPage();
  page.on('pageerror', (e) => fail(`page error: ${e.message}`));
  const cdp = await ctx.newCDPSession(page);
  const touch = (type, x, y) => cdp.send('Input.dispatchTouchEvent', { type, touchPoints: type === 'touchEnd' || type === 'touchCancel' ? [] : [{ x: Math.round(x), y: Math.round(y), id: 1 }] });
  const centre = async (loc) => { const b = await loc.boundingBox(); return { ...b, x: b.x + b.width / 2, y: b.y + b.height / 2 }; };
  /** A finger down on `from`, moving to `to` in steps, resting there `rest` ms; `during` runs before it lifts (or is cancelled). */
  async function drag(from, to, { steps = 14, rest = 0, during, cancel = false } = {}) {
    await touch('touchStart', from.x, from.y);
    for (let i = 1; i <= steps; i++) { await touch('touchMove', from.x + (to.x - from.x) * i / steps, from.y + (to.y - from.y) * i / steps); await page.waitForTimeout(16); }
    if (rest) await page.waitForTimeout(rest);
    if (during) await during();
    await touch(cancel ? 'touchCancel' : 'touchEnd');
    await page.waitForTimeout(60);
  }
  const order = (prefix) => page.evaluate((p) => window.__reorder.order().filter((n) => n.startsWith(p)), prefix);
  const row = (name) => page.locator('[data-row]').filter({ has: page.locator('b', { hasText: new RegExp(`^${name}$`) }) });
  const grip = (name) => row(name).locator('[data-testid=workspace-grip]');
  /** Back to the top, and still: a finger's momentum would otherwise carry on scrolling after it. */
  const toTop = async () => { await page.waitForTimeout(700); await page.evaluate(() => document.querySelector('[data-workspace-groups]').scrollTo(0, 0)); await page.waitForTimeout(150); };
  const scrollTop = () => page.evaluate(() => document.querySelector('[data-workspace-groups]').scrollTop);
  const status = () => page.locator('[role=status]').first().innerText();
  const alpha = (...ids) => ids.map((i) => `alpha-${i}`);

  // ---- 1. The Workspaces list, as the phone shows it with nothing open. ----
  await page.goto(`${base}/reorder.html`);
  await page.waitForSelector('[data-row]');
  check((await order('alpha')).join() === alpha(0, 1, 2, 3, 4, 5, 6, 7, 8, 9).join(), `the list is not in the order given: ${await order('alpha')}`);
  check((await row('alpha-3').innerText()).includes('Needs you') && (await row('alpha-5').innerText()).includes('Working'), 'a row does not say what its agent is doing');
  const rowsNow = await page.$$eval('[data-row] b', (els) => els.map((e) => e.textContent));
  check(rowsNow.indexOf('alpha-3') === 3, `a workspace that needs you jumped: ${rowsNow}`);
  ok('the list keeps the order given; a row that needs you or is working stays put and says so in words');

  const grips = await page.$$eval('[data-testid=workspace-grip]', (els) => els.map((e) => { const r = e.getBoundingClientRect(); return [r.width, r.height, getComputedStyle(e).touchAction]; }));
  check(grips.length === 12 && grips.every(([w, h]) => w >= 43.5 && h >= 43.5), `a handle is under 44 px: ${JSON.stringify(grips.filter(([w, h]) => w < 43.5 || h < 43.5))}`);
  check(grips.every(([, , ta]) => ta === 'none'), 'a handle does not take the touch for itself (touch-action: none)');
  const elsewhere = await page.$$eval('[data-row] > :not([data-testid=workspace-grip])', (els) => els.map((e) => getComputedStyle(e).touchAction));
  check(elsewhere.length > 0 && elsewhere.every((ta) => ta !== 'none'), `something besides the handle blocks scrolling: ${elsewhere}`);
  const small = await page.$$eval('[data-row] button', (els) => els.filter((e) => e.getBoundingClientRect().width < 43.5 || e.getBoundingClientRect().height < 43.5).map((e) => e.getAttribute('aria-label') || e.textContent.trim().slice(0, 20)));
  check(small.length === 0, `row targets under 44 px: ${small}`);
  check((await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)) <= 0, 'the list overflows sideways');
  ok('every handle and action is 44 px; only the handle has touch-action: none');

  // A finger on the card (not the handle) scrolls the list.
  const card = await centre(row('alpha-6').locator('button').nth(1));
  await drag({ x: card.x, y: card.y }, { x: card.x, y: card.y - 220 }, { steps: 10 });
  check((await scrollTop()) > 40, `the list does not scroll under a finger on a card (scrollTop ${await scrollTop()})`);
  check((await order('alpha')).join() === alpha(0, 1, 2, 3, 4, 5, 6, 7, 8, 9).join(), 'scrolling reordered the list');
  await toTop();
  ok('a finger on a card still scrolls the list and moves nothing');

  // ---- 2. Dragging by the handle. ----
  const from = await centre(grip('alpha-4'));
  const target = await row('alpha-1').boundingBox();
  await drag({ x: from.x, y: from.y }, { x: from.x, y: target.y + 8 }, {
    during: async () => {
      check(await row('alpha-4').evaluate((e) => e.classList.contains('dragging') && getComputedStyle(e).transform !== 'none'), 'the dragged row does not follow the finger');
      check(await row('alpha-1').evaluate((e) => e.classList.contains('drop-before')), 'no line shows where the row will land');
      check((await order('alpha')).join() === alpha(0, 1, 2, 3, 4, 5, 6, 7, 8, 9).join(), 'the list changed before the finger lifted');
    },
  });
  check((await order('alpha')).join() === alpha(0, 4, 1, 2, 3, 5, 6, 7, 8, 9).join(), `the drop did not land: ${await order('alpha')}`);
  check((await order('beta')).join() === 'beta-0,beta-1', 'another server moved');
  check((await status()) === 'alpha-4 moved to position 2 of 10', `not announced: "${await status()}"`);
  check((await page.evaluate(() => JSON.parse(localStorage.getItem('termenbox_workspace_order:alpha')).length)) === 10, 'the order was not kept on the device');
  check((await page.evaluate(() => scrollY)) === 0, 'the page panned during the drag');
  ok('dragging a handle with a finger moves the row, shows where it lands, announces it and keeps it');

  // The list scrolls by itself near its edge, and a row only lands among its own server's.
  await toTop();
  const top = await centre(grip('alpha-0'));
  let scrolled = 0;
  await drag({ x: top.x, y: top.y }, { x: top.x, y: 838 }, { steps: 20, rest: 900, during: async () => { scrolled = await scrollTop(); } });
  check(scrolled > 150, `the list did not scroll near the edge (${scrolled})`);
  check((await order('alpha')).join() === alpha(4, 1, 2, 3, 5, 6, 7, 8, 9, 0).join(), `a drop below the server did not land at its end: ${await order('alpha')}`);
  check((await order('beta')).join() === 'beta-0,beta-1', 'a row crossed to another server');
  ok('holding a row at the bottom edge scrolls the list; a row lands only among its own server\'s');
  await toTop();

  // A cancelled touch (a system gesture) puts everything back.
  const c = await centre(grip('alpha-2'));
  await drag({ x: c.x, y: c.y }, { x: c.x, y: c.y + 200 }, { cancel: true });
  check((await order('alpha')).join() === alpha(4, 1, 2, 3, 5, 6, 7, 8, 9, 0).join(), 'a cancelled drag moved a row');
  check(await page.locator('.dragging').count() === 0, 'a cancelled drag leaves a row lifted');
  ok('a cancelled touch leaves the order as it was');

  // The arrow keys on a focused handle move one place, for a keyboard or a switch.
  await grip('alpha-4').focus();
  await page.keyboard.press('ArrowDown');
  check((await order('alpha')).join() === alpha(1, 4, 2, 3, 5, 6, 7, 8, 9, 0).join(), `ArrowDown did not move the row: ${await order('alpha')}`);
  check((await status()) === 'alpha-4 moved to position 2 of 10', `the key move was not announced: "${await status()}"`);
  check(await grip('alpha-4').evaluate((e) => document.activeElement === e), 'the handle lost focus after a move');
  await page.keyboard.press('ArrowUp');
  check((await order('alpha')).join() === alpha(4, 1, 2, 3, 5, 6, 7, 8, 9, 0).join(), `ArrowUp did not move the row back: ${await order('alpha')}`);
  ok('the arrow keys on a handle move a row one place, announced, with focus kept');

  // ---- 3. The order survives a reload. ----
  await page.reload();
  await page.waitForSelector('[data-row]');
  check((await order('alpha')).join() === alpha(4, 1, 2, 3, 5, 6, 7, 8, 9, 0).join(), `the order did not survive a reload: ${await order('alpha')}`);
  ok('the order is kept per device across a reload');

  // ---- 4. Move up / Move down in the actions sheet, no dragging. ----
  await toTop();
  await tap(row('alpha-0').locator('[data-testid=workspace-actions]'));
  const dialog = page.getByRole('dialog', { name: 'Workspace actions' });
  await dialog.waitFor();
  const pos = dialog.locator('[data-testid=workspace-position]');
  check((await pos.innerText()) === 'Position 10 of 10 on alpha', `wrong position: ${await pos.innerText()}`);
  check(await dialog.getByRole('button', { name: 'Move down' }).isDisabled() && !(await dialog.getByRole('button', { name: 'Move up' }).isDisabled()), 'the buttons do not know the workspace is last');
  await tap(dialog.getByRole('button', { name: 'Move up' }));
  check((await pos.innerText()) === 'Position 9 of 10 on alpha', 'Move up did not move');
  check((await dialog.locator('[role=status]').innerText()) === 'alpha-0 moved to position 9 of 10', 'the move was not announced');
  await tap(dialog.getByRole('button', { name: 'To top' }));
  check((await pos.innerText()) === 'Position 1 of 10 on alpha' && await dialog.getByRole('button', { name: 'Move up' }).isDisabled(), 'To top did not move to the top');
  await tap(dialog.getByRole('button', { name: 'Move down' }));
  check((await order('alpha')).join() === alpha(4, 0, 1, 2, 3, 5, 6, 7, 8, 9).join(), `the list did not follow the buttons: ${await order('alpha')}`);
  const bad = await dialog.locator('button').evaluateAll((els) => els.filter((e) => e.getBoundingClientRect().width < 43.5 || e.getBoundingClientRect().height < 43.5).map((e) => e.textContent.trim()));
  check(bad.length === 0, `actions targets under 44 px: ${bad}`);
  const box = await dialog.boundingBox();
  check(box.y >= 0 && box.y + box.height <= 844 && box.x >= 0 && box.x + box.width <= 390, 'the actions sheet does not fit the screen');
  await tap(dialog.getByRole('button', { name: 'Cancel', exact: true }));
  await dialog.waitFor({ state: 'hidden' });
  ok('Move up, Move down and To top work without dragging, announce themselves, keep the sheet open, and fit at 390 px');

  // ---- 5. Rename with the soft keyboard up. ----
  const renames = () => page.evaluate(() => window.__reorder.sent().filter((m) => m.type === 'rename_workspace'));
  const open = async (name) => { await tap(row(name).locator('[data-testid=workspace-actions]')); await dialog.waitFor(); await tap(dialog.getByRole('button', { name: 'Rename…' })); return dialog.getByLabel('Workspace name'); };
  let input = await open('alpha-2');
  const focus = await input.evaluate((e) => [document.activeElement === e, e.selectionStart, e.selectionEnd, e.value.length, document.activeElement.tagName]);
  check(focus[0] && focus[1] === 0 && focus[2] === focus[3], `the field is not focused with the name selected: ${focus}`);
  const style = await input.evaluate((e) => { const s = getComputedStyle(e); return [parseFloat(s.fontSize), e.getBoundingClientRect().height]; });
  check(style[0] >= 16, `text under 16 px makes iOS zoom in: ${style[0]}`);
  check(style[1] >= 43.5, `the field is under 44 px high: ${style[1]}`);
  const buttons = await dialog.locator('button').evaluateAll((els) => els.map((e) => [e.textContent.trim(), e.getBoundingClientRect().height]));
  check(buttons.every(([, h]) => h >= 43.5), `rename buttons under 44 px: ${JSON.stringify(buttons)}`);
  check((await dialog.boundingBox()).y <= 40, 'the rename sheet is not pinned to the top, where the keyboard leaves room');
  // The keyboard takes the lower half of the screen (interactive-widget=resizes-content shrinks the layout viewport).
  await page.setViewportSize({ width: 390, height: 400 });
  await page.waitForTimeout(120);
  const field = await input.boundingBox();
  const sheet = await dialog.boundingBox();
  check(field.y >= 0 && field.y + field.height <= 400, `the field is hidden behind the keyboard (${Math.round(field.y)}..${Math.round(field.y + field.height)} of 400)`);
  check(sheet.y >= 0 && sheet.y + sheet.height <= 400 || (await dialog.evaluate((e) => e.scrollHeight > e.clientHeight)), 'the sheet neither fits above the keyboard nor scrolls');
  await page.keyboard.press('Control+A');
  await page.keyboard.type('build server');
  await page.keyboard.press('Enter');
  await dialog.waitFor({ state: 'hidden' });
  const sent = await renames();
  check(sent.length === 1 && sent[0].workspace === 'w2' && sent[0].name === 'build server', `Enter did not send the new name: ${JSON.stringify(sent)}`);
  check((await page.evaluate(() => window.__reorder.order())).includes('build server'), 'the list does not show the new name');
  ok('rename with the keyboard up: field in view, 16 px text, 44 px targets; Enter saves the trimmed name and closes');
  await page.setViewportSize({ width: 390, height: 844 });

  // Empty keeps the field; Escape and Cancel do not save; Save does.
  input = await open('alpha-3');
  await page.keyboard.press('Control+A'); await page.keyboard.press('Backspace'); await page.keyboard.press('Enter');
  check(await input.isVisible() && (await renames()).length === 1, 'an empty name was accepted or closed the field');
  check(await dialog.getByRole('button', { name: 'Save name' }).isDisabled(), 'Save is enabled with an empty name');
  await page.keyboard.type('nope');
  await page.keyboard.press('Escape');
  await dialog.waitFor({ state: 'hidden' });
  check((await renames()).length === 1, 'Escape saved the name');
  input = await open('alpha-3');
  await page.keyboard.type('x');
  await tap(dialog.getByRole('button', { name: 'Cancel', exact: true }));
  check(await dialog.getByRole('button', { name: 'Rename…' }).isVisible() && (await renames()).length === 1, 'Cancel saved the name or did not go back');
  await tap(dialog.getByRole('button', { name: 'Rename…' }));
  await page.keyboard.press('Control+A');
  await page.keyboard.type('docs');
  await tap(dialog.getByRole('button', { name: 'Save name' }));
  await dialog.waitFor({ state: 'hidden' });
  const all = await renames();
  check(all.length === 2 && all[1].name === 'docs', `Save did not send the name: ${JSON.stringify(all)}`);
  ok('an empty name keeps the field open; Escape and Cancel keep the old name; Save by tap commits');

  // ---- 6. The workspace sheet and the tab strip follow the same order, and "needs you" is still counted. ----
  await page.goto(`${base}/navigation.html`);
  await page.waitForFunction(() => document.querySelectorAll('[role=tab]').length === 12);
  const keys = () => page.$$eval('[role=tab]', (els) => els.map((e) => e.dataset.key));
  check((await keys()).slice(0, 4).join() === 'alpha/workspace-0,alpha/workspace-1,alpha/workspace-2,alpha/workspace-3', `the tab strip puts something before the order given: ${(await keys()).slice(0, 4)}`);
  await page.getByRole('button', { name: 'Workspaces, 3 need you' }).click();
  await page.getByRole('button', { name: 'By server', exact: true }).click();
  const sheetRow = (key) => page.locator(`[data-row="${key}"]`);
  for (let i = 0; i < 3 && (await page.locator('section.workspace-sheet').getAttribute('data-height')) !== 'full'; i++) await page.getByRole('button', { name: /^Resize workspaces sheet/ }).click();
  await sheetRow('alpha/workspace-3').waitFor();
  check(await page.locator('[data-testid=workspace-grip]').count() === 12, 'the sheet has no handle on each row');
  const g = await centre(sheetRow('alpha/workspace-3').locator('[data-testid=workspace-grip]'));
  const t0 = await sheetRow('alpha/workspace-0').boundingBox();
  await drag({ x: g.x, y: g.y }, { x: g.x, y: t0.y + 6 });
  await page.waitForFunction(() => document.querySelector('[role=tab]').dataset.key === 'alpha/workspace-3', null, { timeout: 5000 }).catch(() => {});
  check((await keys()).slice(0, 4).join() === 'alpha/workspace-3,alpha/workspace-0,alpha/workspace-1,alpha/workspace-2', `the tab strip does not follow the sheet: ${(await keys()).slice(0, 4)}`);
  const inSheet = await page.$$eval('[data-row][data-list=alpha]', (els) => els.map((e) => e.dataset.row));
  check(inSheet.join() === 'alpha/workspace-3,alpha/workspace-0,alpha/workspace-1,alpha/workspace-2', `the sheet did not reorder: ${inSheet}`);
  await page.getByRole('button', { name: 'Workspaces, 3 need you' }).waitFor();
  check((await page.getByRole('tab').nth(2).getAttribute('aria-label')).includes('Needs you'), 'the needs-you tab does not say so');
  ok('the sheet drags too; the tab strip follows; the count and the tab still say who needs you');

  // The sheet has a way to the actions without a long press.
  await sheetRow('alpha/workspace-0').getByRole('button', { name: 'Show details' }).click();
  await tap(sheetRow('alpha/workspace-0').getByRole('button', { name: 'Rename, move or detach…' }));
  const sheetDialog = page.getByRole('dialog', { name: 'Workspace actions' });
  await sheetDialog.waitFor();
  check((await sheetDialog.locator('[data-testid=workspace-position]').innerText()) === 'Position 2 of 4 on alpha', 'the sheet\'s actions do not know the position');
  await tap(sheetDialog.getByRole('button', { name: 'Move down' }));
  await tap(sheetDialog.getByRole('button', { name: 'Cancel', exact: true }));
  await sheetDialog.waitFor({ state: 'hidden' });
  const afterButtons = await page.$$eval('[data-row][data-list=alpha]', (els) => els.map((e) => e.dataset.row));
  check(afterButtons.join() === 'alpha/workspace-3,alpha/workspace-1,alpha/workspace-0,alpha/workspace-2', `Move down in the sheet's actions did not show at once: ${afterButtons}`);
  ok('the card\'s details lead to the actions: rename, move, detach');
} catch (e) {
  fail(String(e));
} finally {
  await browser.close();
  srv.close();
}
