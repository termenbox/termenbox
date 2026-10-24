// U04 on a phone: 390 px, touch, mobile emulation (desktop Chromium: not an iPhone or Android check). The four destinations
// by the bottom bar, the bar hidden under the soft keyboard in a session, the palette, the strip by keyboard, nothing moving
// under a touch, swipe-back. Usage: node e2e/appshell.mjs (after: pnpm run e2e:build)
import { start } from './frame-lib.mjs';
import { ENGINE } from './browser.mjs';

const t = await start({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
const { page } = t;
await t.open();

// 1. Every destination by the bottom bar; the bar is four 44+ px targets 8 px apart and nothing overflows sideways.
const bar = page.getByRole('navigation', { name: 'Main' });
t.check((await page.locator('[data-shell=sidebar]').count()) === 0, 'the sidebar is on a phone');
for (const [id, heading] of [['home', 'Home'], ['activity', 'Activity'], ['settings', 'Settings'], ['sessions', null]]) {
  await bar.locator(`[data-destination=${id}]`).click();
  t.check((await t.destination()) === id, `the bar did not reach ${id}`);
  t.check((await bar.locator(`[data-destination=${id}]`).getAttribute('aria-current')) === 'page', `${id} is not marked as the current page`);
  if (heading) await page.getByRole('heading', { name: heading, exact: true }).waitFor();
  t.check((await t.overflow()) <= 0, `${id} overflows sideways`);
}
t.check((await t.smallTargets('[data-shell=bottom-bar]')).length === 0, `bottom bar targets under 44 pt: ${await t.smallTargets('[data-shell=bottom-bar]')}`);
const boxes = await bar.locator('button').evaluateAll((els) => els.map((e) => e.getBoundingClientRect()).map((r) => [r.left, r.right, r.bottom]));
t.check(boxes.length === 4 && boxes.every((b, i) => i === 0 || b[0] - boxes[i - 1][1] >= 7.5), 'the bar buttons are closer than 8 pt');
t.check(boxes.every((b) => b[2] <= 844), 'the bar is outside the screen');
t.ok('all four destinations are reachable by the bottom bar at 390 px');

// 2. The address bar follows, and Back goes to the previous destination.
await bar.locator('[data-destination=activity]').click();
await page.goBack();
t.check((await t.destination()) === 'sessions', `Back went to ${await t.destination()}, not sessions`);
await page.goForward();
t.check((await t.destination()) === 'activity', 'Forward did not return to activity');
await page.goto(t.url + '#/settings');
await page.getByRole('heading', { name: 'Settings', exact: true }).waitFor();
t.ok('the destination is in the address bar and survives Back, Forward and a reload');
await t.open();

// 3. The bar goes away under the soft keyboard in a session, only there.
await page.locator('[role=tab][data-key="alpha/svc-0"]').click();
t.check(await bar.isVisible(), 'the bar is missing in a session with the keyboard closed');
await t.api('keyboard', true);
await bar.waitFor({ state: 'hidden' });
await t.api('keyboard', false);
await bar.waitFor();
await bar.locator('[data-destination=home]').click();
await t.api('keyboard', true);
t.check(await bar.isVisible(), 'the bar hid under the keyboard outside a session');
await t.api('keyboard', false);
await bar.locator('[data-destination=sessions]').click();
t.ok('the bar is hidden while the keyboard is open inside a session, and only then');

// 4. Ordinary keys reach the terminal; the palette (hardware keyboard) opens a session by name and Escape restores focus.
await t.ordinaryKeysReachTerminal();
await t.paletteOpensSessionByName();
await t.escapeRestoresFocus();

// 5. The in-session tab switch and the strip by keyboard.
await t.inSessionTabSwitch();
await t.stripKeyboard();

// Steps 6 and 7 send raw touch events through CDP, which only Chromium has.
if (ENGINE === 'chromium') {
  // 6. Nothing moves under a touch: a finger is down on a session card, a status update arrives, no control moves.
  await t.api('sheet', 'half');
  const sheet = page.locator('.session-sheet[data-height=half]');
  await sheet.waitFor();
  await t.api('setState', 'alpha', 'svc-9', 'idle'); // a known baseline
  await page.waitForTimeout(2300);
  const cards = '.session-sheet .content button';
  const before = await t.rects(cards);
  const first = await page.locator(cards).first().boundingBox();
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: Math.round(first.x + first.width / 2), y: Math.round(first.y + first.height / 2), id: 1 }] });
  await t.api('setState', 'alpha', 'svc-9', 'needs_input'); // would sort to the top of its group
  await page.waitForTimeout(500);
  const during = await t.rects(cards);
  t.check(before.length > 5 && JSON.stringify(before) === JSON.stringify(during), 'a control moved while a touch was down and a status update arrived');
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  t.ok('no control moves when a status update arrives while a touch is down (session cards)');

  // 7. Swipe-back from the left edge in a session opens the session list (emulated touch events, not a device).
  await t.api('sheet', 'closed');
  await page.locator('[role=tab][data-key="alpha/svc-5"]').click();
  await page.locator('[data-shell=edge]').waitFor({ state: 'attached' });
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: 4, y: 400, id: 2 }] });
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: 90, y: 404, id: 2 }] });
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
  await page.locator('.session-sheet[data-height=half]').waitFor({ timeout: 3000 }).catch(() => {});
  t.check((await page.locator('.session-sheet[data-height=half]').count()) === 1, 'a swipe from the left edge did not open the session list');
  t.ok('a swipe from the left edge of a session opens the session list');
} else {
  t.ok(`skipped on ${ENGINE}: the held touch and the edge swipe need CDP touch events; Chromium covers them`);
}

await t.finish();
