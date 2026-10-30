// The New workspace dialog (DECISION-0148) at five widths, against the built client and a scripted server whose `list_dir`
// answers from a fake home (fake-tree.mjs). Every state is asserted visible BEFORE it is checked: a missing element throws and the
// script exits non-zero, never skips (GUARDRAIL ux-scenarios-assert-a-state-before-they-capture-it). Hard UX rules checked in
// every state: no horizontal overflow, every tappable at least 44 x 44, no console error.
// What this does not prove: a real soft keyboard (the 360 x 440 resize only emulates one) or the real server's confinement (run.mjs).
import { startShared, startApp } from './app-lib.mjs';
import { HOME } from './fake-tree.mjs';

const WIDTHS = [[360, 740], [640, 800], [768, 900], [1024, 768], [1440, 900]];
const only = process.env.E2E_VIEWPORT ? process.env.E2E_VIEWPORT.split('x').map(Number) : null;
// The scenario context has no touch support (app-lib), so a tap is a click: same pointer events, 44 px targets are checked by size.
const tap = (locator) => locator.click();
const shared = await startShared();
let allOk = true;

// E2E_VIEWPORT (the phone suite) runs that one size; otherwise the five widths.
for (const [width, height] of only ? [only] : WIDTHS) {
  const t = await startApp(shared, { viewport: { width, height } });
  const { page } = t;
  const at = (m) => `${width}px: ${m}`;
  const dialog = page.locator('dialog[aria-label="New workspace"]');
  const q = (id) => dialog.locator(`[data-testid="${id}"]`);
  const see = async (locator, what) => { await locator.first().waitFor({ state: 'visible', timeout: 5000 }).catch(() => { throw new Error(at(`expected ${what} to be visible`)); }); };
  const gone = async (locator, what) => { await locator.first().waitFor({ state: 'detached', timeout: 5000 }).catch(() => { throw new Error(at(`expected ${what} to be gone`)); }); };
  const text = async (locator, re, what) => {
    await see(locator, what);
    await page.waitForFunction(([el, src]) => new RegExp(src).test(el.textContent ?? ''), [await locator.first().elementHandle(), re.source], { timeout: 5000 })
      .catch(async () => { throw new Error(at(`${what}: wanted ${re}, got "${await locator.first().innerText()}"`)); });
  };
  /** The hard rules, in whatever state the dialog is in now. */
  const rules = async (state) => {
    const r = await page.evaluate(() => {
      const d = document.querySelector('dialog[aria-label="New workspace"]');
      const small = [];
      for (const el of d.querySelectorAll('button, [role=radio], [role=option], input:not([type=checkbox]), select, label:has(input[type=checkbox])')) {
        const b = el.getBoundingClientRect();
        if (b.width === 0 || b.height === 0 || el.closest('[hidden], [inert]') || getComputedStyle(el).visibility === 'hidden') continue;
        if (b.width < 43.5 || b.height < 43.5) small.push(`${el.tagName.toLowerCase()}${el.dataset.testid ? `[${el.dataset.testid}]` : ''} "${(el.textContent || el.getAttribute('aria-label') || '').trim().slice(0, 20)}" ${Math.round(b.width)}x${Math.round(b.height)}`);
      }
      return { page: document.body.scrollWidth - innerWidth, dialog: d.scrollWidth - d.clientWidth, small };
    });
    if (r.page > 0 || r.dialog > 0) throw new Error(at(`${state}: horizontal overflow (page ${r.page}, dialog ${r.dialog})`));
    if (r.small.length) throw new Error(at(`${state}: targets under 44 px: ${r.small.join('; ')}`));
    if (t.errors.length) throw new Error(at(`${state}: console errors: ${t.errors.join(' | ')}`));
    t.ok(at(state));
  };
  const open = async () => {
    // The sidebar's "+ New" where there is a sidebar; below that the command palette's New workspace action (Ctrl+K).
    const plus = page.getByRole('button', { name: /^(\+ New|New workspace( on .*)?)$/ }).first();
    if (await plus.isVisible()) await plus.click();
    else {
      await page.keyboard.press('Control+K');
      await page.getByRole('option', { name: /New workspace/ }).first().click({ timeout: 10000 });
    }
    await see(dialog, 'the New workspace dialog');
    await see(q('agent-row'), 'the agent row');
  };
  const focused = (testid) => page.evaluate((id) => document.activeElement?.getAttribute('data-testid') === id, testid);
  const go = async (path) => { await q('picker-path').fill(path); await tap(q('picker-go')); };

  try {
    await t.open();
    await open();

    // Default: three tiles and More, cold start claude / codex / shell, the first preselected, a summary, Create enabled.
    await see(q('agents-more'), 'the More tile');
    const tiles = await q('agent-tile').evaluateAll((els) => els.map((e) => [...e.querySelectorAll(':scope > span')].at(-1).textContent.trim()));
    if (tiles.join('|') !== 'Claude Code|Codex|Shell') throw new Error(at(`cold-start tiles are ${tiles.join('|')}`));
    if ((await q('agent-tile').first().getAttribute('aria-checked')) !== 'true') throw new Error(at('the first tile is not selected'));
    await text(q('summary'), /^Claude Code in the default folder$/, 'the summary');
    if (await dialog.locator('input[aria-label="Folder"], input[name=dir], input[placeholder*="/home"]').count()) throw new Error(at('the form has a free-text path input'));
    await see(dialog.getByRole('button', { name: 'Create' }), 'Create');
    await rules('default');

    // Agents layer: open, filter to a match, filter to nothing, pick; the pick shows as a selected tile and nothing is saved yet.
    await tap(q('agents-more'));
    await see(q('agents-layer'), 'the agents layer');
    await see(q('agents-layer').getByRole('option', { name: /Gemini/ }), 'the Gemini option');
    await rules('agents layer');
    await q('agents-layer').getByRole('searchbox').or(q('agents-layer').locator('input')).first().fill('gem');
    await see(q('agents-layer').getByRole('option', { name: /Gemini/ }), 'Gemini after filtering');
    if (await q('agents-layer').getByRole('option', { name: /Codex/ }).count()) throw new Error(at('the filter left Codex in the list'));
    await rules('agents layer filtered');
    await q('agents-layer').locator('input').first().fill('zzz');
    await see(q('agents-layer').getByText('No agents match'), 'the empty message');
    await q('agents-layer').locator('input').first().fill('gem');
    await tap(q('agents-layer').getByRole('option', { name: /Gemini/ }));
    await gone(q('agents-layer'), 'the agents layer');
    if (!(await focused('agents-more'))) throw new Error(at('focus did not return to the More tile'));
    await text(q('summary'), /^Gemini in the default folder$/, 'the summary after a layer pick');
    const picked = await q('agent-tile').evaluateAll((els) => els.filter((e) => e.getAttribute('aria-checked') === 'true').map((e) => [...e.querySelectorAll(':scope > span')].at(-1).textContent.trim()));
    if (picked.join() !== 'Gemini') throw new Error(at(`selected tile is ${picked.join() || 'none'}`));
    if (await page.evaluate(() => localStorage.getItem('termenbox_recent_agents'))) throw new Error(at('a pick saved a recent agent before Create'));
    await rules('layer pick');

    // Escape closes the layer first, then the dialog.
    await tap(q('agents-more'));
    await see(q('agents-layer'), 'the agents layer');
    await page.keyboard.press('Escape');
    await gone(q('agents-layer'), 'the agents layer after Escape');
    await see(dialog, 'the dialog after the first Escape');
    await page.keyboard.press('Escape');
    await dialog.waitFor({ state: 'detached', timeout: 5000 }).catch(() => { throw new Error(at('the second Escape did not close the dialog')); });

    // Folder picker at the root, nested, empty, error + Retry, truncated, hidden, typed paths.
    await open();
    await text(q('folder-value'), /Choose a folder/, 'the empty folder field');
    await text(q('summary'), /^Claude Code in the default folder$/, 'the summary');
    await tap(q('folder-change'));
    await see(q('folder-picker'), 'the folder picker');
    await see(q('picker-list').locator('[data-dir="code"]'), 'code at the root');
    if (await q('picker-list').locator('[data-dir=".config"]').count()) throw new Error(at('a hidden folder is listed by default'));
    if (!(await q('picker-up').isDisabled())) throw new Error(at('Up is enabled at the home folder'));
    await rules('picker at root');
    await tap(q('picker-list').locator('[data-dir="code"]'));
    await see(q('picker-list').locator('[data-dir="termenbox"]'), 'termenbox inside code');
    await text(q('picker-trail'), /code/, 'the breadcrumb');
    if (await q('picker-up').isDisabled()) throw new Error(at('Up is disabled inside code'));
    await text(q('picker-select'), /code/, 'the Select button');
    await rules('picker nested');
    await q('picker-filter').fill('app');
    await see(q('picker-list').locator('[data-dir="app"]'), 'app after filtering');
    await gone(q('picker-list').locator('[data-dir="api"]'), 'api after filtering (the filter is debounced)');
    if (!t.server.tree.requests.some((r) => r.prefix === 'app')) throw new Error(at('the filter did not send a prefix'));
    await q('picker-filter').fill('');
    await see(q('picker-list').locator('[data-dir="api"]'), 'api after clearing the filter');
    await tap(q('picker-up'));
    await see(q('picker-list').locator('[data-dir="empty"]'), 'the root again');
    await tap(q('picker-list').locator('[data-dir="empty"]'));
    await see(q('picker-empty'), 'the empty folder message');
    await text(q('picker-empty'), /No folders here/, 'the empty message');
    await rules('picker empty');
    await tap(q('picker-home'));
    await tap(q('picker-list').locator('[data-dir="locked"]'));
    await see(q('picker-error'), 'the error state');
    if (!(await q('picker-select').isDisabled())) throw new Error(at('Select is enabled while the listing errored'));
    await rules('picker error');
    t.server.tree.broken.clear();
    await tap(q('picker-retry'));
    await see(q('picker-empty'), 'the folder after Retry');
    await rules('picker after retry');
    await tap(q('picker-home'));
    await tap(q('picker-list').locator('[data-dir="big"]'));
    await see(q('picker-truncated'), 'the truncated notice');
    await text(q('picker-truncated'), /first 500/, 'the truncated notice');
    await rules('picker truncated');
    await tap(q('picker-home'));
    await see(q('picker-list').locator('[data-dir="code"]'), 'the root');
    await tap(q('picker-hidden'));
    await see(q('picker-list').locator('[data-dir=".config"]'), 'the hidden folder after the toggle');
    await rules('picker hidden shown');
    await tap(q('picker-hidden'));

    // Typed paths: a relative one is refused, one inside home navigates, one outside is accepted as typed.
    await go('code/app');
    await see(q('picker-path-error'), 'the relative path error');
    await rules('typed relative path');
    await go(`${HOME}/code/app`);
    await see(q('picker-empty'), 'the typed folder');
    await text(q('picker-select'), /app/, 'Select for the typed folder');
    await tap(q('picker-select'));
    await gone(q('folder-picker'), 'the picker after Select');
    await text(q('folder-value'), /code\/app$/, 'the chosen folder');
    await text(q('summary'), /^Claude Code in ~\/code\/app$/, 'the summary with a folder');
    if (!(await focused('folder-change'))) throw new Error(at('focus did not return to Change'));
    await rules('folder chosen');

    // A 200-character folder outside home, accepted as typed, truncates instead of overflowing.
    const long = `/srv/${'segment-'.repeat(24)}end`;
    await tap(q('folder-change'));
    await go(long);
    await see(q('picker-outside'), 'the outside-home notice');
    await text(q('picker-select'), /segment-/, 'the full long path in the picker footer');
    await tap(q('picker-select'));
    await gone(q('folder-picker'), 'the picker after Select');
    await text(q('folder-value'), /segment-/, 'the full long path');
    await text(q('summary'), /segment-/, 'the full long path in the summary');
    await rules('long path');

    // Create: recents are written now, not before; the request carries the folder.
    await page.evaluate(() => localStorage.removeItem('termenbox_recent_dirs'));
    await tap(q('folder-change'));
    await go(`${HOME}/code/app`);
    await tap(q('picker-select'));
    await tap(dialog.getByRole('button', { name: 'Create' }));
    await dialog.waitFor({ state: 'detached', timeout: 5000 }).catch(() => { throw new Error(at('Create did not close the dialog')); });
    await page.waitForFunction(() => localStorage.getItem('termenbox_recent_dirs')?.includes('code/app'), null, { timeout: 5000 }).catch(() => { throw new Error(at('Create did not save the recent folder')); });
    const made = t.server.of('new_workspace').at(-1);
    if (!made || made.dir !== `${HOME}/code/app`) throw new Error(at(`new_workspace carried ${JSON.stringify(made)}`));

    // Recent chips, now that a folder is remembered; More opens the limited list.
    await open();
    await see(q('dir-chip'), 'a recent chip');
    const chipTitle = await q('dir-chip').first().getAttribute('title');
    if (chipTitle !== `${HOME}/code/app`) throw new Error(at(`first chip is ${chipTitle}`));
    await rules('recent chips');
    await tap(q('dir-chip').first());
    await text(q('folder-value'), /code\/app$/, 'the folder from a chip');
    await text(q('summary'), /~\/code\/app$/, 'the summary from a chip');
    await page.evaluate(() => localStorage.setItem('termenbox_recent_dirs', JSON.stringify({ one: ['/home/tester/code/app', '/home/tester/code/api', '/home/tester/notes', '/home/tester/empty', '/home/tester/big'] })));
    await page.reload();
    await t.open();
    await open();
    if (await q('dirs-more').count() === 0) throw new Error(at('five recent folders show no More chip'));
    await tap(q('dirs-more'));
    await see(q('dirs-layer'), 'the recent folders layer');
    await rules('recent folders layer');
    await q('dirs-layer').locator('input').first().fill('notes');
    await see(q('dirs-layer').getByRole('option', { name: /notes/ }), 'notes after filtering');
    await tap(q('dirs-layer').getByRole('option', { name: /notes/ }));
    await gone(q('dirs-layer'), 'the recent folders layer');
    await text(q('summary'), /~\/notes$/, 'the summary from the layer');

    // Create is disabled with the reason visible: a taken name, and an invalid one.
    await q('dirs-more').count();
    await dialog.getByLabel('Name').fill('s1');
    await see(dialog.getByText('That name is taken.'), 'the taken-name reason');
    if (!(await dialog.getByRole('button', { name: 'Create' }).isDisabled())) throw new Error(at('Create is enabled with a taken name'));
    await rules('create disabled, name taken');
    // `|` is the list separator the server refuses; a space and punctuation are valid label text (0146/0149).
    await dialog.getByLabel('Name').fill('bad|name');
    await see(dialog.getByText(/Use one line of at most 64 characters, without \|/), 'the invalid-name reason');
    await rules('create disabled, invalid name');
    await page.keyboard.press('Escape');
    await dialog.waitFor({ state: 'detached', timeout: 5000 });

    // Keyboard open, emulated: the viewport shrinks to 440 px high; the focused path input and the footer stay in view.
    if (width <= 480) {
      await open();
      await tap(q('folder-change'));
      await see(q('folder-picker'), 'the folder picker');
      await page.setViewportSize({ width, height: 440 });
      await q('picker-path').focus();
      await page.waitForTimeout(300);
      const inView = async (id) => { const b = await q(id).first().boundingBox(); return !!b && b.y >= 0 && b.y + b.height <= 440; };
      if (!(await inView('picker-path'))) throw new Error(at('the path input is out of view with the keyboard open'));
      if (!(await inView('picker-select'))) throw new Error(at('the Select button is out of view with the keyboard open'));
      await rules('keyboard open');
      await page.setViewportSize({ width, height });
    }
    t.ok(at('all states'));
  } catch (e) {
    t.fail(e.message);
  }
  if (!(await t.end())) allOk = false;
}
await shared.stop();
if (!allOk) process.exit(1);
console.log('newworkspace: all passed');
