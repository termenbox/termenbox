// The hook offer (U10, decisions 0079 and 0106) in the real app against a real termenbox-server and tmux: the card on Home
// with the exact change, Not now / Never / the 14-day return, Enable writing exactly the previewed file, Settings showing
// and removing it, and the session reaching T1 once the hook entry that was written is run. A stand-in `claude` (a copy of
// `sleep` under that name) is what the server's process scan detects; the settings file lives in a private HOME.
// Usage: node e2e/hookoffer.mjs   (needs a built server, `pnpm run build` and `pnpm run e2e:build`, and Chrome)
import { launch, contextOptions } from './browser.mjs';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, existsSync, rmSync, mkdirSync, copyFileSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const fail = (msg) => { console.error(`FAIL: ${msg}`); process.exitCode = 1; };
const check = (c, m) => { if (!c) fail(m); };
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
async function waitFor(fn, what, ms = 15000) {
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
// A private HOME: the server writes the settings file there and nowhere else.
const root = mkdtempSync(join(tmpdir(), 'termenbox-hookoffer-'));
const userHome = join(root, 'home');
const claudeDir = join(userHome, '.claude');
const settingsFile = join(claudeDir, 'settings.json');
const transcript = join(claudeDir, 'projects/p/session.jsonl');
mkdirSync(join(claudeDir, 'projects/p'), { recursive: true });
copyFileSync(resolve(here, '../../server/testdata/transcripts/claude-session.jsonl'), transcript);
const ORIGINAL = `{
  "model": "opus",
  "hooks": {
    "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "my-linter"}]}]
  }
}
`;
writeFileSync(settingsFile, ORIGINAL);
const fakeBin = join(root, 'bin');
mkdirSync(fakeBin);
const configDir = join(root, 'config');
const opencodeDir = join(root, 'opencode-config');
const httpPort = 30000 + Math.floor(Math.random() * 30000);
const claim = randomBytes(32).toString('base64url');
const server = spawn(BIN, [], {
  env: {
    ...process.env, HOME: userHome, TMUX_TMPDIR: root, TERMENBOX_CONFIG_DIR: configDir, TERMENBOX_HOST: '127.0.0.1', TERMENBOX_PORT: String(httpPort),
    TERMENBOX_UDP_PORT: String(httpPort + 1), TERMENBOX_PUBLIC_IP: '127.0.0.1', TERMENBOX_DIRECTORY_URL: '', TERMENBOX_DEV_INFO: 'on',
    TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`, CLAUDE_CONFIG_DIR: claudeDir, OPENCODE_CONFIG_DIR: opencodeDir, LANG: 'C', LC_ALL: 'C', TERMENBOX_CLAIM_SECRET: claim,
  },
  stdio: ['ignore', 'inherit', 'inherit'],
});

let browser;
try {
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
  const info = await (await fetch(`http://127.0.0.1:${httpPort}/rtc-info`)).json();
  const record = { ip: info.ip, udp_port: info.port, fingerprint: info.fingerprint };
  const serverId = serverIdOf(configDir);
  browser = await launch(['--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection']);

  // The harness page plays the agent's shell: window 0 is a shell, window 1 runs the stand-in `claude`.
  const hp = await (await browser.newContext(contextOptions())).newPage();
  hp.on('pageerror', (e) => fail(`harness page error: ${e.message}`));
  await hp.goto(`http://127.0.0.1:${webPort}/`);
  const cert = mint(await hp.evaluate(() => window.__e2e.deviceKey()), 1, 'alice');
  await hp.evaluate((o) => window.__e2e.start(o), { record, cert, serverId, claim });
  await waitFor(() => hp.evaluate(() => window.__e2e.state === 'connected'), 'harness connected');
  await hp.evaluate(() => window.__e2e.send({ type: 'new_session', name: 's1' }));
  await waitFor(() => hp.evaluate(() => (window.__e2e.send({ type: 'list_sessions' }), window.__e2e.sessions.length > 0)), 'session listed');
  const id = await hp.evaluate(() => window.__e2e.sessions[0].id);
  await hp.evaluate((id) => window.__e2e.send({ type: 'attach', session_id: id }), id);
  await sleep(500);
  const typeLine = async (line, marker) => {
    await hp.evaluate(() => window.__e2e.clear());
    await hp.evaluate((l) => window.__e2e.type(l), `${line}; echo ${marker}-$((1+1))\r`);
    await waitFor(() => hp.evaluate((m) => window.__e2e.output.includes(`${m}-2`), marker), `${marker} ran`);
  };
  const selectWindow = async (index) => {
    await hp.evaluate(([id, index]) => window.__e2e.send({ type: 'select_window', session: id, index }), [id, index]);
    await sleep(700);
  };
  await typeLine(`ln -s "$(command -v sleep)" ${fakeBin}/claude`, 'linked');
  await hp.evaluate((id) => window.__e2e.send({ type: 'new_window', session: id }), id);
  await sleep(700);
  await hp.evaluate((l) => window.__e2e.type(l), `${fakeBin}/claude 3000\r`);

  // The real app, on a phone.
  const ctx = await browser.newContext(contextOptions({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true }));
  await ctx.addInitScript(() => localStorage.setItem('termenbox_ios_hint', '1'));
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
  const offer = page.locator('[data-hook-offer]');
  const goto = (name) => page.locator(`[data-shell=bottom-bar] [data-destination=${name}]`).click();
  const stored = () => page.evaluate(() => JSON.parse(localStorage.getItem('termenbox_hook_offers') ?? '{}'));
  const settings = () => readFileSync(settingsFile, 'utf8');

  // 1. First detection of Claude Code: one inline card with the exact change, nothing written yet.
  await offer.waitFor({ timeout: 30000 });
  check(await offer.getByRole('heading', { name: /Claude Code/ }).count() === 1, 'the card does not name Claude Code');
  check((await page.getByRole('dialog').count()) === 0, 'the offer is a modal');
  const shown = await offer.locator('pre').innerText();
  check(shown.includes('my-linter') && shown.includes('--hook claude') && shown.includes('"model": "opus"'), `the preview lacks the wrapped settings: ${shown.slice(0, 120)}`);
  check(settings() === ORIGINAL, 'showing the offer changed the settings file');
  for (const name of ['Enable', 'Not now', 'Never']) {
    const h = await offer.getByRole('button', { name, exact: true }).evaluate((b) => b.getBoundingClientRect().height);
    check(h >= 44, `${name} is ${h}px high, under 44`);
  }
  console.log('ok  Home shows one inline card for Claude Code with the exact change, and nothing is written');

  // 2. Not now: gone, and it stays gone across a reload; 15 days later it returns.
  await offer.getByRole('button', { name: 'Not now', exact: true }).click();
  await offer.waitFor({ state: 'detached' });
  await page.reload();
  await page.locator('[data-home-tiles]').waitFor();
  await sleep(2500);
  check((await offer.count()) === 0, 'Not now was offered again straight away');
  const key = Object.keys(await stored())[0];
  check(!!key && (await stored())[key].answer === 'later', `the answer is not kept per agent per server: ${JSON.stringify(await stored())}`);
  await page.evaluate(([k, at]) => localStorage.setItem('termenbox_hook_offers', JSON.stringify({ [k]: { answer: 'later', at, manual: false } })), [key, Date.now() - 15 * 86_400_000]);
  await page.reload();
  await offer.waitFor({ timeout: 30000 });
  console.log('ok  Not now hides the card across a reload and 14 days bring it back');

  // 3. Never: gone for good, reversible in Settings.
  await offer.getByRole('button', { name: 'Never', exact: true }).click();
  await offer.waitFor({ state: 'detached' });
  await page.evaluate(([k]) => { const o = JSON.parse(localStorage.getItem('termenbox_hook_offers')); o[k].at = Date.now() - 400 * 86_400_000; localStorage.setItem('termenbox_hook_offers', JSON.stringify(o)); }, [key]);
  await page.reload();
  await page.locator('[data-home-tiles]').waitFor();
  await sleep(2500);
  check((await offer.count()) === 0, 'Never was offered again');
  await goto('settings');
  const row = page.locator('[data-hook-row=claude]');
  await row.waitFor();
  check((await row.locator('[data-hook-status]').innerText()).includes('Never'), 'Settings does not say Never was chosen');
  await row.getByRole('button', { name: 'Offer on Home again' }).click();
  await goto('home');
  await offer.waitFor({ timeout: 15000 });
  console.log('ok  Never stops the offers and Settings turns them back on');

  // 4. Enable: the file on disk is exactly the text that was shown, the original is backed up and the change is journaled.
  const previewText = await offer.locator('pre').evaluate((p) => p.textContent);
  await offer.getByRole('button', { name: 'Enable', exact: true }).click();
  await offer.waitFor({ state: 'detached', timeout: 15000 });
  const written = settings();
  check(written === previewText, 'the file written is not the text that was previewed');
  check(JSON.parse(written).hooks.PreToolUse.length === 2 && written.includes('my-linter'), 'the existing hook was not kept beside ours');
  check(existsSync(join(configDir, 'claude-settings.backup')) && readFileSync(join(configDir, 'claude-settings.backup'), 'utf8') === ORIGINAL, 'the original settings were not backed up');
  check(existsSync(join(configDir, 'claude-hooks.json')), 'the change was not journaled');
  console.log('ok  Enable writes exactly the previewed file, with a backup and a journal entry');

  // 5. Settings shows it on. The hook entry that was written, run in the session, takes the view to T1.
  await goto('settings');
  await row.waitFor();
  check((await row.getAttribute('data-hook-state')) === 'connected' && (await row.locator('[data-hook-status]').innerText()) === 'On', 'Settings does not show the hooks on');
  const command = JSON.parse(written).hooks.SessionStart.flatMap((e) => e.hooks).map((h) => h.command).find((c) => c.includes('--hook claude'));
  await selectWindow(0);
  const payload = JSON.stringify({ hook_event_name: 'SessionStart', session_id: 'x', transcript_path: transcript, cwd: '/tmp' });
  await typeLine(`printf '%s' '${payload}' | ${command}`, 'hook-sent');
  await selectWindow(1);
  await goto('sessions');
  const sessionRow = page.locator('[data-session-groups] button').filter({ has: page.locator('b', { hasText: /^s1$/ }) }).first();
  await sessionRow.waitFor({ timeout: 20000 });
  await sessionRow.click();
  await page.locator('.xterm').waitFor({ timeout: 20000 });
  await page.getByRole('radio', { name: 'Decision' }).click();
  await page.locator('[data-testid=timeline]').waitFor({ timeout: 20000 });
  check((await page.locator('[data-testid=session-view]').innerText()).includes('Structured view'), 'the session did not reach T1');
  console.log('ok  the hook entry Enable wrote takes the session to T1');

  // 6. Remove from Settings restores the original bytes; Enable there shows the change first.
  await goto('settings');
  await row.waitFor();
  await row.getByRole('button', { name: 'Remove' }).click();
  await waitFor(async () => (await row.getAttribute('data-hook-state')) === 'not_connected', 'hooks removed in Settings');
  check(settings() === ORIGINAL, 'Remove did not restore the original settings');
  await row.getByRole('button', { name: 'Enable…' }).click();
  await row.locator('pre').waitFor();
  check(settings() === ORIGINAL, 'the preview in Settings changed the file');
  const again = await row.locator('pre').evaluate((p) => p.textContent);
  await row.getByRole('button', { name: 'Enable', exact: true }).click();
  await waitFor(async () => (await row.getAttribute('data-hook-state')) === 'connected', 'hooks enabled in Settings');
  check(settings() === again, 'Settings wrote something other than its preview');
  console.log('ok  Settings removes the hooks (original bytes back) and enables them again after showing the change');

  // 7. OpenCode's plugin takes the same Settings path: no process scan is needed, the row is offered per server.
  await goto('settings');
  const ocRow = page.locator('[data-hook-row=opencode]');
  await ocRow.waitFor();
  const ocPlugin = join(opencodeDir, 'plugins/termenbox.js');
  await ocRow.getByRole('button', { name: 'Enable…' }).click();
  await ocRow.locator('pre').waitFor();
  check(!existsSync(ocPlugin), 'the OpenCode preview changed the file');
  const ocPreview = await ocRow.locator('pre').evaluate((p) => p.textContent);
  await ocRow.getByRole('button', { name: 'Enable', exact: true }).click();
  await waitFor(async () => (await ocRow.getAttribute('data-hook-state')) === 'connected', 'OpenCode plugin enabled');
  check(readFileSync(ocPlugin, 'utf8') === ocPreview, 'the OpenCode plugin written is not the preview');
  await ocRow.getByRole('button', { name: 'Remove' }).click();
  await waitFor(async () => (await ocRow.getAttribute('data-hook-state')) === 'not_connected', 'OpenCode plugin removed');
  check(!existsSync(ocPlugin), 'Remove did not delete the OpenCode plugin');
  console.log('ok  Settings enables and removes the OpenCode plugin with its preview');
  check(errors.length === 0, `page errors: ${errors.slice(0, 3).join(' | ')}`);
  await ctx.close();
} catch (e) {
  fail(e.stack ?? String(e));
} finally {
  await browser?.close().catch(() => {});
  server.kill();
  appServer.close();
  harness.close();
  rmSync(root, { recursive: true, force: true });
}
if (process.exitCode) process.exit(process.exitCode);
console.log('PASS');
process.exit(0);
