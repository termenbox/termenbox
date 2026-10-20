// Desktop (mouse and keyboard, no touch) scenarios. Each desktop step of the roadmap adds its own scenario here
// (desktop review, PR #149).
//   Part 1 (1280 px, U04): the shell probe: the sidebar with the four destinations and every session of two servers, the
//   Ctrl/Cmd+K palette, its shortcut list and off switch, and a sidebar that holds still under the pointer. The probe's
//   terminal is a stand-in textarea, so this part proves what the page does with a key, not what xterm does with it.
//   Part 2 (U15): the REAL app (real hub, connection, xterm, session view, decision card) against a scripted server:
//   sidebar toggle/resize/keyboard/reload at 721, 1024, 1440 and 1920 px, crossing 720 px, the live terminal beside the
//   structured view, the shortcuts, and no key reaching the terminal from the composer. See app-lib.mjs for its limits.
// Usage: node e2e/desktop.mjs (after: pnpm run e2e:build, and the app's own: pnpm build)
import { start } from './frame-lib.mjs';
import { realApp } from './desktop-app.mjs';

const t = await start({ viewport: { width: 1280, height: 800 }, isMobile: false, hasTouch: false });
const failures = [];
const { page } = t;
await t.open();

// 1. Sidebar, not a bottom bar; every destination reachable; 20 sessions on two servers, each with a state word.
const side = page.locator('[data-shell=sidebar]');
t.check(await side.isVisible(), 'no sidebar at 1280 px');
t.check((await page.locator('[data-shell=bottom-bar]').count()) === 0, 'the bottom bar is on the desktop');
for (const [id, heading] of [['home', 'Home'], ['activity', 'Activity'], ['settings', 'Settings'], ['sessions', null]]) {
  await side.locator(`[data-destination=${id}]`).click();
  t.check((await t.destination()) === id, `the sidebar did not reach ${id}`);
  t.check((await side.locator(`[data-destination=${id}]`).getAttribute('aria-current')) === 'page', `${id} is not marked as the current page`);
  if (heading) await page.getByRole('heading', { name: heading, exact: true }).waitFor();
  t.check((await t.overflow()) <= 0, `${id} overflows sideways`);
}
t.check((await side.locator('[data-key]').count()) === 20, `the sidebar lists ${await side.locator('[data-key]').count()} sessions, not 20`);
t.check((await side.locator('section[data-server]').count()) === 2, 'the sessions are not grouped under two servers');
t.check((await side.locator('[data-key="alpha/svc-2"]').innerText()).includes('Needs you'), 'a session needing you does not say so in words');
t.check((await t.smallTargets('[data-shell=sidebar]')).length === 0, `sidebar targets under 44 pt: ${await t.smallTargets('[data-shell=sidebar]')}`);
t.ok('sidebar: four destinations, 20 sessions on two servers with their states');

// 2. Open a session from the sidebar; the in-session tab switch and the strip.
await side.locator('[data-key="alpha/svc-2"]').click();
t.check((await t.api('active')) === 'alpha/svc-2', 'a sidebar session did not open');
await t.inSessionTabSwitch();
await t.stripKeyboard();

// 3. Ordinary keys, the palette, Escape.
await t.ordinaryKeysReachTerminal();
await t.paletteOpensSessionByName();
await t.escapeRestoresFocus();

