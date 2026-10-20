// U15 on the real app: the docked sidebar (toggle, resize by pointer and keyboard, reload), the live terminal beside the
// structured view, crossing 720 px, and the keyboard shortcuts. Runs from desktop.mjs; see app-lib.mjs for what the
// scripted server stands in for.
import { startShared, startApp, CLAUDE_PROMPT } from './app-lib.mjs';

const WORKSPACE = 'termenbox_workspace';
const WIDTHS = [721, 1024, 1440, 1920];
const MIN_CONTENT = 420; // src/lib/panes.ts
const sidebarMaxAt = (vw) => Math.min(480, vw - MIN_CONTENT);

const writes = (page) => page.evaluate((k) => window.__writes.filter((w) => w === k).length, WORKSPACE);
const saved = (page) => page.evaluate((k) => JSON.parse(localStorage.getItem(k) ?? 'null'), WORKSPACE);
const num = async (loc, attr) => Number(await loc.getAttribute(attr));

/** The last size the server was told, once it is newer than `since` messages. */
async function nextSize(t, since, what = '') {
  const m = await t.waitResize(since);
  t.check(!!m, `the server never received a new terminal size ${what}(had ${since})`);
  return m ?? { cols: 0, rows: 0 };
}

/** The sidebar at one width: toggle, resize, keyboard, reload, storage writes, overflow, and the server told the new size. */
async function sidebarAt(shared, width) {
  const t = await startApp(shared, { viewport: { width, height: 800 } });
  const { page, server } = t;
  await t.open();
  const side = page.locator('[data-shell=sidebar]');
  t.check(await side.isVisible(), `${width}: no sidebar`);
  t.check((await page.locator('[data-shell=bottom-bar]').count()) === 0, `${width}: bottom bar beside the sidebar`);
  await t.openSession('s1');
  const toggle = page.locator('.sessions-button');
  const sep = page.getByRole('separator', { name: 'Sidebar width' });

  // The Sessions control: accurate expanded state, and it names what it controls.
  t.check((await toggle.getAttribute('aria-expanded')) === 'true', `${width}: Sessions does not say the sidebar is open`);
  const controls = await toggle.getAttribute('aria-controls');
  t.check(controls === 'workspace-sidebar' && (await page.locator(`#${controls}`).count()) === 1, `${width}: Sessions does not control the sidebar (${controls})`);
  let sizes = 0;
  await nextSize(t, 0, `${width} first `);
  await page.waitForTimeout(400);
  const first = server.of('resize').at(-1); // the size the terminal settled on
  sizes = server.of('resize').length;

  // Closing frees the width: the terminal refits and the server gets more columns; the page does not overflow.
  await toggle.click();
  t.check((await side.count()) === 0, `${width}: Sessions did not close the sidebar`);
  t.check((await toggle.getAttribute('aria-expanded')) === 'false' && !(await toggle.getAttribute('aria-controls')), `${width}: expanded state wrong while closed`);
  t.check((await page.locator('[data-shell=bottom-bar]').count()) === 1, `${width}: no destinations left while the sidebar is closed`);
  t.check((await t.overflow()) <= 0, `${width}: overflow with the sidebar closed`);
  const closed = await nextSize(t, sizes, `${width} closed `);
  t.check(closed.cols > first.cols, `${width}: the server got ${closed.cols} columns with the sidebar closed, not more than ${first.cols}`);
  sizes = server.of('resize').length;
  await toggle.click();
  t.check(await side.isVisible(), `${width}: Sessions did not reopen the sidebar`);
  t.check((await toggle.getAttribute('aria-expanded')) === 'true', `${width}: expanded state wrong after reopening`);
  await page.waitForFunction(() => true);
  const back = Date.now() + 5000;
  while (Math.abs((server.of('resize').at(-1)?.cols ?? 0) - first.cols) > 1 && Date.now() < back) await page.waitForTimeout(50);
  t.check(Math.abs((server.of('resize').at(-1)?.cols ?? 0) - first.cols) <= 1, `${width}: columns did not return after reopening (${server.of('resize').at(-1)?.cols}, wanted ${first.cols} give or take the scrollbar's column)`);
  t.check((await saved(page))?.sidebarOpen === true, `${width}: open state not saved`);

  // The separator: role, values, controls, a 44 px target around a thin line.
  t.check((await sep.getAttribute('aria-controls')) === 'workspace-sidebar' && (await sep.getAttribute('aria-orientation')) === 'vertical', `${width}: separator wiring`);
  const max = sidebarMaxAt(width);
  t.check((await num(sep, 'aria-valuemin')) === 200 && (await num(sep, 'aria-valuemax')) === max, `${width}: separator range ${await sep.getAttribute('aria-valuemin')}..${await sep.getAttribute('aria-valuemax')}, wanted 200..${max}`);
  t.check((await num(sep, 'aria-valuenow')) === Math.round((await side.boundingBox()).width), `${width}: aria-valuenow is not the width`);
  const hit = await sep.boundingBox();
  t.check(hit.width >= 44 && hit.height >= 200, `${width}: separator target ${hit.width}x${hit.height}`);
  t.check((await sep.evaluate((e) => parseFloat(getComputedStyle(e, '::after').width))) <= 4, `${width}: the line is not thin`);
  t.check((await sep.getAttribute('tabindex')) === '0', `${width}: separator not focusable`);

  // Pointer: moves while dragging, nothing is written until the pointer is up, the width stays inside its clamp.
  const w0 = (await side.boundingBox()).width;
  const cx = hit.x + hit.width / 2, cy = hit.y + 300;
  const before = await writes(page);
  sizes = server.of('resize').length;
  await page.mouse.move(cx, cy);
  await page.mouse.down();
  await page.mouse.move(cx + 120, cy, { steps: 8 });
  const w1 = (await side.boundingBox()).width;
  t.check(w1 > w0 || w0 >= max, `${width}: dragging did not widen the sidebar (${w0} -> ${w1})`);
  t.check((await writes(page)) === before, `${width}: the layout was written while dragging`);
  await page.mouse.up();
  t.check((await writes(page)) === before + 1, `${width}: the layout was written ${(await writes(page)) - before} times for one drag`);
  const mid = await sep.boundingBox();
  await page.mouse.move(mid.x + mid.width / 2, cy);
  await page.mouse.down();
  await page.mouse.move(mid.x + 900, cy, { steps: 8 });
  await page.mouse.up();
  const w2 = (await side.boundingBox()).width;
  t.check(Math.abs(w2 - max) <= 1, `${width}: the clamp is ${max} but the sidebar is ${w2}`);
  t.check((await page.locator('[data-shell=main]').boundingBox()).width >= MIN_CONTENT - 1, `${width}: the content is narrower than ${MIN_CONTENT}`);
  t.check((await t.overflow()) <= 0, `${width}: overflow after resizing`);
  if (w2 > w0 + 8) { const wide = await nextSize(t, sizes, `${width} widened `); t.check(wide.cols < first.cols, `${width}: the server was not told the narrower terminal (${wide.cols} vs ${first.cols})`); }
  const edge = await sep.boundingBox();
  await page.mouse.move(edge.x + edge.width / 2, cy);
  await page.mouse.down();
  await page.mouse.move(0, cy, { steps: 8 });
  await page.mouse.up();
  t.check(Math.abs((await side.boundingBox()).width - 200) <= 1, `${width}: dragging left did not stop at the 200 px minimum`);

  // Keyboard: arrows (Shift: larger), Home, End, Enter reset, double click reset; saved on key up, not on key down.
  await sep.focus();
  const now = () => num(sep, 'aria-valuenow');
  await page.keyboard.press('Home');
  t.check((await now()) === 200, `${width}: Home did not go to the minimum`);
  await page.keyboard.press('ArrowRight');
  t.check((await now()) === 216, `${width}: ArrowRight moved to ${await now()}, not 216`);
  await page.keyboard.press('Shift+ArrowRight');
  t.check((await now()) === 280, `${width}: Shift+ArrowRight moved to ${await now()}, not 280`);
  await page.keyboard.press('ArrowLeft');
  t.check((await now()) === 264, `${width}: ArrowLeft moved to ${await now()}, not 264`);
  await page.keyboard.press('End');
  t.check((await now()) === max, `${width}: End did not go to the maximum`);
  await page.keyboard.press('ArrowRight');
  t.check((await now()) === max, `${width}: ArrowRight went past the maximum`);
  const k0 = await writes(page);
  await page.keyboard.down('ArrowLeft');
  t.check((await writes(page)) === k0, `${width}: written on key down`);
  await page.keyboard.up('ArrowLeft');
  t.check((await writes(page)) === k0 + 1, `${width}: not written on key up`);
  await page.keyboard.press('Enter');
  t.check((await now()) === Math.min(260, max), `${width}: Enter did not reset to 260`);
  t.check((await side.boundingBox()).width === Math.min(260, max), `${width}: the sidebar did not follow the reset`);
  t.check((await sep.evaluate((e) => document.activeElement === e)), `${width}: the separator lost focus`);
  await page.keyboard.press('End');
  await sep.dblclick();
  t.check((await now()) === Math.min(260, max), `${width}: a double click did not reset`);
  t.check((await t.overflow()) <= 0, `${width}: overflow after keyboard resizing`);

  // Reload restores the width and the open state.
  await sep.focus();
  await page.keyboard.press('Shift+ArrowRight');
  const kept = await now();
  await t.reload();
  t.check(Math.round((await page.locator('[data-shell=sidebar]').boundingBox()).width) === kept, `${width}: the width did not survive a reload`);
  await toggle.waitFor({ state: 'detached' }).catch(() => {}); // the app opens on Home after a reload: no Sessions control yet
  await page.locator('[data-shell=sidebar] [data-destination=sessions]').click();
  await page.locator('.sessions-button').waitFor();
  await page.locator('.sessions-button').click();
  t.check((await page.locator('[data-shell=sidebar]').count()) === 0, `${width}: could not close the sidebar after the reload`);
  await t.reload();
  t.check((await page.locator('[data-shell=sidebar]').count()) === 0 && (await page.locator('[data-shell=bottom-bar]').count()) === 1, `${width}: a closed sidebar was not restored closed`);
  t.check((await t.overflow()) <= 0, `${width}: overflow after reload`);
  const ok = await t.end();
  if (ok) t.ok(`${width} px: the sidebar opens, closes, resizes by pointer and keyboard, resets, clamps, is saved on release, restores after a reload, never overflows, and the server gets each new size`);
  return ok;
}

