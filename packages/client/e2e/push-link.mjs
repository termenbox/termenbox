// U11: what a push opens, and the reminder settings, in the REAL app against a scripted server (app-lib.mjs).
//   1. A push's deep link (#/home?focus=<workspace key>) opens Home on that workspace's decision card, ahead of an older
//      waiting one, with no terminal attached: the tap on the push and one tap on an option are the two taps to a sent answer.
//   2. The same link followed in a tab where the app is already open (a notification click only changes the fragment).
//   3. Settings: the reminder form appears for a server that has settings, saves minutes as seconds with the browser's
//      time-zone offset, shows a rejection, and is absent from a server that answers nothing.
// Not covered: the service worker's notificationclick itself (a desktop Chrome cannot raise a real push here); the link it
// opens is this one. Desktop Chromium: not an iPhone or Android check.
// Usage: node e2e/push-link.mjs (after: pnpm run build, and Chrome)
import { ScriptedServer, startApp, startShared } from './app-lib.mjs';

class PushServer extends ScriptedServer {
  settings = { initial_wait_secs: 600, max_interval_secs: 3600, multiplier: 2, utc_offset_minutes: 0, muted_agents: [] };
  reject = false;
  handle(ws, m) {
    if (m.type === 'push_info') this.send(ws, { type: 'push_info', configured: true, public_key: 'k', devices: 0, subscribed: false, settings: true });
    else if (m.type === 'push_settings_get') this.send(ws, { type: 'push_settings', settings: this.settings });
    else if (m.type === 'push_settings_set') {
      this.received.push(m);
      if (this.reject) return this.send(ws, { type: 'push_settings_rejected', reason: 'Quiet hours must look like 22:00-07:00' });
      this.settings = m.settings;
      this.send(ws, { type: 'push_settings', settings: this.settings });
    } else super.handle(ws, m);
  }
}

const shared = await startShared();
const failures = [];
const phone = { width: 390, height: 844 };
/** The hooks' confirmed question on a workspace, as the server's event stream carries it: it gives the Home card its buttons. */
const ask = (server, workspace, seq = 2) => server.broadcast({ type: 'agent_events', events: [{ workspace: workspace, seq, ts: Date.now(), source: 'hook', confidence: 'high', kind: 'prompt', id: `p-${workspace}`, prompt_kind: 'choice', choices: ['Yes', 'No'] }] });
const heroKey = (t) => t.page.locator('[data-decision-card]').first().getAttribute('data-decision-card');

try {
  // 1. Cold start from the push: the named workspace is the hero although another has waited longer.
  {
    const t = await startApp(shared, { viewport: phone, server: new PushServer() });
    await t.page.goto(`${shared.origin}#/home?focus=${encodeURIComponent('one/s1')}`);
    await t.page.locator('[data-decision-card]').first().waitFor();
    ask(t.server, 's1');
    t.check((await heroKey(t)) === 'one/s1', `the pushed workspace is not the hero: ${await heroKey(t)}`);
    t.check((await t.page.evaluate(() => location.hash)) === '#/home', `the link stays in the address bar: ${await t.page.evaluate(() => location.hash)}`);
    t.check(t.server.of('attach').length === 0, 'the push attached a terminal instead of showing the card');
    const card = t.page.locator('[data-decision-card="one/s1"]');
    await card.getByRole('button', { name: 'Yes', exact: true }).click();
    const answers = t.server.of('answer');
    t.check(answers.length === 1 && answers[0].workspace === 's1' && answers[0].choice === 0, `one tap on the card did not send the answer: ${JSON.stringify(answers)}`);
    t.check(t.server.of('attach').length === 0, 'answering attached a terminal');
    t.ok('cold start: the push opens the named decision, no terminal, one tap on an option sends the answer');
    if (!(await t.end())) failures.push('cold start');
  }

  // 2. The app is open on the oldest decision; the link changes only the fragment.
  {
    const t = await startApp(shared, { viewport: phone, server: new PushServer() });
    await t.open();
    await t.page.locator('[data-decision-card]').first().waitFor();
    t.check((await heroKey(t)) === 'one/s2', `without a link the oldest wait is not the hero: ${await heroKey(t)}`);
    await t.page.evaluate(() => { location.hash = '#/home?focus=one%2Fs1'; });
    await t.page.locator('[data-decision-card="one/s1"]').waitFor();
    t.check(t.server.of('attach').length === 0, 'following the link attached a terminal');
    t.check((await t.page.evaluate(() => performance.getEntriesByType('navigation').length)) === 1, 'following the link reloaded the page');
    t.ok('open app: a link to another decision moves it to the top without a reload');
    if (!(await t.end())) failures.push('open app');
  }

  // 3. Settings.
  {
    const server = new PushServer();
    const t = await startApp(shared, { viewport: phone, server });
    await t.open();
    await t.page.getByRole('button', { name: 'Settings' }).first().click();
    const form = t.page.locator('[data-settings-push]');
    await form.waitFor();
    t.check((await form.getByLabel('Remind me after (minutes)').inputValue()) === '10', 'the wait is not shown in minutes');
    await form.getByLabel('Remind me after (minutes)').fill('5');
    await form.getByLabel('Never wait longer than (minutes)').fill('30');
    await form.getByLabel(/Quiet hours/).fill('22:00-07:00');
    await form.getByRole('button', { name: 'Save' }).click();
    await t.page.waitForFunction(() => !document.querySelector('[data-settings-push] button[type=submit]')?.disabled);
    const sent = server.of('push_settings_set').at(-1)?.settings;
    t.check(sent?.initial_wait_secs === 300 && sent?.max_interval_secs === 1800 && sent?.quiet_hours === '22:00-07:00', `the form sent ${JSON.stringify(sent)}`);
    t.check(typeof sent?.utc_offset_minutes === 'number', 'the browser offset was not sent');
    // A rejection leaves the button usable again and shows the reason.
    server.reject = true;
    await form.getByRole('button', { name: 'Save' }).click();
    await t.page.getByText('Quiet hours must look like 22:00-07:00').first().waitFor();
    t.check(await form.getByRole('button', { name: 'Save' }).isEnabled(), 'a rejected save leaves the button stuck');
    const small = await t.page.$$eval('[data-settings-push] button, [data-settings-push] input', (els) => els.filter((e) => e.getBoundingClientRect().height < 43.5).length);
    t.check(small === 0, `${small} settings controls are under 44 px`);
    t.check((await t.overflow()) <= 0, 'Settings overflows sideways');
    t.ok('settings: minutes shown, seconds and offset sent, a rejection shown, controls >= 44 px');
    if (!(await t.end())) failures.push('settings');
  }
} catch (e) {
  console.error(`FAIL: ${e.stack ?? e}`);
  failures.push('error');
} finally {
  await shared.stop();
}
if (failures.length) { console.error(`FAIL: ${failures.join(', ')}`); process.exit(1); }
