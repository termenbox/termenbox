// #372: the needs-you banner floats over the terminal. When an agent starts waiting, the open terminal keeps its size
// (no resize reaches the server, so tmux does not repaint) and its scroll position, the banner is still answerable,
// a tap on a choice leaves the terminal focused, and x dismisses it. Real app against a scripted server (app-lib.mjs).
// Desktop Chromium: not an iPhone or Android check. Usage: node e2e/banner-resize.mjs (after: pnpm run build, and Chrome)
import { ScriptedServer, startApp, startShared } from './app-lib.mjs';

const shared = await startShared();
const failures = [];
try {
  const server = new ScriptedServer();
  server.states = { s1: 'idle', s2: 'idle', s3: 'idle' };
  const t = await startApp(shared, { viewport: { width: 1280, height: 800 }, server });
  await t.open();
  await t.openWorkspace('s3', { leaveTerminal: false });
  // Enough output that the viewport has somewhere to scroll to, then scrolled up a little.
  server.output(Array.from({ length: 120 }, (_, i) => `line ${i}\r\n`).join(''));
  await t.page.waitForTimeout(500);
  await t.page.locator('.xterm-viewport').evaluate((el) => { el.scrollTop = Math.max(0, el.scrollTop - 100); });
  await t.page.waitForTimeout(300);
  const measure = () => t.page.evaluate(() => {
    const v = document.querySelector('.xterm-viewport'), s = document.querySelector('.xterm-screen').getBoundingClientRect();
    const pane = document.querySelector('.terminal-pane').getBoundingClientRect();
    return { rows: document.querySelectorAll('.xterm-rows > div').length, screen: `${s.width}x${s.height}`, paneTop: pane.top, pane: pane.height, scrollTop: v.scrollTop, viewportY: Math.round(v.scrollTop / (s.height / document.querySelectorAll('.xterm-rows > div').length)) };
  });
  const before = await measure();
  const resizesBefore = server.of('resize').length;
  const lastBefore = server.of('resize').at(-1);

  // The agent starts waiting on a confirmed question.
  server.states.s1 = 'needs_input';
  server.broadcast(server.snapshot());
  server.broadcast({ type: 'agent_events', events: [{ workspace: 's1', seq: 2, ts: Date.now(), source: 'hook', confidence: 'high', kind: 'prompt', id: 'p-s1', prompt_kind: 'choice', choices: ['Yes', 'No'] }] });
  const banner = t.page.getByRole('status').filter({ hasText: /needs you/i }).first();
  await banner.getByRole('button', { name: 'Yes', exact: true }).waitFor();
  await t.page.waitForTimeout(800); // a resize, if the banner caused one, has been sent by now
  const after = await measure();
  t.check(JSON.stringify(after) === JSON.stringify(before), `the banner changed the terminal: ${JSON.stringify(before)} -> ${JSON.stringify(after)}`);
  t.check(server.of('resize').length === resizesBefore, `the banner sent ${server.of('resize').length - resizesBefore} resize message(s); last before ${JSON.stringify(lastBefore)}, last now ${JSON.stringify(server.of('resize').at(-1))}`);
  t.ok(`the banner appeared over the terminal: size ${after.screen} / ${after.rows} rows, scroll ${after.scrollTop}, and ${resizesBefore} resize message(s) before and after`);

  // A tap on a choice answers and leaves the terminal focused.
  await t.page.locator('.xterm-helper-textarea').focus();
  await banner.getByRole('button', { name: 'Yes', exact: true }).click();
  t.check(server.of('answer').length === 1 && server.of('answer')[0].choice === 0, `the choice was not sent: ${JSON.stringify(server.of('answer'))}`);
  t.check(await t.page.evaluate(() => document.activeElement?.classList.contains('xterm-helper-textarea')), 'tapping a choice took the focus from the terminal');
  t.ok('a choice answers and the terminal keeps focus');

  // x dismisses.
  await banner.getByRole('button', { name: 'Dismiss' }).click();
  await t.page.getByRole('status').filter({ hasText: /needs you/i }).waitFor({ state: 'detached' }).catch(() => {});
  t.check((await t.page.getByRole('button', { name: 'Dismiss' }).count()) === 0, 'x did not dismiss the banner');
  t.check(JSON.stringify(await measure()) === JSON.stringify(before), 'dismissing the banner changed the terminal');
  t.ok('x dismisses the banner and the terminal is unchanged');
  if (!(await t.end())) failures.push('banner-resize');
} catch (e) {
  console.error(`FAIL: ${e.stack ?? e}`);
  failures.push('error');
} finally {
  await shared.stop();
}
if (failures.length) { console.error(`FAIL: ${failures.join(', ')}`); process.exit(1); }
console.log('PASS');