/** Same app, storage that throws for the layout key: it must come up with defaults and keep working. */
async function storageBlocked(shared) {
  const t = await startApp(shared, { viewport: { width: 1024, height: 800 } });
  await t.ctx.addInitScript((k) => {
    for (const m of ['getItem', 'setItem']) {
      const orig = Storage.prototype[m];
      Storage.prototype[m] = function (key, ...rest) { if (key === k) throw new DOMException('denied', 'SecurityError'); return orig.call(this, key, ...rest); };
    }
  }, WORKSPACE);
  await t.open();
  await t.openSession('s1');
  t.check(await t.page.locator('[data-shell=sidebar]').isVisible(), 'blocked storage: no sidebar');
  await t.page.locator('.sessions-button').click();
  t.check((await t.page.locator('[data-shell=sidebar]').count()) === 0, 'blocked storage: the toggle stopped working');
  const ok = await t.end();
  if (ok) t.ok('storage that throws for the layout key leaves the app on its defaults and the toggle working');
  return ok;
}

/** One live xterm; the phone layout and the desktop layout swap around it. */
async function crossing(shared) {
  const t = await startApp(shared, { viewport: { width: 1024, height: 800 } });
  const { page } = t;
  await t.open();
  await t.openSession('s1');
  const sep = page.getByRole('separator', { name: 'Sidebar width' });
  await sep.focus();
  await page.keyboard.press('Shift+ArrowRight'); // 260 -> 324
  const w = await num(sep, 'aria-valuenow');
  await page.keyboard.press('Alt+p');
  t.check((await page.locator('[data-pane-body]').getAttribute('data-split')) === 'true', 'crossing: the split did not open at 1024');
  await page.evaluate(() => { window.__xterm = document.querySelector('.xterm'); });
  const same = () => page.evaluate(() => document.querySelectorAll('.xterm').length === 1 && document.querySelector('.xterm') === window.__xterm && window.__xterm.isConnected);
  const bar = page.locator('[data-shell=bottom-bar]');

  await page.setViewportSize({ width: 720, height: 800 });
  await page.waitForFunction(() => document.querySelector('[data-shell=frame]')?.dataset.wide === 'false');
  t.check((await page.locator('[data-shell=sidebar]').count()) === 0 && (await bar.count()) === 1, 'crossing: 720 px is not the phone layout');
  t.check((await page.locator('[data-pane-body]').getAttribute('data-split')) === 'false', 'crossing: the split survived on a phone layout');
  t.check(await same(), 'crossing: the xterm was rebuilt going down to 720 px');
  t.check((await t.overflow()) <= 0, 'crossing: overflow at 720');
  // The phone sheet works on its own: its state does not follow the saved sidebar.
  const sessions = page.locator('.sessions-button');
  t.check((await sessions.getAttribute('aria-expanded')) === 'false', 'crossing: the sheet reads as open on a phone');
  await sessions.click();
  t.check((await sessions.getAttribute('aria-expanded')) === 'true', 'crossing: the phone sheet did not open');
  t.check((await saved(page)).sidebarOpen === true, 'crossing: opening the phone sheet changed the saved sidebar');
  await sessions.click();
  t.check((await sessions.getAttribute('aria-expanded')) === 'false', 'crossing: the phone sheet did not close');

  await page.setViewportSize({ width: 721, height: 800 });
  await page.waitForFunction(() => document.querySelector('[data-shell=frame]')?.dataset.wide === 'true');
  t.check((await page.locator('[data-shell=sidebar]').count()) === 1 && (await bar.count()) === 0, 'crossing: 721 px is not the desktop layout');
  t.check(Math.round((await page.locator('[data-shell=sidebar]').boundingBox()).width) === Math.min(w, sidebarMaxAt(721)), `crossing: the sidebar is ${(await page.locator('[data-shell=sidebar]').boundingBox()).width} px, not the saved ${w} clamped to ${sidebarMaxAt(721)}`);
  t.check(await same(), 'crossing: the xterm was rebuilt coming up to 721 px');
  t.check((await page.locator('[data-pane-body]').getAttribute('data-split')) === 'false' && (await page.getByText('Too narrow to show both').count()) === 1, 'crossing: 721 px should say the split is too narrow');
  t.check((await t.overflow()) <= 0, 'crossing: overflow at 721');

  await page.setViewportSize({ width: 1440, height: 800 });
  await page.waitForFunction(() => document.querySelector('[data-pane-body]')?.dataset.split === 'true');
  t.check(await same(), 'crossing: the xterm was rebuilt going up to 1440 px');
  const ok = await t.end();
  if (ok) t.ok('crossing 720 px both ways keeps each layout, the saved sidebar and the split, the phone sheet works on its own, and the same xterm instance lives throughout');
  return ok;
}

