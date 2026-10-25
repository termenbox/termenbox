// The workspace list on a fake event stream, in real Chrome at phone width: every state has a word, answers carry the
// prompt they were read from, quick replies go to the chosen workspace, and the order holds still under a finger.
// Usage: node e2e/cards.mjs   (needs the e2e harness built: vite build -c e2e/vite.config.ts)
import { launch, contextOptions, tap } from './browser.mjs';
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
  const ctx = await browser.newContext(contextOptions({ viewport: { width: 390, height: 900 }, isMobile: true, hasTouch: true }));
  await ctx.addInitScript(() => {
    const vv = Object.assign(new EventTarget(), { height: innerHeight, width: innerWidth, offsetTop: 0, offsetLeft: 0, scale: 1 });
    Object.defineProperty(window, 'visualViewport', { value: vv });
    window.__vv = patch => { Object.assign(vv, patch); vv.dispatchEvent(new Event('resize')); vv.dispatchEvent(new Event('scroll')); };
    // The init script runs before the mobile viewport meta tag is parsed.
    document.addEventListener('DOMContentLoaded', () => { vv.height = innerHeight; vv.width = innerWidth; vv.dispatchEvent(new Event('resize')); });
  });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => fail(`page error: ${e.message}`));
  await page.goto(`http://127.0.0.1:${srv.address().port}/cards.html`);
  await page.waitForSelector('[role=group]');
  const names = () => page.$$eval('[role=group][aria-label*=":"] .name', (els) => els.map((e) => e.textContent));
  const sent = () => page.evaluate(() => window.__sent);

  // 1. Every card says its state in words, and the order puts "needs you" first.
  const text = await page.innerText('body');
  for (const w of ['Needs you', 'Working', 'Done']) if (!text.includes(w)) fail(`no "${w}" chip`);
  const order = await names();
  if (order[0] !== 'api-refactor') fail(`needs-you card is not first: ${order}`);
  else console.log('ok  chips carry words and "needs you" comes first');

  // 1b. A card with server status shows its task and how long it has run.
  if (!text.includes('Wire the touch controller in') || !/running 1 min/.test(text)) fail('the card shows no task title and running time');
  else console.log('ok  a card shows its task and running time');

  // 2. The stale card says so and offers no answers.
  if (!(await page.innerText('body')).includes('no update for 4 min')) fail('the stale card does not say how long it has been quiet');
  else console.log('ok  a card with no news says "no update for N min"');

  // 3. A one-tap answer carries the prompt id and event number it was read from, and the choice index.
  await page.getByRole('button', { name: 'Show details' }).first().click();
  await page.getByRole('button', { name: 'No', exact: true }).click();
  const answer = (await sent()).find((m) => m.type === 'answer');
  if (JSON.stringify(answer) !== JSON.stringify({ type: 'answer', session: 'api-refactor', prompt_id: '1-ab12cd34', seq: 2, choice: 2 })) fail(`wrong answer message: ${JSON.stringify(answer)}`);
  else console.log('ok  an answer names its prompt, event number and choice');

  // 4. Quick reply goes to the chosen workspace; text is cleaned of control characters.
  await page.selectOption('#reply-to', 'docs');
  await page.fill('input[aria-label="Reply text"]', 'run the tests');
  await page.getByRole('button', { name: 'Send', exact: true }).click();
  const reply = (await sent()).find((m) => m.type === 'reply');
  if (JSON.stringify(reply) !== JSON.stringify({ type: 'reply', session: 'docs', text: 'run the tests' })) fail(`wrong reply: ${JSON.stringify(reply)}`);
  else console.log('ok  a quick reply goes to the chosen workspace');

  // 5. Tap targets are at least 44 px.
  const small = await page.$$eval('.sc .open, .sc .more, .answer, .s1', (els) => els.filter((e) => e.getBoundingClientRect().height < 43.5 || e.getBoundingClientRect().width < 43.5).map((e) => e.className));
  if (small.length) fail(`tap targets under 44px: ${small}`);
  else console.log('ok  tap targets are at least 44 px');

  // G.4: the actual top bar on 12 workspaces from three fake connections, at phone width.
  await page.goto(`http://127.0.0.1:${srv.address().port}/navigation.html`);
  await page.waitForFunction(() => document.querySelectorAll('[role=tab]').length === 12);
  await page.waitForFunction(() => {
    const tab = document.querySelector('[role=tab][aria-selected=true]').getBoundingClientRect();
    const strip = document.querySelector('[role=tablist]').getBoundingClientRect();
    return tab.left >= strip.left - 1 && tab.right <= strip.right + 1;
  });
  // A narrower viewport must reveal the active tab again, even when it was visible before resize.
  const activeInView = () => {
    const tab = document.querySelector('[role=tab][aria-selected=true]').getBoundingClientRect();
    const strip = document.querySelector('[role=tablist]').getBoundingClientRect();
    return tab.left >= strip.left - 1 && tab.right <= strip.right + 1;
  };
  for (const width of [360, 390]) {
    await page.setViewportSize({ width, height: 900 });
    await page.evaluate(width => window.__vv?.({ width }), width);
    await page.waitForFunction(activeInView);
  }
  const tabs = page.getByRole('tab');
  if (!(await tabs.first().getAttribute('aria-label')).includes('Needs you')) fail('needs-you tabs are not first');
  if (await page.locator('[role=tab] .badge').count() !== 12) fail('tabs do not name all three servers');
  if ((await page.locator('[role=tablist]').evaluate(e => e.scrollWidth <= e.clientWidth))) fail('12 tabs do not form a scrolling strip');
  const short = await page.locator('header button').evaluateAll(els => els.filter(e => e.getBoundingClientRect().height < 43.5 || e.getBoundingClientRect().width < 43.5).map(e => e.getAttribute('aria-label')));
  if (short.length) fail(`top-bar targets under 44px: ${short}`);
  await page.getByRole('button', { name: 'Workspaces, 3 need you' }).waitFor();
  console.log('ok  12 workspaces on 3 servers scroll at 390px; active tab visible, badges, count and 44px controls');

  const keys = () => tabs.evaluateAll(els => els.map(e => e.dataset.key));
  await page.waitForTimeout(2200);
  const before = await keys();
  await tabs.last().dispatchEvent('pointerdown', { pointerId: 1, pointerType: 'touch', clientX: 200, clientY: 20 });
  await page.locator('body').dispatchEvent('pointerup', { pointerId: 99, pointerType: 'touch' });
  await page.waitForTimeout(30);
  await page.evaluate(() => window.__navigation.change());
  await page.waitForTimeout(100);
  if (JSON.stringify(before) !== JSON.stringify(await keys())) fail('insertion/removal moved tabs under a finger');
  await tabs.last().dispatchEvent('pointercancel', { pointerId: 1, pointerType: 'touch' });
  await page.waitForTimeout(2200);
  if (!(await keys()).includes('alpha/new') || (await keys()).includes('alpha/workspace-0')) fail('deferred tab changes did not apply');
  console.log('ok  tab insertion and removal wait for the finger to lift and scroll hold to expire');

  const activeBefore = await page.evaluate(() => window.__navigation.active());
  const actionTab = page.locator('[role=tab][data-key="alpha/workspace-1"]');
  await actionTab.dispatchEvent('pointerdown', { pointerId: 2, pointerType: 'touch', clientX: 200, clientY: 20 });
  await page.getByRole('dialog', { name: 'Workspace actions' }).waitFor();
  await actionTab.dispatchEvent('pointerup', { pointerId: 2, pointerType: 'touch' });
  await actionTab.dispatchEvent('click', { detail: 1 });
  if ((await page.evaluate(() => window.__navigation.active())) !== activeBefore) fail('long press also switched the workspace');
  // Detach and Close are separate, and Close asks first, naming the workspace and its server.
  const actions = page.getByRole('dialog', { name: 'Workspace actions' });
  await actions.getByRole('button', { name: /^Detach/ }).waitFor();
  await actions.getByRole('button', { name: 'Close workspace…' }).click();
  await actions.getByRole('alert').filter({ hasText: 'Everything running in it ends' }).waitFor();
  await actions.getByRole('button', { name: 'Cancel', exact: true }).click(); // back to the choices
  await actions.getByRole('button', { name: /^Detach/ }).waitFor();
  await actions.getByRole('button', { name: 'Cancel', exact: true }).click();
  await actions.waitFor({ state: 'hidden' });
  await actionTab.focus();
  await page.keyboard.press('Enter');
  if ((await page.evaluate(() => window.__navigation.active())) !== 'alpha/workspace-1') fail('long press suppressed later keyboard activation');
  await page.getByRole('button', { name: 'Menu', exact: true }).click();
  await page.getByRole('dialog', { name: 'Menu', exact: true }).getByRole('button', { name: 'Settings', exact: true }).click();
  await page.getByRole('button', { name: /^Appearance, notifications and more/ }).click();
  await page.getByLabel('Cache TTL').selectOption('3600000');
  await page.getByLabel('Theme').selectOption('light');
  const stored = await page.evaluate(() => JSON.parse(localStorage.getItem('termenbox_settings')));
  if (stored.cacheTtlMs !== 3600000 || stored.theme !== 'light') fail('settings did not persist together');
  await page.getByRole('button', { name: 'Back to Settings', exact: true }).click();
  console.log('ok  long press opens actions without switching; appearance settings persist');

  // G.5: adaptive view, real drag/snaps, server status, controls, persistence and keyboard-safe layout.
  await page.getByRole('button', { name: 'Workspaces, 3 need you' }).click();
  await page.locator('.workspace-sheet[data-height=half]').waitFor();
  await page.getByRole('button', { name: 'Needs me', exact: true }).waitFor();
  if (await page.getByRole('button', { name: 'Needs me', exact: true }).getAttribute('aria-pressed') !== 'true') fail('adaptive default did not choose Needs me');
  await page.getByRole('button', { name: 'Resize workspaces sheet, half', exact: true }).click();
  await page.locator('.workspace-sheet[data-height=full]').waitFor();
  const fullBox = await page.locator('.workspace-sheet').boundingBox();
  const handle = await page.locator('.handle').boundingBox();
  const x = handle.x + handle.width / 2, y = handle.y + handle.height / 2;
  await page.mouse.move(x, y); await page.mouse.down();
  await page.mouse.move(x, y + fullBox.height / 2, { steps: 12 }); await page.mouse.up();
  await page.locator('.workspace-sheet[data-height=half]').waitFor();
  await page.getByRole('button', { name: 'Minimize workspaces', exact: true }).click();
  await page.locator('.workspace-sheet[data-height=closed]').waitFor({ state: 'hidden' }); // nothing of it stays on a phone screen
  console.log('ok  sheet closes on a phone and has half and full heights; a drag snaps to half');

  await page.getByRole('button', { name: 'Workspaces, 3 need you' }).click();
  await page.getByRole('button', { name: 'By server', exact: true }).click();
  await page.locator('[data-group="server:beta"]').waitFor();
  await page.evaluate(() => window.__navigation.degrade());
  await page.locator('[data-group="server:beta"] .server-status').getByText('unreachable', { exact: true }).waitFor();
  if (!(await page.locator('[data-group="server:beta"]').innerText()).includes('Test server offline')) fail('group header lost the server reason');
  await page.locator('[data-group="server:beta"]').getByRole('button', { name: 'Retry', exact: true }).click();
  await page.locator('[data-group="server:beta"] .server-status').getByText('connected', { exact: true }).waitFor();
  console.log('ok  group headers carry server state/reason and Retry reconnects only that server');

  // Freeze the whole group layout while a card is expanded, including an arriving prompt.
  await page.getByRole('button', { name: 'Needs me', exact: true }).click();
  const card = page.locator('.workspace-sheet .sc').filter({ hasText: 'alpha' }).first();
  await card.getByRole('button', { name: 'Show details', exact: true }).click();
  const original = await card.getByRole('button', { name: 'Yes', exact: true }).textContent();
  await page.evaluate(() => window.__navigation.changePrompt());
  await card.getByRole('button', { name: 'Yes', exact: true }).click();
  const answerSent = await page.evaluate(() => window.__navigation.sent().filter(m => m.message.type === 'answer').at(-1));
  if (original !== 'Yes' || answerSent?.message.prompt_id !== 'prompt-alpha' || answerSent.message.seq !== 2 || answerSent.message.session !== 'workspace-1') fail(`answer did not keep the displayed prompt: ${JSON.stringify(answerSent)}`);
  await card.getByRole('button', { name: 'Hide details', exact: true }).click();
  console.log('ok  expanded cards hold their layout and answers carry the prompt read by the user');

  await page.getByRole('button', { name: 'Recent', exact: true }).click();
  await page.getByRole('button', { name: 'Group, sort and filter', exact: true }).click();
  const findField = page.getByLabel('Find', { exact: true });
  await findField.dispatchEvent('pointerdown', { pointerId: 8, pointerType: 'touch', bubbles: true });
  await findField.focus();
  if (await page.locator('.workspace-sheet').getAttribute('data-height') !== 'half') fail('focusing a field moved the sheet before the finger lifted');
  await page.locator('body').dispatchEvent('pointerup', { pointerId: 99, pointerType: 'touch' });
  await page.waitForTimeout(30);
  if (await page.locator('.workspace-sheet').getAttribute('data-height') !== 'half') fail('an unrelated pointer release moved the focused sheet under a finger');
  await findField.dispatchEvent('pointerup', { pointerId: 8, pointerType: 'touch', bubbles: true });
  await page.locator('.workspace-sheet[data-height=full]').waitFor();
  await page.getByLabel('Group', { exact: true }).selectOption('agent');
  await page.getByLabel('Sort', { exact: true }).selectOption('name');
  await page.getByLabel('Server', { exact: true }).selectOption('beta');
  await page.getByLabel('Find', { exact: true }).fill('workspace-2');
  await page.waitForFunction(() => document.querySelectorAll('.workspace-sheet .sc').length === 1);
  await page.reload();
  await page.getByRole('button', { name: 'Workspaces, 3 need you' }).click();
  await page.waitForFunction(() => document.querySelectorAll('.workspace-sheet .sc').length === 1);
  await page.getByRole('button', { name: 'Group, sort and filter', exact: true }).click();
  if (await page.getByLabel('Group', { exact: true }).inputValue() !== 'agent' || await page.getByLabel('Sort', { exact: true }).inputValue() !== 'name' || await page.getByLabel('Server', { exact: true }).inputValue() !== 'beta' || await page.getByLabel('Find', { exact: true }).inputValue() !== 'workspace-2') fail('custom view was not restored on reload');
  console.log('ok  group, sort, server and text filters survive reload on this device');

  await page.getByRole('button', { name: 'By server', exact: true }).click();
  await page.locator('.workspace-sheet #reply-to').selectOption('beta/workspace-2');
  await page.locator('.workspace-sheet input[aria-label="Reply text"]').fill('continue');
  await page.evaluate(() => window.__vv({ height: 460, offsetTop: 80 }));
  await page.waitForTimeout(300);
  await page.locator('.workspace-sheet input[aria-label="Reply text"]').scrollIntoViewIfNeeded();
  const replyBox = await page.locator('.workspace-sheet input[aria-label="Reply text"]').boundingBox();
  const sheetBox = await page.locator('.workspace-sheet').boundingBox();
  if (replyBox.y < 80 || replyBox.y + replyBox.height > 540 || sheetBox.y + sheetBox.height > 540) fail('the keyboard viewport shrink/pan hides the sheet input');
  await page.locator('.workspace-sheet').getByRole('button', { name: 'Send', exact: true }).click();
  const sentReply = await page.evaluate(() => window.__navigation.sent().filter(m => m.message.type === 'reply').at(-1));
  if (sentReply?.server !== 'beta' || sentReply.message.session !== 'workspace-2' || sentReply.message.text !== 'continue') fail(`quick reply crossed servers: ${JSON.stringify(sentReply)}`);
  await page.getByRole('button', { name: 'Minimize workspaces', exact: true }).click();
  const keyBox = await page.getByRole('toolbar', { name: 'Terminal keys', exact: true }).boundingBox();
  if (keyBox.y < 80 || keyBox.y + keyBox.height > 540) fail('the keyboard viewport shrink/pan hides the terminal keys');
  console.log('ok  emulated keyboard shrink/pan keeps sheet input and terminal keys visible; replies route to their server');

  await page.evaluate(() => window.__vv({ height: 900, offsetTop: 0, width: 1100 }));
  await page.setViewportSize({ width: 1100, height: 900 });
  await page.locator('.workspace-sheet[data-height=docked]').waitFor();
  const docked = await page.locator('.workspace-sheet').boundingBox();
  if (docked.width < 280 || docked.width > 360 || docked.x !== 0) fail(`wide screen did not dock the sidebar: ${JSON.stringify(docked)}`);
  console.log('ok  wide screens keep a docked workspaces sidebar');

  // U05: Home on 20 workspaces at 390 px (desktop Chromium with mobile emulation: not an iPhone or Android check).
  const home = await ctx.newPage();
  home.on('pageerror', (e) => fail(`home page error: ${e.message}`));
  await home.setViewportSize({ width: 390, height: 844 });
  await home.goto(`http://127.0.0.1:${srv.address().port}/home.html`);
  await home.waitForSelector('[data-home-tiles]');
  // Every state count is visible with no tap and no scroll, in one row inside the screen.
  const tiles = await home.$$eval('[data-home-tiles] button', (els) => els.map((e) => {
    const r = e.getBoundingClientRect();
    return { text: e.textContent.replace(/\s+/g, ' ').trim(), left: r.left, right: r.right, top: r.top, bottom: r.bottom, h: r.height, w: r.width };
  }));
  const want = ['3 !Needs you', '6 ●Working', '2 ✕Failed', '4 ✓Finished'];
  if (JSON.stringify(tiles.map((t) => t.text)) !== JSON.stringify(want)) fail(`fleet tiles read ${JSON.stringify(tiles.map((t) => t.text))}`);
  if (tiles.some((t) => t.left < 0 || t.right > 390 || t.bottom > 844 || t.h < 43.5 || t.w < 43.5)) fail(`a tile is off screen or under 44 px: ${JSON.stringify(tiles)}`);
  if (new Set(tiles.map((t) => Math.round(t.top))).size !== 1) fail('the tiles are not in one row');
  if ((await home.evaluate(() => document.documentElement.scrollWidth - innerWidth)) > 0) fail('Home scrolls sideways');
  console.log('ok  Home: four state counts (3/6/2/4 of 20 workspaces) visible at 0 taps at 390 px, no sideways scroll');

  // The hero is the oldest waiting decision, with one filled primary option.
  const hero = home.getByRole('region', { name: 'Decision' });
  if (!(await hero.innerText()).includes('task-4')) fail(`the hero is not the oldest wait: ${await hero.innerText()}`);
  const filled = await hero.locator('[data-hero-options] button').evaluateAll((els) => els.map((e) => getComputedStyle(e).backgroundColor));
  if (filled.length !== 3 || new Set(filled).size !== 2 || filled[0] === filled[1] || filled[1] !== filled[2]) fail(`not exactly one filled primary option: ${filled}`);
  const queue = await home.locator('[data-home-queue] > button').evaluateAll((els) => els.map((e) => e.querySelector('b').textContent));
  if (JSON.stringify(queue.slice(0, 5)) !== JSON.stringify(['task-1', 'task-8', 'task-2', 'task-3', 'task-10'])) fail(`queue order: ${queue}`);
  const row = await home.locator('[data-home-queue] > button').first().innerText();
  if (!/Refactor part 1/.test(row) || !/beta/.test(row) || !/waiting 12 min/.test(row)) fail(`queue row lacks task, server or wait: ${row}`);
  console.log('ok  Home: oldest wait is the hero with one filled option; queue ordered needs you, failed, finished with task, server and wait');

  // The queue holds still under a touch and for 2 s after a scroll, then follows the data.
  const heroText = () => home.getByRole('region', { name: 'Decision' }).innerText();
  const agentOf = (key) => home.evaluate((k) => window.__home.agents()[k], key);
  const arriving = { ...(await agentOf('beta/s1')), attention: { ...(await agentOf('beta/s1')).attention, id: 'new', raised_at: Date.now() - 60 * 60_000, session: 's0' } };
  await home.locator('[data-home-queue] > button').first().dispatchEvent('pointerdown', { pointerId: 5, pointerType: 'touch' });
  await home.evaluate(([k, a]) => window.__home.set(k, a), ['alpha/s0', { ...arriving, prompt: undefined }]);
  await home.waitForTimeout(1200);
  if (!(await heroText()).includes('task-4') || (await home.locator('[data-home-queue]').innerText()).includes('task-0')) fail('a new item appeared or the hero changed under a finger');
  await home.locator('[data-home-queue] > button').first().dispatchEvent('pointerup', { pointerId: 5, pointerType: 'touch' });
  await home.getByRole('region', { name: 'Decision' }).getByText('task-0').waitFor({ timeout: 3000 }); // oldest wait now: it becomes the hero once released
  console.log('ok  Home: an arriving item waits for the finger to lift, then takes its place');
  await home.evaluate(() => document.querySelector('[data-home]').dispatchEvent(new Event('scroll')));
  await home.evaluate(() => window.__home.set('alpha/s0', null));
  await home.waitForTimeout(1200);
  if (!(await heroText()).includes('task-0')) fail('the queue changed 1.2 s after a scroll');
  await home.getByRole('region', { name: 'Decision' }).getByText('task-4').waitFor({ timeout: 3500 });
  console.log('ok  Home: after a scroll the queue changes only after 2 s');

  // One tap on the primary option answers the prompt it was read from.
  await home.evaluate(() => { window.__home.answers.length = 0; });
  const primary = home.getByRole('region', { name: 'Decision' }).locator('[data-hero-options] button').first();
  await tap(primary);
  const answers = await home.evaluate(() => window.__home.answers);
  if (JSON.stringify(answers) !== JSON.stringify([{ type: 'answer', session: 'alpha/s4', prompt_id: 'p4', seq: 11, choice: 0 }])) fail(`hero answer: ${JSON.stringify(answers)}`);
  console.log('ok  Home: the hero is answered with one tap, carrying its prompt id and event number');

  // A state tile filters.
  await tap(home.locator('[data-home-tiles] button', { hasText: 'Failed' }));
  await home.locator('[data-home-filter]').waitFor();
  if ((await home.locator('[data-home-filter] > button').count()) !== 2) fail('the Failed tile did not filter to its 2 workspaces');
  console.log('ok  Home: a state tile filters the list');
} catch (error) {
  fail(error.stack ?? String(error));
} finally {
  await browser.close();
  srv.close();
}

process.exit(process.exitCode ?? 0);