// 4. The palette also takes destinations, servers and actions, and opens from the sidebar's Search button.
const input = page.getByRole('combobox', { name: 'Search' });
// Typing before the input has focus loses the first letters, so wait for the focus, not only for the box to show.
const openPalette = async () => {
  await side.getByRole('button', { name: /^Search/ }).click();
  await input.waitFor();
  await page.waitForFunction(() => document.activeElement?.getAttribute('role') === 'combobox', null, { timeout: 5000 });
};
await openPalette();
await page.keyboard.type('activity');
await page.keyboard.press('Enter');
t.check((await t.destination()) === 'activity', 'the palette did not go to Activity');
await openPalette();
await page.keyboard.type('pause alpha');
await page.keyboard.press('Enter');
t.check((await t.api('actions')).includes('pause:alpha'), 'the palette did not run "Pause server alpha"');
await openPalette();
await page.keyboard.type('beta job-3');
await page.keyboard.press('Enter');
t.check((await t.api('active')) === 'beta/job-3', 'a server name plus a session name did not find the session');
await openPalette();
await page.keyboard.type('zzzzzz');
await page.getByText('Nothing matches').waitFor();
await page.keyboard.press('Escape');
t.check(await side.getByRole('button', { name: /^Search/ }).evaluate((e) => document.activeElement === e), 'focus did not return to the Search button after Escape');
t.check((await t.smallTargets('[data-shell=palette]')).length === 0, 'palette targets under 44 pt');
t.ok('the palette opens a destination, runs an action, finds a server+session, says when nothing matches, and returns focus');

// 5. Shortcut list from the palette; shortcuts can be turned off in Settings and then Ctrl+K belongs to the terminal.
await page.keyboard.press('Control+k');
await page.keyboard.type('keyboard shortcuts');
await page.keyboard.press('Enter');
const help = page.getByRole('dialog', { name: 'Keyboard shortcuts' });
await help.waitFor();
t.check((await help.innerText()).includes('Ctrl+K'), 'the shortcut list does not name Ctrl+K');
await page.keyboard.press('Escape');
await help.waitFor({ state: 'detached' });
await side.locator('[data-destination=settings]').click();
await page.getByRole('switch', { name: /Keyboard shortcuts/ }).click();
await side.locator('[data-destination=sessions]').click();
await t.api('clearKeys');
await page.locator('textarea[aria-label="Terminal input"]').focus();
await page.keyboard.press('Control+k');
await page.waitForTimeout(300);
t.check((await page.getByRole('combobox', { name: 'Search' }).count()) === 0, 'Ctrl+K opened the palette with shortcuts off');
t.check((await t.api('keys')).some((k) => k.key === 'k' && k.ctrl && !k.prevented), 'Ctrl+K did not reach the terminal with shortcuts off');
await side.locator('[data-destination=settings]').click();
await page.getByRole('switch', { name: /Keyboard shortcuts/ }).click();
await side.locator('[data-destination=sessions]').click();
await page.locator('textarea[aria-label="Terminal input"]').focus();
await page.keyboard.press('Control+k');
await page.getByRole('combobox', { name: 'Search' }).waitFor();
await page.keyboard.press('Escape');
t.ok('the shortcut list opens from the palette and names the keys; Settings turns shortcuts off and on');

// 7. The sidebar holds still under the pointer: press on a session, a status update arrives, nothing moves; release, then it re-sorts.
await t.api('setState', 'alpha', 'svc-9', 'idle');
await page.waitForTimeout(2300);
const rows = '[data-shell=sidebar] [data-key]';
const before = await t.rects(rows);
const box = await side.locator('[data-key="alpha/svc-7"]').boundingBox();
await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
await page.mouse.down();
await t.api('setState', 'alpha', 'svc-9', 'needs_input');
await page.waitForTimeout(400);
const during = await t.rects(rows);
t.check(JSON.stringify(before) === JSON.stringify(during), 'a sidebar row moved while the pointer was down and a status update arrived');
t.check((await side.locator('[data-key="alpha/svc-9"]').innerText()).includes('Needs you'), 'the row did not show the new state in place');
await page.mouse.move(box.x + box.width + 400, box.y); // release away from the row: no click
await page.mouse.up();
await page.waitForTimeout(300);
const after = await t.rects(rows);
t.check(JSON.stringify(after) !== JSON.stringify(before), 'the sidebar did not apply the held update after release');
t.ok('no control moves when a status update arrives while the pointer is down');

if (!(await t.end())) failures.push('shell probe');
if (!(await realApp())) failures.push('real app');
if (failures.length) { console.error(`FAIL: ${failures.join(', ')}`); process.exit(1); }
console.log('PASS');