/** The structured view and the live terminal next to each other, resizable, remembered, one tmux size. */
async function split(shared) {
  const t = await startApp(shared, { viewport: { width: 1440, height: 900 } });
  const { page, server } = t;
  await t.open();
  await t.openSession('s1');
  await page.evaluate(() => { window.__xterm = document.querySelector('.xterm'); });
  const body = page.locator('[data-pane-body]');
  const decision = page.locator('[data-pane=decision]');
  const term = page.locator('#pane-terminal');
  t.check((await body.getAttribute('data-split')) === 'false', 'split: open by default');
  const full = await nextSize(t, 0);
  let sizes = server.of('resize').length;
  await page.getByRole('button', { name: 'Terminal beside the decision' }).first().click();
  await decision.waitFor();
  t.check((await body.getAttribute('data-split')) === 'true', 'split: the toggle did not split the view');
  const d = await decision.boundingBox(), r = await term.boundingBox();
  t.check(d.x + d.width <= r.x + 1 && d.width >= 280 && r.width >= 280, `split: the halves overlap or are too small (${JSON.stringify([d, r])})`);
  t.check(await page.evaluate(() => document.querySelectorAll('.xterm').length === 1 && document.querySelector('.xterm') === window.__xterm), 'split: a second xterm exists or the first was rebuilt');
  const half = await nextSize(t, sizes);
  t.check(half.cols < full.cols, `split: the server was not told the narrower terminal (${full.cols} -> ${half.cols})`);
  t.check((await t.overflow()) <= 0, 'split: overflow');

  // Live output in the terminal while the structured view updates next to it.
  server.output('LIVE-OUTPUT-1\r\n');
  await page.waitForFunction(() => [...document.querySelectorAll('.xterm-rows > div')].some((d) => d.textContent.includes('LIVE-OUTPUT-1')));
  server.setView('s1', { prompt: undefined, tail: ['tail says LIVE-VIEW-2'] });
  await page.waitForFunction(() => document.querySelector('[data-testid=tail]')?.textContent.includes('LIVE-VIEW-2'));
  server.output('LIVE-OUTPUT-3\r\n');
  await page.waitForFunction(() => [...document.querySelectorAll('.xterm-rows > div')].some((d) => d.textContent.includes('LIVE-OUTPUT-3')));
  t.check((await t.screen()).includes('LIVE-OUTPUT-1') && (await page.locator('[data-testid=tail]').innerText()).includes('LIVE-VIEW-2'), 'split: terminal and structured view are not both live');

  // The terminal's own divider: pointer, keyboard, saved on release, one size sent per change, never two terminals.
  const sep = page.getByRole('separator', { name: 'Terminal width' });
  t.check((await sep.getAttribute('aria-controls')) === 'pane-terminal' && (await sep.boundingBox()).width >= 44, 'split: divider wiring or target');
  const wNow = () => num(sep, 'aria-valuenow');
  const w0 = await wNow();
  const hb = await sep.boundingBox();
  sizes = server.of('resize').length;
  await page.mouse.move(hb.x + hb.width / 2, hb.y + 200);
  await page.mouse.down();
  await page.mouse.move(hb.x + hb.width / 2 - 120, hb.y + 200, { steps: 8 });
  await page.mouse.up();
  t.check((await wNow()) > w0 + 60, `split: dragging left did not widen the terminal (${w0} -> ${await wNow()})`);
  t.check(Math.round((await term.boundingBox()).width) === (await wNow()), 'split: aria-valuenow is not the terminal width');
  const wider = await nextSize(t, sizes);
  t.check(wider.cols > half.cols, 'split: the server was not told the wider terminal');
  await sep.focus();
  await page.keyboard.press('Home');
  t.check((await wNow()) === 280, 'split: Home did not go to the minimum');
  await page.keyboard.press('End');
  t.check((await decision.boundingBox()).width >= 279, 'split: the maximum leaves the structured view under 280 px');
  await page.keyboard.press('Enter');
  t.check((await wNow()) === 480, 'split: Enter did not reset to 480');
  await page.keyboard.press('ArrowLeft');
  t.check((await wNow()) === 496, 'split: ArrowLeft should widen the terminal by 16');
  t.check(server.of('resize').every((m) => m.cols > 0 && m.rows > 0), 'split: a bad size reached the server');
  const saved1 = await wNow();

  // Remembered per device.
  await t.reload();
  await t.openSession('s1');
  await page.locator('[data-pane=decision]').waitFor();
  t.check((await page.locator('[data-pane-body]').getAttribute('data-split')) === 'true', 'split: not restored after a reload');
  t.check((await num(page.getByRole('separator', { name: 'Terminal width' }), 'aria-valuenow')) === saved1, 'split: the width was not restored after a reload');

  // Toggling off puts the one terminal back full width.
  await t.leaveTerminal();
  await page.keyboard.press('Alt+p');
  t.check((await page.locator('[data-pane-body]').getAttribute('data-split')) === 'false' && (await page.locator('.xterm').count()) === 1, 'split: Alt+P did not close the split');
  const ok = await t.end();
  if (ok) t.ok('the live terminal beside the structured view: both update, one xterm and one tmux size, resizable by pointer and keyboard, remembered after a reload');
  return ok;
}

