// U07b: the ACP workspace view (T0, decision 0111) in the real app against a real termenbox-server, with a fake ACP agent built
// from the recorded captures (e2e/fake-acp-agent.mjs; the server starts it from its allowlist exactly as it starts a real
// agent). Covers Done-when: a permission answered from Home with one tap, an interrupt (Esc and the Stop key), search through
// the scrollback, and the view restored after a reload; and the rest of the step: the companion shell, a view that keeps its
// place when switching workspaces, streaming that does not move the reader, the keyboard guardrail, and a 360 px phone.
// Desktop Chromium with mobile emulation: this is NOT an iPhone or an Android check (guardrail a-phone-check-names-its-platform).
// Usage: node e2e/acp.mjs   (needs a built server, `pnpm run build` and `pnpm run e2e:build`, node, tmux, and Chrome)
import { launch, contextOptions, tap } from './browser.mjs';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const fail = (msg) => { console.error(`FAIL: ${msg}`); process.exitCode = 1; };
const check = (c, m) => { if (!c) fail(m); };
const ok = (m) => console.log(`ok  ${m}`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const listen = (s) => new Promise((r) => s.listen(0, '127.0.0.1', () => r(s.address().port)));
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.json': 'application/json', '.png': 'image/png' };
const serve = (root, spa) => createServer((req, res) => {
  let f = join(root, req.url.split('?')[0].split('#')[0]);
  if (!f.startsWith(root)) return res.writeHead(404).end();
  if (existsSync(f) && statSync(f).isDirectory()) f = join(f, 'index.html');
  if (!existsSync(f)) { if (!spa) return res.writeHead(404).end(); f = join(root, 'index.html'); }
  res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
});
async function waitFor(fn, what, ms = 20000) {
  const start = Date.now();
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${what}`);
    await sleep(100);
  }
}

const signer = generateKeyPairSync('ed25519');
const signerPublic = signer.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
const mint = (deviceKey, id, login) => {
  const payload = Buffer.from(JSON.stringify({ v: 1, kid: 'e2e', sub: id, login, dpk: deviceKey, exp: Math.floor(Date.now() / 1000) + 3600 }));
  return `${payload.toString('base64url')}.${sign(null, payload, signer.privateKey).toString('base64url')}`;
};
const serverIdOf = (configDir) => {
  const seed = readFileSync(join(configDir, 'server_key'));
  const der = Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]);
  return createPublicKey(createPrivateKey({ key: der, format: 'der', type: 'pkcs8' })).export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
};

if (!existsSync(BIN)) throw new Error(`server binary not found: ${BIN}`);
const appServer = serve(resolve(here, '../dist'), true);
const harness = serve(join(here, 'dist'), false);
const appPort = await listen(appServer);
const webPort = await listen(harness);
const home = mkdtempSync(join(tmpdir(), 'termenbox-acp-'));
const configDir = join(home, 'config');
mkdirSync(configDir, { recursive: true });
const agentLog = join(home, 'agent.log');
const work = join(home, 'work-dir');
mkdirSync(work);
// The owner's allowlist: the one agent this server will start, as the fake.
writeFileSync(join(configDir, 'acp-agents.toml'), `max_sessions = 3\n[agents.claude]\ncommand = "node"\nargs = ["${join(here, 'fake-acp-agent.mjs')}"]\nenv = { FAKE_ACP_LOG = "${agentLog}" }\n`);
const httpPort = 30000 + Math.floor(Math.random() * 30000);
const claim = randomBytes(32).toString('base64url');
const server = spawn(BIN, [], {
  env: {
    ...process.env, TMUX_TMPDIR: home, TERMENBOX_CONFIG_DIR: configDir, TERMENBOX_HOST: '127.0.0.1', TERMENBOX_PORT: String(httpPort), TERMENBOX_BACKEND: 'tmux,acp',
    TERMENBOX_UDP_PORT: String(httpPort + 1), TERMENBOX_PUBLIC_IP: '127.0.0.1', TERMENBOX_DIRECTORY_URL: '', TERMENBOX_DEV_INFO: 'on',
    TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`, LANG: 'C', LC_ALL: 'C', TERMENBOX_CLAIM_SECRET: claim,
  },
  stdio: ['ignore', 'inherit', 'inherit'],
});
const logged = () => (existsSync(agentLog) ? readFileSync(agentLog, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);

let browser;
try {
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
  const info = await (await fetch(`http://127.0.0.1:${httpPort}/rtc-info`)).json();
  const record = { ip: info.ip, udp_port: info.port, fingerprint: info.fingerprint };
  const serverId = serverIdOf(configDir);
  browser = await launch(['--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection']);

  // The harness page is a second device of the owner: it makes the workspaces and sends prompts the phone then shows.
  const hp = await (await browser.newContext(contextOptions())).newPage();
  hp.on('pageerror', (e) => fail(`harness page error: ${e.message}`));
  await hp.goto(`http://127.0.0.1:${webPort}/`);
  const cert = mint(await hp.evaluate(() => window.__e2e.deviceKey()), 1, 'alice');
  await hp.evaluate((o) => window.__e2e.start(o), { record, cert, serverId, claim });
  await waitFor(() => hp.evaluate(() => window.__e2e.state === 'connected'), 'harness connected');
  const send = (m) => hp.evaluate((msg) => window.__e2e.send(msg), m);
  const workspaces = async () => { await send({ type: 'list_workspaces' }); await sleep(150); return hp.evaluate(() => window.__e2e.workspaces); };
  // The server mints each id; the name typed is the label. `ids` maps a label to the id the server gave it.
  const ids = {};
  const findId = async (label, backend) => (await workspaces()).find((s) => s.name === label && s.id.startsWith(`${backend}:`))?.id;
  const createAcp = async (name) => {
    await send({ type: 'new_workspace', name, backend: 'acp', dir: work, template: 'claude' });
    await waitFor(async () => (ids[name] = await findId(name, 'acp')), `workspace ${name}`);
  };
  await createAcp('work');
  const listed = (await workspaces()).find((s) => s.id === ids.work);
  check(listed.backend === 'acp' && listed.agent === 'claude', `the listing does not say it is an ACP claude session: ${JSON.stringify(listed)}`);

  // ---- The owner's phone, on Home.
  const app = async (viewport, extra = {}) => {
    const ctx = await browser.newContext(contextOptions({ viewport, isMobile: true, hasTouch: true, ...extra }));
    await ctx.addInitScript(() => {
      localStorage.setItem('termenbox_ios_hint', '1');
      // Every peer connection, so a test can cut the transport the way a network drop would.
      window.__peers = [];
      const Native = window.RTCPeerConnection;
      // What the page receives: the bytes on the terminal channel, and each acp_view (its reset flag and entry ids).
      window.__termBytes = 0;
      window.__acp = [];
      const watch = (ch) => ch.addEventListener('message', (e) => {
        if (typeof e.data === 'string') {
          if (e.data.startsWith('{"type":"acp_view"')) { const m = JSON.parse(e.data); window.__acp.push({ reset: m.reset, ids: m.entries.map((x) => x.id), rev: m.rev }); }
        } else window.__termBytes += e.data.byteLength ?? e.data.size ?? 0;
      });
      window.RTCPeerConnection = class extends Native {
        constructor(...a) { super(...a); window.__peers.push(this); }
        createDataChannel(...a) { const ch = super.createDataChannel(...a); watch(ch); return ch; }
      };
    });
    const page = await ctx.newPage();
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    await page.goto(`http://127.0.0.1:${appPort}/`);
    const deviceKey = await page.evaluate(async () => {
      const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
      await new Promise((ok, no) => {
        const r = indexedDB.open('termenbox', 1);
        r.onupgradeneeded = () => r.result.createObjectStore('device');
        r.onsuccess = () => { const tx = r.result.transaction('device', 'readwrite'); tx.objectStore('device').put(pair, 'key'); tx.oncomplete = ok; tx.onerror = () => no(tx.error); };
        r.onerror = () => no(r.error);
      });
      const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
      return btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
    });
    await page.evaluate(([c, item]) => {
      localStorage.setItem('termenbox_auth', JSON.stringify({ cert: c, login: 'alice' }));
      localStorage.setItem('termenbox_servers', JSON.stringify([item]));
    }, [mint(deviceKey, 1, 'alice'), { server_url: '', server_id: serverId, server_name: 'e2e-server', registered_at: Date.now(), record }]);
    await page.reload();
    return { ctx, page, errors };
  };
  const phone = await app({ width: 390, height: 844 });
  const { page } = phone;
  await page.locator('[data-shell=bottom-bar] [data-destination=home]').waitFor();

  // From Home or the list a workspace opens by its row; from another open workspace by its tab, else from the Workspaces sheet
  // (after Refresh: a workspace made on another device is listed when asked for).
  const openWorkspace = async (p, name) => {
    // The Workspaces destination is the one workspace switcher on a phone now; the tab plaque switches tabs inside one.
    await p.locator('[data-shell=bottom-bar] [data-destination=workspaces]').click();
    await p.locator('[data-workspace-groups] button').filter({ has: p.locator('b', { hasText: new RegExp(`^${name}$`) }) }).first().click({ timeout: 20000 });
    await p.locator(`[data-testid=acp-view][data-workspace$="/${ids[name]}"]`).waitFor({ timeout: 20000 });
  };
  const prompt = (p) => p.locator('textarea[aria-label=Prompt]');
  const say = async (p, text) => {
    const field = prompt(p);
    await field.fill(text);
    await tap(p.getByRole('button', { name: 'Send', exact: true }));
  };
  const idle = (p) => p.waitForFunction(() => !document.querySelector('[data-testid=stop]'), null, { timeout: 30000 });
  const streamText = (p) => p.locator('[data-testid=stream]').innerText();

  // 1. Home: a permission from the agent, answered with one tap, no other screen opened.
  await send({ type: 'acp_open', workspace: ids.work });
  await send({ type: 'acp_prompt', workspace: ids.work, text: 'edit a.txt' });
  const card = page.getByRole('region', { name: 'Decision' });
  await card.waitFor({ timeout: 20000 });
  await waitFor(async () => (await card.innerText()).includes('Write a.txt'), 'the Home card shows the tool'); // the card mounts before its text
  const cardText = await card.innerText();
  for (const w of ['work', 'Write a.txt', 'T0 · ACP', 'Yes', 'No']) check(cardText.includes(w), `the Home card lacks "${w}": ${cardText}`);
  check(!/always|all edits/i.test(cardText), `a standing rule is offered on the card: ${cardText}`);
  const options = await card.locator('[data-hero-options] button').allInnerTexts();
  check(options.length === 2, `the card offers ${options.length} options, not the agent's two one-time ones: ${options}`);
  await tap(card.locator('[data-hero-options] button').first()); // the one tap
  await waitFor(() => logged().some((m) => m.id >= 9000 && m.result?.outcome), 'the agent got the answer');
  const answers = logged().filter((m) => m.id >= 9000 && m.result);
  check(answers.length === 1 && answers[0].result.outcome.outcome === 'selected' && answers[0].result.outcome.optionId === 'allow-once', `the agent received ${JSON.stringify(answers)}, not exactly allow-once`);
  await card.waitFor({ state: 'detached', timeout: 15000 });
  ok('Home: the agent asks to write a.txt; one tap on the card answers with exactly the option id the agent offered ("allow-once"); no always-option on the card');

  // 2. The workspace view: the stream of that turn, with the diff, the tool status and the outcome of the permission.
  await openWorkspace(page, 'work');
  const tier = await page.locator('[data-testid=tier]').innerText();
  check(tier === 'T0 · ACP', `the tier label is "${tier}"`);
  await page.locator('[data-kind=message]').filter({ hasText: 'Created' }).waitFor({ timeout: 15000 });
  const text1 = await streamText(page);
  for (const w of ['edit a.txt', 'Write a.txt', 'Allowed once · Yes', 'Created', 'hi']) check(text1.includes(w), `the stream lacks "${w}": ${text1}`);
  const toolRow = page.locator('[data-kind=tool][data-status=completed]');
  check((await toolRow.count()) === 1, 'the Write tool is not one completed row');
  check((await toolRow.getAttribute('aria-label')).includes('edit') && (await toolRow.getAttribute('aria-label')).includes('done'), 'the tool row does not say its kind and status');
  // The diff: collapsed behind a 44 pt summary, opened by a tap, shows the added line.
  const diffBtn = toolRow.locator('[data-diff] button[aria-expanded]');
  check((await diffBtn.getAttribute('aria-expanded')) === 'false', 'a diff opens by itself');
  await tap(diffBtn);
  await toolRow.locator('[data-diff] pre').waitFor();
  check((await toolRow.locator('[data-diff] pre').innerText()).includes('+ hi'), 'the diff does not show the added line');
  check(await page.locator('[data-testid=context]').isVisible(), 'the context meter is missing');
  const mode = page.locator('[data-testid=mode]');
  check((await mode.inputValue()) === 'default', 'the mode is not the agent\'s current one');
  check((await mode.locator('option').allInnerTexts()).join() === 'Manual,Accept edits,Plan,Auto,Bypass permissions', 'the modes are not the agent\'s own list');
  ok('workspace view: tier label T0 · ACP, the stream of the turn (prompt, tool with kind and status, diff, outcome of the permission, reply), context meter, the agent\'s own modes');

  // 2b. A permission that arrives while the view is open is the same card as on Home, with the diff in it; the rejected
  // path ends as the agent recorded it.
  await say(page, 'edit again');
  const inline = page.locator('[data-testid=stream]').getByRole('region', { name: 'Decision' });
  await inline.waitFor({ timeout: 15000 });
  const itext = await inline.innerText();
  for (const w of ['Write a.txt', 'Wants to change a file', 'T0 · ACP', 'Yes', 'No', '+ hi']) check(itext.includes(w), `the inline card lacks "${w}": ${itext}`);
  check(!/always|all edits/i.test(itext), `a standing rule is offered inline: ${itext}`);
  check((await page.locator('[data-testid=stream] [data-diff]').count()) === 2, 'the diff is not shown exactly once while the permission is open (one in the card, one in the earlier, finished tool)');
  const bigEnough = await inline.locator('[data-hero-options] button').evaluateAll((b) => b.every((x) => x.getBoundingClientRect().height >= 43.5));
  check(bigEnough, 'inline option buttons under 44 pt');
  await tap(inline.getByRole('button', { name: 'No', exact: true }));
  await page.locator('[data-kind=permission][data-state=answered]').filter({ hasText: 'Rejected · No' }).waitFor({ timeout: 15000 });
  await idle(page);
  check(logged().filter((m) => m.id >= 9000 && m.result?.outcome?.optionId === 'reject').length === 1, 'the agent did not receive exactly the rejecting option id');
  check((await page.locator('[data-kind=tool][data-status=failed]').count()) === 1, 'the refused write is not shown as failed');
  await say(page, 'shell ls /etc');
  await inline.waitFor({ timeout: 15000 });
  const shellText = await inline.innerText();
  check(shellText.includes('Wants to run a command') && shellText.includes('ls /etc') && !/always/i.test(shellText), `the shell request card is wrong: ${shellText}`);
  await tap(inline.getByRole('button', { name: 'Allow once' }));
  await page.locator('[data-kind=message]').filter({ hasText: 'Listed /etc.' }).waitFor({ timeout: 15000 });
  await idle(page);
  ok('inline permission: the same card as Home, with the diff in it, kind in words and only one-time options; the answer ends as the agent recorded it');

  // 3. Mode switch from the agent's list.
  await mode.selectOption('plan');
  await waitFor(() => logged().some((m) => m.method === 'session/set_mode' && m.params.modeId === 'plan'), 'session/set_mode with the listed id');
  await waitFor(async () => (await mode.inputValue()) === 'plan', 'the mode shows the new one');
  ok('mode switch: the agent\'s listed mode id goes out as session/set_mode and the view shows it');

  // 4. Slash commands from available_commands_update.
  const field = prompt(page);
  await tap(field);
  await field.fill('/des');
  const list = page.getByRole('listbox', { name: 'Commands' });
  await list.waitFor();
  check((await list.innerText()).includes('/design'), 'the agent\'s own /design is not offered');
  check((await list.locator('button').evaluateAll((b) => b.every((x) => x.getBoundingClientRect().height >= 43.5))), 'completion targets under 44 pt');
  await tap(list.getByRole('button', { name: /design/ }));
  check((await field.inputValue()) === '/design ', `choosing a command left "${await field.inputValue()}"`);
  await field.fill('');
  ok('slash commands: typing / offers the agent\'s own commands (with 44 pt targets) and a tap completes one');

  // 5. Interrupt: Esc in the field, then the visible Stop key.
  await say(page, 'stream');
  const stop = page.getByRole('button', { name: 'Stop the agent' });
  await stop.waitFor();
  await page.locator('[data-kind=message]').filter({ hasText: 'token-3' }).waitFor();
  check((await stop.boundingBox()).width >= 43.5, 'the Stop key is under 44 pt');
  await field.press('Escape');
  await page.locator('[data-kind=notice]').filter({ hasText: 'Interrupted' }).first().waitFor({ timeout: 15000 });
  await idle(page);
  check(logged().filter((m) => m.method === 'session/cancel').length === 1, 'Esc did not send exactly one session/cancel');
  await say(page, 'stream');
  await stop.waitFor();
  await page.locator('[data-kind=message]').filter({ hasText: 'token-3' }).last().waitFor();
  await tap(stop);
  await waitFor(async () => (await page.locator('[data-kind=notice]').filter({ hasText: 'Interrupted' }).count()) === 2, 'the second interrupt');
  await idle(page);
  check(logged().filter((m) => m.method === 'session/cancel').length === 2, 'the Stop key did not send session/cancel');
  // Esc with nothing focused (the page body) goes through the window listener, not the field's own key handling.
  await say(page, 'stream');
  await stop.waitFor();
  await page.evaluate(() => document.activeElement instanceof HTMLElement && document.activeElement.blur());
  check(await page.evaluate(() => document.activeElement === document.body), 'the focus is not on the page body');
  await page.keyboard.press('Escape');
  await waitFor(async () => (await page.locator('[data-kind=notice]').filter({ hasText: 'Interrupted' }).count()) === 3, 'the third interrupt, by Esc on the page body');
  await idle(page);
  check(logged().filter((m) => m.method === 'session/cancel').length === 3, 'Esc on the page body did not send exactly one more session/cancel');
  ok('interrupt: Esc in the field, Esc on the page body (window listener) and the visible Stop key each cancel a running turn (session/cancel), and the stream says "Interrupted"');

  // 6. Streaming does not move the reader: scroll up while a turn streams; what is on screen stays put, a Latest key appears.
  await say(page, 'stream');
  await stop.waitFor();
  await page.locator('[data-kind=message]').filter({ hasText: 'token-20' }).last().waitFor();
  const streamBox = page.locator('[data-testid=stream]');
  const lastLen = () => page.locator('[data-kind=message]').last().evaluate((e) => e.textContent.length);
  // Scrolled to the middle of a long stream (not the top: at the top nothing could ever move the reader).
  await streamBox.evaluate((e) => { e.scrollTop = Math.round((e.scrollHeight - e.clientHeight) / 2); });
  await sleep(300);
  const mid = await streamBox.evaluate((e) => ({ top: e.scrollTop, max: e.scrollHeight - e.clientHeight }));
  check(mid.top > 100 && mid.top < mid.max - 100, `the reader is not in the middle of the stream: ${JSON.stringify(mid)}`);
  const visibleEntry = () => page.locator('[data-entry]').evaluateAll((els) => { const box = document.querySelector('[data-testid=stream]').getBoundingClientRect(); const e = els.find((x) => x.getBoundingClientRect().bottom > box.top + 1); return { id: e.dataset.entry, top: Math.round(e.getBoundingClientRect().top - box.top) }; });
  const before = await visibleEntry();
  // While the hold lasts (a finger just scrolled) the text on screen does not change, though the turn goes on.
  const grew0 = await lastLen();
  await sleep(1000);
  check((await lastLen()) === grew0, 'the reply grew on screen during the hold after a scroll');
  const latest = page.getByTestId('latest');
  check((await latest.innerText()).includes('New output'), `the Latest key does not say there is new output: ${await latest.innerText()}`);
  await sleep(2600); // past the 2 s hold: what arrived is applied, with the reader's place kept
  check((await lastLen()) > grew0, 'the held output was never applied after the hold');
  const after6 = await visibleEntry();
  check(after6.id === before.id && Math.abs(after6.top - before.top) <= 1, `the stream moved under the reader while a turn streamed: ${JSON.stringify(before)} -> ${JSON.stringify(after6)}`);
  await latest.waitFor();
  await tap(latest);
  await waitFor(() => streamBox.evaluate((e) => e.scrollHeight - e.scrollTop - e.clientHeight < 60), 'Latest takes the view to the end');
  await tap(stop);
  await idle(page);
  ok('streaming: scrolled away from the end, the view holds still while the turn streams; Latest returns to the end');

  // 7. Scrollback with search. Many turns first, so the hit is far above the screen.
  await say(page, 'markdown');
  await page.locator('[data-kind=message]').filter({ hasText: 'Fixing the lexer' }).waitFor({ timeout: 15000 });
  await idle(page);
  for (let i = 1; i <= 8; i++) { await say(page, `note ${i}`); await page.locator('[data-kind=message]').filter({ hasText: `You said: note ${i}` }).waitFor(); await idle(page); }
  const mdLinks = await page.locator('[data-kind=message] a').evaluateAll((a) => a.map((x) => [x.getAttribute('href'), x.rel, x.target]));
  check(mdLinks.some(([h, r, t]) => h === 'https://example.com/spec' && r.includes('noopener') && t === '_blank'), `the link is not a safe web link: ${JSON.stringify(mdLinks)}`);
  check((await page.locator('[data-kind=message] figure pre').count()) >= 1 && (await page.locator('[data-kind=message] h1, [data-kind=message] [role=heading]').count()) >= 1, 'the markdown reply lacks its code block or heading');
  await tap(page.getByRole('button', { name: 'Search this workspace' }));
  const q = page.getByRole('searchbox', { name: 'Search this workspace' });
  await q.fill('lex');
  const count = page.getByTestId('matches');
  await waitFor(async () => (await count.innerText()).trim() === '1 / 3', 'three hits for "lex"');
  const visible = (sel) => page.locator(sel).first().evaluate((e) => { const r = e.getBoundingClientRect(); const s = document.querySelector('[data-testid=stream]').getBoundingClientRect(); return r.top >= s.top - 1 && r.bottom <= s.bottom + 1; });
  await waitFor(() => visible('mark[data-current]'), 'the first hit scrolls into view');
  await q.press('Enter');
  await waitFor(async () => (await count.innerText()).trim() === '2 / 3', 'next hit');
  await waitFor(() => visible('mark[data-current]'), 'the second hit scrolls into view');
  await tap(page.getByRole('button', { name: 'Next match' }));
  await waitFor(async () => (await count.innerText()).trim() === '3 / 3', 'third hit');
  await tap(page.getByRole('button', { name: 'Next match' }));
  await waitFor(async () => (await count.innerText()).trim() === '1 / 3', 'wrap-around to the first');
  await q.fill('zzzzqq');
  await waitFor(async () => (await count.innerText()).trim() === 'No matches', 'no matches is said');
  await q.press('Escape');
  await page.getByRole('searchbox').waitFor({ state: 'detached' });
  check((await page.locator('mark[data-hit]').count()) === 0, 'marks stay after the search closed');
  ok('scrollback: markdown reply (heading, list, code block, safe link), search counts hits, jumps to each in view, wraps, says "No matches" and clears');

  // 7b. A burst of 5000 chunks reaches the phone in a handful of updates, and the reply stays within its bound.
  const views = () => hp.evaluate(() => window.__e2e.received.filter((m) => m.startsWith('{"type":"acp_view"')).length);
  await send({ type: 'acp_open', workspace: ids.work });
  await sleep(300);
  const v0 = await views();
  const t0 = Date.now();
  // The page's own timer should keep ticking during the burst: the longest stall is the worst gap between two ticks.
  await page.evaluate(() => { window.__stall = 0; let last = performance.now(); window.__stallTimer = setInterval(() => { const n = performance.now(); window.__stall = Math.max(window.__stall, n - last - 50); last = n; }, 50); });
  // Three of them: the stream is then well over 64 KiB, the most the direct connection carries in one message, so a
  // reload below has to be sent in parts (a message above the limit would drop the connection).
  for (let i = 1; i <= 3; i++) {
    await say(page, 'burst');
    await page.locator('[data-kind=message]').filter({ hasText: 'w10 ' }).nth(i - 1).waitFor({ timeout: 30000 });
    await idle(page);
  }
  const sent = (await views()) - v0;
  const burst = await page.locator('[data-kind=message]').last().innerText();
  check(sent <= 60, `3 x 5000 chunks became ${sent} updates, not a batched few`);
  check(burst.length <= 16_100 && burst.includes('w0 '), `the burst reply is ${burst.length} characters`);
  const stall = await page.evaluate(() => { clearInterval(window.__stallTimer); return Math.round(window.__stall); });
  check(stall < 1000 && Date.now() - t0 < 25000, `the page stalled for ${stall} ms during the burst`);
  ok(`streaming: 3 x 5000 chunks in ${Date.now() - t0} ms reached the phone as ${sent} updates (longest page stall ${stall} ms); each reply is capped at ${burst.length} characters`);

  // 8. Plan progress from kilo's recorded todowrite calls.
  await say(page, 'plan');
  await page.locator('[data-kind=plan]').waitFor({ timeout: 15000 });
  const planText = await page.locator('[data-kind=plan]').innerText();
  check(/Plan · 0 of 3 done/.test(planText) && planText.includes('write a.txt') && planText.includes('run ls'), `the plan is not kilo's three todos: ${planText}`);
  await idle(page);

  // 9. Reload restores the view: the stream, the draft, and the place.
  await tap(field);
  await field.fill('half-typed prompt');
  await sleep(600);
  await streamBox.evaluate((e) => { e.scrollTop = Math.max(0, e.scrollHeight / 3); });
  await sleep(700); // the place is saved when scrolling stops
  const place = await page.locator('[data-entry]').evaluateAll((els) => { const box = document.querySelector('[data-testid=stream]').getBoundingClientRect(); const e = els.find((x) => x.getBoundingClientRect().bottom > box.top + 1); return { id: e.dataset.entry, top: Math.round(e.getBoundingClientRect().top - box.top) }; });
  await page.reload();
  await openWorkspace(page, 'work');
  await page.locator('[data-kind=user]').first().waitFor({ timeout: 20000 });
  const after = await waitFor(async () => {
    const r = await page.locator('[data-entry]').evaluateAll((els) => { const box = document.querySelector('[data-testid=stream]').getBoundingClientRect(); const e = els.find((x) => x.getBoundingClientRect().bottom > box.top + 1); return e ? { id: e.dataset.entry, top: Math.round(e.getBoundingClientRect().top - box.top) } : null; });
    return r && r.id === place.id ? r : null;
  }, 'the reader is back where they were');
  check(Math.abs(after.top - place.top) <= 3, `the place moved by ${after.top - place.top} px after the reload`);
  check((await prompt(page).inputValue()) === 'half-typed prompt', 'the unsent draft was lost by the reload');
  const text2 = await streamText(page);
  for (const w of ['edit a.txt', 'Allowed once · Yes', 'You said: note 3', 'Plan · 0 of 3 done']) check(text2.includes(w), `after the reload the stream lacks "${w}"`);
  check((await page.locator('[data-testid=tier]').innerText()) === 'T0 · ACP', 'no tier label after the reload');
  const big = await page.evaluate(() => document.querySelector('[data-testid=stream]').innerText.length);
  check(big > 48_000, `the reloaded stream is only ${big} characters: the multi-part reset was not exercised`);
  ok(`reload: the whole stream (${big} characters, sent in parts under the 64 KiB message limit) comes back from the server, with the unsent draft and the reader\'s place`);

  // 10. Companion shell: a plain tmux workspace in the workspace's directory, one tap, the same one the second time.
  const shellBtn = page.getByTestId('shell');
  check((await shellBtn.boundingBox()).height >= 43.5, 'the Shell key is under 44 pt');
  await tap(shellBtn);
  await page.locator('.xterm').waitFor({ timeout: 20000 });
  await waitFor(async () => (await workspaces()).some((s) => s.name === 'work-shell' && s.id.startsWith('tmux:')), 'the companion tmux workspace');
  await send({ type: 'attach', workspace_id: await findId('work-shell', 'tmux') });
  await sleep(500);
  await hp.evaluate(() => { window.__e2e.clear(); window.__e2e.type('pwd\r'); });
  await waitFor(() => hp.evaluate((d) => window.__e2e.output.includes(d), work), `the shell starts in ${work}`);
  await send({ type: 'detach' });
  await openWorkspace(page, 'work');
  await tap(page.getByTestId('shell'));
  await page.locator('.xterm').waitFor();
  check((await workspaces()).filter((s) => s.name === 'work-shell' && s.id.startsWith('tmux:')).length === 1, 'a second tap made a second shell');
  ok('companion shell: one tap opens a plain tmux workspace in the ACP workspace\'s directory; the next tap opens the same one');

  // 10b. Leaving a terminal for an ACP workspace lets the terminal go: its output stops reaching the phone. (Control first:
  // while the phone shows the shell, what the harness types into it does reach the phone.)
  await send({ type: 'attach', workspace_id: await findId('work-shell', 'tmux') });
  await sleep(500);
  const termBytes = () => page.evaluate(() => window.__termBytes);
  const t1 = await termBytes();
  await hp.evaluate(() => { window.__e2e.clear(); window.__e2e.type('echo control-one\r'); });
  await waitFor(async () => (await termBytes()) > t1, 'the shell\'s output reaches the phone while it shows the shell');
  await openWorkspace(page, 'work'); // from the shell to the ACP view
  await sleep(600);
  const t2 = await termBytes();
  await hp.evaluate(() => { window.__e2e.clear(); window.__e2e.type('echo after-switch\r'); });
  await waitFor(() => hp.evaluate(() => window.__e2e.output.includes('after-switch')), 'the shell ran the command');
  await sleep(800);
  check((await termBytes()) === t2, `the shell kept streaming to the phone after the ACP view opened (${(await termBytes()) - t2} bytes)`);
  await send({ type: 'detach' });
  await tap(page.getByTestId('shell')); // back to the shell, where the next step starts
  await page.locator('.xterm').waitFor();
  ok('leaving a terminal for an ACP workspace detaches it: the shell\'s output reaches the phone before, and stops after');

  // 11. Switching workspaces keeps the place.
  await createAcp('other');
  await openWorkspace(page, 'work');
  await streamBox.evaluate((e) => { e.scrollTop = 120; });
  await sleep(700);
  const keep = await streamBox.evaluate((e) => Math.round(e.scrollTop));
  await openWorkspace(page, 'other');
  check((await page.locator('[data-testid=acp-view]').getAttribute('data-workspace')).endsWith(`/${ids.other}`), 'did not switch to the other workspace');
  await openWorkspace(page, 'work');
  await page.locator('[data-kind=user]').first().waitFor();
  await waitFor(async () => Math.abs((await streamBox.evaluate((e) => Math.round(e.scrollTop))) - keep) <= 4, `the place after switching back (was ${keep}, now ${await streamBox.evaluate((e) => Math.round(e.scrollTop))}, saved ${await page.evaluate(() => localStorage.getItem('termenbox_acp_view'))})`);
  ok('switching workspaces and back keeps the reader\'s place');

  // 11b. The connection drops and comes back: the view says so, keeps what it shows, and catches up with only what it missed.
  const viewsBefore = await page.evaluate(() => window.__acp.length);
  await page.evaluate(() => window.__peers.forEach((p) => p.close()));
  await page.getByTestId('subtitle').filter({ hasText: 'Reconnecting' }).waitFor({ timeout: 15000 });
  check(/^Reconnecting/.test(await prompt(page).getAttribute('placeholder')), 'the prompt line does not say it is waiting for the connection');
  check((await streamText(page)).includes('You said: note 3'), 'the stream was cleared by the drop');
  await send({ type: 'acp_prompt', workspace: ids.work, text: 'said while away' });
  await page.locator('[data-kind=message]').filter({ hasText: 'You said: said while away' }).waitFor({ timeout: 40000 });
  check((await page.locator('[data-kind=user]').filter({ hasText: 'said while away' }).count()) === 1, 'the catch-up repeated an entry');
  check((await page.locator('[data-kind=user]').filter({ hasText: 'note 3' }).count()) === 1, 'the catch-up repeated older entries');
  // On the wire too: the server sent only what was missed (a handful of entries), never a reset of the whole stream.
  const caught = (await page.evaluate(() => window.__acp)).slice(viewsBefore);
  check(caught.length > 0 && caught.every((m) => !m.reset), `the catch-up was a reset: ${JSON.stringify(caught.map((m) => [m.reset, m.ids.length]))}`);
  check(caught.reduce((n, m) => n + m.ids.length, 0) <= 6, `the catch-up carried ${caught.reduce((n, m) => n + m.ids.length, 0)} entries, not only what was missed`);
  await sleep(4500); // the "Connected" toast covers the top bar for a moment
  ok('reconnect: the view says it is waiting, keeps what it shows, and catches up with what was said meanwhile, once');

  // 12. No page errors, no sideways overflow on the phone.
  check((await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)) <= 0, 'horizontal overflow at 390 px');
  check(phone.errors.length === 0, `page errors: ${phone.errors.slice(0, 3).join(' | ')}`);

  // 13. The agent dies: the view says so and the prompt line cannot be sent.
  await createAcp('doomed');
  await send({ type: 'acp_prompt', workspace: ids.doomed, text: 'crash' });
  await openWorkspace(page, 'doomed');
  await page.locator('[data-kind=notice][data-level=error]').waitFor({ timeout: 20000 });
  await waitFor(async () => (await page.getByTestId('subtitle').innerText()).startsWith('Stopped'), 'the header says the agent stopped');
  await waitFor(() => prompt(page).isDisabled(), 'the prompt line closes on a dead workspace');
  ok('a crashed agent: the view says it stopped, with an error line, and the prompt line is closed');
  await phone.ctx.close();

  // 14. The 360 px phone and the soft keyboard (a faked visual viewport, as the iPhone reports it).
  const small = await app({ width: 360, height: 740 }, {});
  await small.ctx.addInitScript(() => {
    // What iOS reports: the visual viewport shrinks for the keyboard and Safari pans it. Unset values follow the window.
    const set = {};
    const vv = new EventTarget();
    const read = { height: () => innerHeight, width: () => innerWidth, offsetTop: () => 0, offsetLeft: () => 0 };
    for (const k of Object.keys(read)) Object.defineProperty(vv, k, { get: () => set[k] ?? read[k]() });
    vv.scale = 1;
    Object.defineProperty(window, 'visualViewport', { value: vv, configurable: true });
    window.__vv = (patch, ev) => { Object.assign(set, patch); vv.dispatchEvent(new Event(ev)); };
  });
  await small.page.reload();
  await openWorkspace(small.page, 'work');
  const sp = small.page;
  check((await sp.evaluate(() => document.documentElement.scrollWidth - innerWidth)) <= 0, 'horizontal overflow at 360 px');
  // Every control is 44 pt each way, except the keys of the key row: 44 pt tall and a ninth of the width each, as in the terminal's row.
  const tiny = await sp.$$eval('[data-testid=acp-view] button, [data-testid=acp-view] select', (els) => els.filter((e) => e.getBoundingClientRect().width > 0 && !e.closest('[role=toolbar][aria-label="Prompt keys"]') && (e.getBoundingClientRect().height < 43.5 || e.getBoundingClientRect().width < 43.5)).map((e) => e.getAttribute('aria-label') || e.textContent.trim().slice(0, 20)));
  check(tiny.length === 0, `targets under 44 pt at 360 px: ${tiny}`);
  const narrow = await sp.$$eval('[role=toolbar][aria-label="Prompt keys"] button', (els) => els.filter((e) => e.getBoundingClientRect().height < 43.5 || e.getBoundingClientRect().width < 32).map((e) => e.getAttribute('aria-label')));
  check(narrow.length === 0, `key row keys under 44 pt tall or 32 pt wide: ${narrow}`);
  const dock = () => sp.evaluate(() => {
    const vv = window.visualViewport;
    const r = (s) => document.querySelector(s).getBoundingClientRect();
    return { field: r('textarea[aria-label=Prompt]'), keys: r('[role=toolbar][aria-label="Prompt keys"]'), header: r('[data-testid=acp-view] header'), viewTop: vv.offsetTop, viewBottom: vv.offsetTop + vv.height };
  });
  // In the visible area; with the keyboard open the dock is flush with it (nothing wasted, nothing covered).
  const within = (d) => d.field.top >= d.viewTop - 0.5 && d.keys.bottom <= d.viewBottom + 0.5 && d.header.top >= d.viewTop - 0.5;
  const flush = (d) => within(d) && d.viewBottom - d.keys.bottom <= 30;
  check(within(await dock()), `the prompt and key row are outside the view with no keyboard: ${JSON.stringify(await dock())}`);
  check((await dock()).field.height <= 50, `the prompt line is ${(await dock()).field.height} px tall when empty (its hint wraps?)`);
  await tap(sp.locator('textarea[aria-label=Prompt]'));
  await sp.evaluate(() => window.__vv({ height: 740 - 336 }, 'resize'));
  await sleep(300);
  check(flush(await dock()), `the keyboard covers the prompt or the key row: ${JSON.stringify(await dock())}`);
  await sp.evaluate(() => window.__vv({ height: 740 - 336 - 44, offsetTop: 180 }, 'scroll'));
  await sleep(300);
  check(flush(await dock()), `Safari's pan hides the prompt or the key row: ${JSON.stringify(await dock())}`);
  const rowKeys = await sp.$$eval('[role=toolbar][aria-label="Prompt keys"] button', (b) => b.map((x) => x.getAttribute('aria-label')));
  check(rowKeys.includes('Escape') && rowKeys.includes('Tab') && rowKeys.some((k) => /Arrow up/.test(k)) && rowKeys.includes('Slash'), `the key row lacks Esc, Tab, arrows or slash: ${rowKeys}`);
  await sp.evaluate(() => window.__vv({ height: 740, offsetTop: 0 }, 'resize'));
  ok('360 px phone: no sideways overflow, every control >= 44 pt, the prompt and key row stay in the visible area as the keyboard opens and Safari pans');
  await small.ctx.close();

  // 15. Desktop: the same view with a mouse and keyboard; no key row, Esc interrupts.
  const desk = await app({ width: 1280, height: 800 }, { isMobile: false, hasTouch: false });
  const dp = desk.page;
  const row = dp.locator(`[data-shell=sidebar] [data-key$="${ids.work}"]`);
  await row.waitFor({ timeout: 20000 });
  await row.click();
  await dp.locator('[data-testid=acp-view]').waitFor();
  check((await dp.locator('[role=toolbar][aria-label="Prompt keys"]').count()) === 0, 'a key row shows with a mouse and keyboard');
  const dfield = dp.locator('textarea[aria-label=Prompt]');
  await dfield.fill('stream');
  await dfield.press('Enter'); // Enter sends
  await dp.getByTestId('stop').waitFor();
  const cancels = logged().filter((m) => m.method === 'session/cancel').length;
  await dp.keyboard.press('Escape');
  await dp.locator('[data-kind=notice]').filter({ hasText: 'Interrupted' }).first().waitFor({ timeout: 15000 });
  check(logged().filter((m) => m.method === 'session/cancel').length === cancels + 1, 'Esc did not interrupt on the desktop');
  await dfield.fill('line one');
  await dfield.press('Shift+Enter');
  await dfield.type('line two');
  check((await dfield.inputValue()) === 'line one\nline two', 'Shift+Enter does not break the line');
  check((await dp.evaluate(() => document.documentElement.scrollWidth - innerWidth)) <= 0 && desk.errors.length === 0, `desktop overflow or errors: ${desk.errors}`);
  ok('desktop 1280 px: Enter sends, Shift+Enter breaks the line, Esc interrupts, no key row beside a physical keyboard');
  await desk.ctx.close();
} catch (e) {
  fail(e.stack ?? String(e));
} finally {
  await browser?.close().catch(() => {});
  server.kill();
  appServer.close();
  harness.close();
  rmSync(home, { recursive: true, force: true });
}
if (process.exitCode) process.exit(process.exitCode);
console.log('PASS');
process.exit(0);
