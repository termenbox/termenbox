// U08: the decision card in real Chrome at phone width, on Home, in the inbox and in the session view. A needs_input is
// answered with one tap on an option, or with a typed reply, without opening the terminal; the card stays (and says it
// is waiting) until the server clears Attention. Desktop Chromium with mobile emulation: not an iPhone or Android check.
// Usage: node e2e/decision.mjs   (needs the e2e harness built: pnpm e2e:build; CHROME_PATH for the browser)
import { launch } from './browser.mjs';
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
const base = `http://127.0.0.1:${srv.address().port}`;
const browser = await launch();
const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const small = (page, sel) => page.$$eval(sel, (els) => els.filter((e) => e.getBoundingClientRect().height < 43.5 || e.getBoundingClientRect().width < 43.5).map((e) => e.getAttribute('aria-label') || e.textContent.trim().slice(0, 20)));
try {
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  const open = async (path, ready) => {
    const page = await ctx.newPage();
    page.on('pageerror', (e) => fail(`page error: ${e.message}`));
    page.on('console', (m) => { if (m.type() === 'error') fail(`console error: ${m.text()}`); });
    await page.goto(`${base}/${path}`);
    await page.waitForSelector(ready);
    return page;
  };

  // ---- Home
  const home = await open('home.html', '[data-home-tiles]');
  const card = home.getByRole('region', { name: 'Decision' });
  const text = await card.innerText();
  for (const w of ['task-4', 'Refactor part 4', 'Waiting 30 min', 'Run migration 4?', 'The agent is about to use Bash', 'pnpm test --filter client # 4', 'Structured', 'From the agent’s hooks', 'Yes, always'])
    if (!text.includes(w)) fail(`the Home card lacks "${w}": ${text}`);
  const buttons = await card.getByRole('button').allInnerTexts();
  if (buttons.some((b) => /approve all|allow all|always allow/i.test(b))) fail(`a standing rule or batch approve is offered: ${buttons}`);
  const tiny = await small(home, '[role=region][aria-label=Decision] button');
  if (tiny.length) fail(`card targets under 44 px: ${tiny}`);
  console.log('ok  Home card: task, wait, question, T1 step as text, tier label, options, reply field; no standing rule or batch approve; targets >= 44 px');

  // One tap on an option sends that option and nothing else; the card waits for the server.
  await home.evaluate(() => { window.__home.answers.length = 0; });
  await card.locator('[data-hero-options] button').nth(1).tap();
  const answers = await home.evaluate(() => window.__home.answers);
  if (!eq(answers, [{ type: 'answer', session: 'alpha/s4', prompt_id: 'p4', seq: 11, choice: 1 }])) fail(`answer: ${JSON.stringify(answers)}`);
  await home.waitForTimeout(600);
  if (!(await card.innerText()).includes('task-4')) fail('the card resolved before the server cleared Attention');
  if (!(await card.locator('[data-decision-status]').innerText()).includes('Waiting for the agent')) fail('the card does not say it is waiting');
  const disabled = await card.locator('[data-hero-options] button').evaluateAll((els) => els.map((e) => e.disabled));
  if (!disabled.every(Boolean)) fail('the options are still tappable while the answer is in flight');
  await card.locator('[data-hero-options] button').first().tap({ force: true }).catch(() => {});
  if ((await home.evaluate(() => window.__home.answers.length)) !== 1) fail('a second tap sent a second answer');
  console.log('ok  Home: one tap sends exactly that option; the card stays, says it waits, and cannot be answered twice');

  // The server clears Attention: now, and only now, the card resolves and the next decision takes its place.
  await home.evaluate(() => { const a = window.__home.agents()['alpha/s4']; window.__home.set('alpha/s4', { ...a, attention: undefined, prompt: undefined }); });
  await card.getByText('task-1', { exact: true }).waitFor({ timeout: 3000 });
  if ((await card.innerText()).includes('task-4')) fail('the old card is still shown after Attention cleared');
  console.log('ok  Home: the card resolves when Attention clears, and the next decision shows');

  // A typed reply from the card: text then Enter on the server (the message is `reply`), no terminal opened.
  const input = card.getByLabel('Write a reply');
  await input.waitFor();
  await input.fill('run the tests first');
  await card.getByRole('button', { name: 'Send', exact: true }).tap();
  const replies = await home.evaluate(() => window.__home.replies);
  if (!eq(replies, [{ type: 'reply', session: 'beta/s1', text: 'run the tests first' }])) fail(`reply: ${JSON.stringify(replies)}`);
  if ((await home.evaluate(() => window.__home.opened)).length) fail('the terminal was opened to answer');
  await home.waitForTimeout(600);
  if (!(await card.innerText()).includes('task-1')) fail('the card resolved on send, before Attention cleared');
  if (!(await card.locator('[data-decision-status]').innerText()).includes('Reply sent')) fail('the card does not confirm the reply');
  console.log('ok  Home: a typed reply goes to the card’s session as one line; the terminal stays closed and the card waits for Attention');

  // No answer from the server: after a few seconds the card says so and the options come back.
  await home.waitForFunction(() => /Nothing changed yet/.test(document.querySelector('[data-decision-status]').textContent), null, { timeout: 12000 });
  if ((await card.locator('[data-hero-options] button').first().isDisabled())) fail('the options stay disabled after the wait');
  console.log('ok  Home: with no change on the server the card says so and can be answered again');
  await home.close();

  // ---- Inbox
  const inbox = await open('decision.html?surface=inbox', 'dialog[open]');
  const icard = inbox.getByRole('region', { name: 'Decision' });
  const itext = await icard.innerText();
  for (const w of ['api', 'Move login to the session store', 'Waiting 4 min', 'Allow the edit?', 'src/auth/session.ts +12 -3', 'Structured']) if (!itext.includes(w)) fail(`the inbox card lacks "${w}"`);
  await icard.getByRole('button', { name: 'Yes', exact: true }).tap();
  if (!eq(await inbox.evaluate(() => window.__decision.sent), [{ type: 'answer', session: 'srv/api', prompt_id: 'p1', seq: 3, choice: 0 }])) fail('inbox answer');
  await inbox.waitForTimeout(400);
  if (await icard.count() !== 1) fail('the inbox card resolved before Attention cleared');
  await inbox.evaluate(() => window.__decision.clear());
  await icard.waitFor({ state: 'detached', timeout: 3000 });
  if (!(await inbox.locator('dialog').innerText()).includes('api')) fail('the answered item vanished from the inbox list');
  console.log('ok  Inbox: the open question is the same card; one tap answers it in place and it becomes a plain line when Attention clears');
  await inbox.close();

  // ---- Session view
  const sv = await open('decision.html?surface=session', '[data-testid=session-view]');
  const scard = sv.getByRole('region', { name: 'Decision' });
  const stext = await scard.innerText();
  for (const w of ['Allow the edit?', 'Waiting 4 min', 'Move login to the session store', 'The agent is about to use Edit', 'Structured']) if (!stext.includes(w)) fail(`the session card lacks "${w}"`);
  await scard.getByRole('button', { name: 'No', exact: true }).tap();
  if (!eq(await sv.evaluate(() => window.__decision.sent), [{ type: 'session_answer', session: 'api', prompt_id: 'p1', option: 2 }])) fail('session answer');
  await sv.waitForTimeout(400);
  if (await scard.count() !== 1) fail('the session card resolved before Attention cleared');
  await sv.getByLabel('Send a line').fill('thanks, carry on');
  await sv.getByRole('button', { name: 'Send', exact: true }).tap();
  const sent = await sv.evaluate(() => window.__decision.sent);
  if (!eq(sent[1], { type: 'reply', session: 'api', text: 'thanks, carry on' }) || sent.some((m) => m.terminal)) fail(`session reply: ${JSON.stringify(sent)}`);
  await sv.evaluate(() => window.__decision.clear());
  await scard.waitFor({ state: 'detached', timeout: 3000 });
  console.log('ok  Session view: the same card answers by option index (the server holds the keys), the composer replies, the card resolves when Attention clears');
} catch (error) {
  fail(error.stack ?? String(error));
} finally {
  await browser.close();
  srv.close();
}

process.exit(process.exitCode ?? 0);