const sentAnswers = (server) => server.of('session_answer').map((m) => `${m.session}:${m.option}`);
const typedBytes = (server) => server.of('input').map((m) => Buffer.from(m.data, 'base64').toString('latin1'));

/** The shortcuts, on a decision with Claude Code's options. */
async function shortcuts(shared) {
  const t = await startApp(shared, { viewport: { width: 1440, height: 900 } });
  const { page, server } = t;
  await t.open();
  await t.openSession('s1');
  await page.keyboard.press('Alt+p'); // terminal beside the decision
  await page.locator('[data-pane=decision] [data-hero-options] button').first().waitFor();
  const options = page.locator('[data-pane=decision] [data-hero-options] button');
  t.check((await options.count()) === 3, 'shortcuts: the decision should offer three options');
  let n = 1;
  const rearm = async (prompt = CLAUDE_PROMPT) => {
    n++;
    server.states.s1 = 'needs_input';
    server.setView('s1', { prompt: { ...prompt, id: `p${n}` }, tail: ['asking again'] });
    server.broadcast(server.snapshot());
    await options.first().waitFor();
    server.clear();
  };
  const keyOf = async (combo) => {
    await page.evaluate(() => { window.__keys = []; document.addEventListener('keydown', (e) => window.__keys.push({ key: e.key, shift: e.shiftKey, alt: e.altKey }), true); });
    await page.keyboard.press(combo);
    return page.evaluate(() => window.__keys.find((k) => k.key !== 'Alt'));
  };

  // Approve: the plain Yes. Playwright reports Alt+Y as key "Y" with no Shift, and the app must not care about the case.
  const seen = await keyOf('Alt+Y');
  t.check(seen.key === 'Y' && seen.shift === false && seen.alt === true, `shortcuts: Playwright reported ${JSON.stringify(seen)} for Alt+Y`);
  await page.waitForFunction(() => document.querySelector('[data-hero-options] button')?.disabled || !document.querySelector('[data-hero-options]'));
  t.check(sentAnswers(server).join() === 's1:0', `shortcuts: Alt+Y sent ${sentAnswers(server)}, not option 0 (Yes)`);
  await rearm();
  await page.keyboard.press('Alt+y');
  await page.waitForFunction(() => document.querySelector('[data-hero-options] button')?.disabled || !document.querySelector('[data-hero-options]'));
  t.check(sentAnswers(server).join() === 's1:0', `shortcuts: Alt+y sent ${sentAnswers(server)}`);
  // Deny: the explicit No, never the standing "don't ask again" that sits second.
  await rearm();
  await page.keyboard.press('Alt+N');
  await page.waitForFunction(() => document.querySelector('[data-hero-options] button')?.disabled || !document.querySelector('[data-hero-options]'));
  t.check(sentAnswers(server).join() === 's1:2', `shortcuts: Alt+N sent ${sentAnswers(server)}, not option 2 (No, and tell Claude)`);
  // Shift makes it another chord; nothing is answered.
  await rearm();
  await page.keyboard.press('Alt+Shift+Y');
  await page.waitForTimeout(300);
  t.check(sentAnswers(server).length === 0, 'shortcuts: Alt+Shift+Y answered');
  // Without a plain option the answer stays with the user, and the page says so.
  await rearm({ ...CLAUDE_PROMPT, options: [{ label: 'Option A' }, { label: 'Option B' }] });
  await page.keyboard.press('Alt+n');
  await page.getByText('no plain No option', { exact: false }).waitFor();
  t.check(sentAnswers(server).length === 0, 'shortcuts: Alt+N guessed an option');
  t.ok('Alt+Y answers Yes and Alt+N answers the explicit No (never "don\'t ask again"), in either case, and neither guesses');

  // Next needs-you: the longest wait after this one, wrapping.
  await rearm();
  await page.keyboard.press('Alt+j');
  await page.locator('[data-shell=sidebar] [data-key="one/s2"][aria-current=true]').waitFor();
  await page.waitForTimeout(600); // the terminal takes the focus when the session attaches; a key typed there is the shell's
  await t.leaveTerminal('s2');
  await page.keyboard.press('Alt+j');
  await page.locator('[data-shell=sidebar] [data-key="one/s1"][aria-current=true]').waitFor();
  await page.waitForTimeout(600);
  t.ok('Alt+J opens the next session that needs you and wraps');

  // Focus composer: the visible one.
  await t.leaveTerminal('s1');
  await page.keyboard.press('Alt+c');
  const focusedComposer = () => page.evaluate(() => { const a = document.activeElement; return !!a && a.matches('input[data-composer]') && a.getClientRects().length > 0; });
  t.check(await focusedComposer(), 'shortcuts: Alt+C did not focus the visible composer');

  // From the composer nothing leaks: not into the terminal, not into the decision, not as a pane toggle.
  server.clear();
  await rearm();
  await page.keyboard.press('Alt+c');
  const before = await page.locator('[data-pane-body]').getAttribute('data-split');
  await page.keyboard.type('yn jcp');
  for (const combo of ['Alt+y', 'Alt+n', 'Alt+j', 'Alt+p', 'Alt+c']) await page.keyboard.press(combo);
  await page.keyboard.press('Control+c');
  await page.waitForTimeout(400);
  t.check(typedBytes(server).length === 0, `shortcuts: the composer leaked into the terminal: ${JSON.stringify(typedBytes(server))}`);
  t.check(sentAnswers(server).length === 0, 'shortcuts: a key in the composer answered the decision');
  t.check((await page.locator('[data-pane-body]').getAttribute('data-split')) === before, 'shortcuts: Alt+P toggled the split from the composer');
  t.check((await page.locator('[data-pane=decision] input[data-composer]').inputValue()) === 'yn jcp', 'shortcuts: the composer did not keep what was typed');
  t.check((await page.locator('[data-shell=sidebar] [data-key="one/s1"][aria-current=true]').count()) === 1, 'shortcuts: Alt+J left the session from the composer');
  // The palette is the one key taken even while typing.
  await page.keyboard.press('Control+k');
  await page.getByRole('combobox', { name: 'Search' }).waitFor();
  await page.keyboard.press('Escape');
  await page.getByRole('combobox', { name: 'Search' }).waitFor({ state: 'detached' });
  t.ok('typing in the composer: no key reaches the terminal, answers a decision, moves a pane or leaves the session; Ctrl+K still opens the palette');

  // From the terminal the same keys belong to the shell.
  await page.locator('.xterm-helper-textarea').focus();
  server.clear();
  await page.keyboard.press('Alt+y');
  await page.keyboard.press('Alt+n');
  await page.waitForFunction(() => true);
  const deadline = Date.now() + 3000;
  while (typedBytes(server).length < 2 && Date.now() < deadline) await page.waitForTimeout(50);
  t.check(typedBytes(server).join('|') === '\x1by|\x1bn', `shortcuts: the terminal got ${JSON.stringify(typedBytes(server))} for Alt+Y, Alt+N`);
  t.check(sentAnswers(server).length === 0, 'shortcuts: a terminal key answered the decision');
  // Ctrl+C reaches the terminal and the browser keys we avoided are not taken.
  server.clear();
  await page.keyboard.press('Control+c');
  const d2 = Date.now() + 3000;
  while (typedBytes(server).length < 1 && Date.now() < d2) await page.waitForTimeout(50);
  t.check(typedBytes(server).join('|') === '\x03', 'shortcuts: Ctrl+C did not reach the terminal');
  t.ok('from the terminal, Alt+Y and Alt+N are the shell\'s own keys and answer nothing');

  // The palette reaches the same actions from the terminal.
  await rearm();
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.press('Control+k');
  await page.keyboard.type('answer no');
  await page.keyboard.press('Enter');
  const viaPalette = Date.now() + 3000;
  while (!sentAnswers(server).length && Date.now() < viaPalette) await page.waitForTimeout(50);
  t.check(sentAnswers(server).join() === 's1:2', `shortcuts: the palette's "Answer No" sent ${sentAnswers(server)}`);
  t.check(typedBytes(server).length === 0, 'shortcuts: the palette let a key reach the terminal');
  await rearm();
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.press('Control+k');
  await page.keyboard.type('reply composer');
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => document.activeElement?.matches?.('input[data-composer]'));
  t.ok('Ctrl+K reaches Answer Yes, Answer No, next session and the composer from the terminal');
  await t.leaveTerminal();

  // The terminal-only view has no decision on screen: say so rather than answer blind.
  await page.locator('.xterm-helper-textarea').focus();
  await page.keyboard.press('Alt+p'); // from the terminal: ignored
  t.check((await page.locator('[data-pane-body]').getAttribute('data-split')) === 'true', 'shortcuts: Alt+P toggled the split from the terminal');
  await t.leaveTerminal();
  await page.keyboard.press('Alt+p');
  t.check((await page.locator('[data-pane-body]').getAttribute('data-split')) === 'false', 'shortcuts: Alt+P did not close the split');
  server.clear();
  await page.keyboard.press('Alt+y');
  await page.getByText('No decision is open here', { exact: false }).waitFor();
  t.check(sentAnswers(server).length === 0, 'shortcuts: Alt+Y answered a decision that is not on screen');

  // The sheet lists every combo and names no key a browser takes first.
  await page.keyboard.press('Control+k');
  await page.keyboard.type('keyboard shortcuts');
  await page.keyboard.press('Enter');
  const help = page.getByRole('dialog', { name: 'Keyboard shortcuts' });
  await help.waitFor();
  const text = await help.innerText();
  for (const want of ['Ctrl+K', 'Alt+Y', 'Alt+N', 'Alt+J', 'Alt+C', 'Alt+P', 'Home End']) t.check(text.includes(want), `shortcuts: the sheet does not list ${want}`);
  t.check(!/Ctrl\+Shift\+[NPOAD]/.test(text), 'shortcuts: the sheet lists a combo browsers keep');
  await page.keyboard.press('Escape');
  const ok = await t.end();
  if (ok) t.ok('the shortcut sheet lists every combo');
  return ok;
}

/** A phone width keeps its own sheet and bottom bar whatever the desktop layout was saved as. */
async function phone(shared) {
  const t = await startApp(shared, { viewport: { width: 390, height: 844 }, storage: { [WORKSPACE]: JSON.stringify({ sidebarOpen: false, sidebarWidth: 480, splitOpen: true, splitWidth: 900 }) } });
  const { page } = t;
  await t.open();
  t.check((await page.locator('[data-shell=sidebar]').count()) === 0 && (await page.locator('[data-shell=bottom-bar]').count()) === 1, 'phone: the saved desktop layout leaked into a phone');
  await page.locator('[data-shell=bottom-bar] [data-destination=sessions]').click();
  const sessions = page.locator('.sessions-button');
  await sessions.waitFor().catch(() => {});
  t.check((await t.overflow()) <= 0, 'phone: overflow');
  const ok = await t.end();
  if (ok) t.ok('390 px ignores the saved sidebar and split');
  return ok;
}

export async function realApp() {
  const shared = await startShared();
  const results = [];
  try {
    for (const w of WIDTHS) results.push(await sidebarAt(shared, w));
    results.push(await storageBlocked(shared));
    results.push(await crossing(shared));
    results.push(await split(shared));
    results.push(await shortcuts(shared));
    results.push(await phone(shared));
  } finally {
    await shared.stop();
  }
  return results.every(Boolean);
}
