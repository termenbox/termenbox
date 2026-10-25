// The in-workspace screen: the Decision / Terminal switch (U04 item 7) and the T1 timeline (U09), in the real app against a
// real termenbox-server and tmux. A Claude Code hook is simulated by running the server's own `--hook claude` command
// inside the workspace, with a transcript made from the recorded fixture under a private Claude config directory.
// Usage: node e2e/pane.mjs   (needs a built server, `pnpm run build` and `pnpm run e2e:build`, and Chrome)
import { launch, contextOptions } from './browser.mjs';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, existsSync, rmSync, mkdirSync, copyFileSync, statSync } from 'node:fs';
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
const home = mkdtempSync(join(tmpdir(), 'termenbox-pane-'));
const claudeDir = join(home, 'claude');
const transcript = join(claudeDir, 'projects/p/session.jsonl');
mkdirSync(join(claudeDir, 'projects/p'), { recursive: true });
copyFileSync(resolve(here, '../../server/testdata/transcripts/claude-session.jsonl'), transcript);
const outside = join(home, 'outside.jsonl');
copyFileSync(transcript, outside);
const httpPort = 30000 + Math.floor(Math.random() * 30000);
const claim = randomBytes(32).toString('base64url');
const server = spawn(BIN, [], {
  env: {
    ...process.env, TMUX_TMPDIR: home, TERMENBOX_CONFIG_DIR: join(home, 'config'), TERMENBOX_HOST: '127.0.0.1', TERMENBOX_PORT: String(httpPort),
    TERMENBOX_UDP_PORT: String(httpPort + 1), TERMENBOX_PUBLIC_IP: '127.0.0.1', TERMENBOX_DIRECTORY_URL: '', TERMENBOX_DEV_INFO: 'on',
    TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`, CLAUDE_CONFIG_DIR: claudeDir, LANG: 'C', LC_ALL: 'C', TERMENBOX_CLAIM_SECRET: claim,
  },
  stdio: ['ignore', 'inherit', 'inherit'],
});

let browser;
try {
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
  const info = await (await fetch(`http://127.0.0.1:${httpPort}/rtc-info`)).json();
  const record = { ip: info.ip, udp_port: info.port, fingerprint: info.fingerprint };
  const serverId = serverIdOf(join(home, 'config'));
  browser = await launch(['--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection']);

  // The harness page owns the server and types into the workspace (it plays the agent's shell).
  const hp = await (await browser.newContext(contextOptions())).newPage();
  hp.on('pageerror', (e) => fail(`harness page error: ${e.message}`));
  await hp.goto(`http://127.0.0.1:${webPort}/`);
  const cert = mint(await hp.evaluate(() => window.__e2e.deviceKey()), 1, 'alice');
  await hp.evaluate((o) => window.__e2e.start(o), { record, cert, serverId, claim });
  await waitFor(() => hp.evaluate(() => window.__e2e.state === 'connected'), 'harness connected');
  await hp.evaluate(() => window.__e2e.send({ type: 'new_session', name: 's1' }));
  await waitFor(() => hp.evaluate(() => (window.__e2e.send({ type: 'list_sessions' }), window.__e2e.workspaces.length > 0)), 'workspace listed');
  const id = await hp.evaluate(() => window.__e2e.workspaces[0].id);
  await hp.evaluate((id) => window.__e2e.send({ type: 'attach', session_id: id }), id);
  await sleep(500);
  const hook = async (path) => {
    const payload = JSON.stringify({ hook_event_name: 'SessionStart', session_id: 'x', transcript_path: path, cwd: '/tmp' });
    await hp.evaluate((cmd) => window.__e2e.type(cmd), `printf '%s' '${payload}' | ${BIN} --hook claude; echo hook-sent-$((1+1))\r`);
    await waitFor(() => hp.evaluate(() => window.__e2e.output.includes('hook-sent-2')), 'hook command ran');
    await hp.evaluate(() => window.__e2e.clear());
  };

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
  await page.locator('[data-shell=bottom-bar] [data-destination=workspaces]').click();
  const row = page.locator('[data-workspace-groups] button').filter({ has: page.locator('b', { hasText: /^s1$/ }) }).first();
  await row.waitFor({ timeout: 20000 });
  await row.click();
  await page.locator('.xterm').waitFor({ timeout: 20000 });

  const radios = page.getByRole('radiogroup', { name: 'Workspace view' }).getByRole('radio');
  check((await radios.count()) === 2, 'the switch has two options');
  check((await radios.evaluateAll((r) => r.filter((b) => b.tabIndex === 0).length)) === 1, 'the switch is one tab stop');
  check((await radios.evaluateAll((r) => r.every((b) => b.getBoundingClientRect().height >= 44))), 'the switch targets are 44 pt');
  check((await page.getByRole('radio', { name: 'Terminal' }).getAttribute('aria-checked')) === 'true', 'the terminal is the default');
  check(!(await page.locator('[data-testid=session-view]').count()), 'the workspace view is not shown by default');
  console.log('ok  the switch is a two-option, one-tab-stop control and the terminal is the default');

  // Switching keeps the very same xterm element mounted, with its size.
  await page.evaluate(() => { window.__xt = document.querySelector('.xterm'); window.__xtSize = document.querySelector('.xterm').getBoundingClientRect().height; });
  await page.getByRole('radio', { name: 'Decision' }).click();
  await page.locator('[data-testid=session-view]').waitFor();
  const kept = await page.evaluate(() => ({ same: window.__xt === document.querySelector('.xterm') && window.__xt.isConnected, h: document.querySelector('.xterm').getBoundingClientRect().height, size: window.__xtSize,
    hidden: getComputedStyle(document.querySelector('[data-pane=terminal]')).visibility }));
  check(kept.same, 'switching to Decision detached or rebuilt xterm');
  check(kept.hidden === 'hidden' && kept.h === kept.size, `the terminal was not kept hidden at its size: ${JSON.stringify(kept)}`);
  check(!(await page.locator('[data-testid=timeline]').count()), 'a timeline shows without hooks');
  await page.locator('[data-testid=tier]').waitFor();
  const tier = await page.locator('[data-testid=tier]').innerText();
  check(/Generic|Profile/.test(tier), `without hooks the view stays at T2 or T3: ${tier}`);
  check((await page.evaluate(() => localStorage.getItem('termenbox_pane_modes'))).includes('decision'), 'the choice is not kept on this device');
  console.log(`ok  Decision shows the workspace view (${tier.split('·')[0].trim()}) and the terminal stays mounted, hidden, at its size`);

  // A hook names the transcript: T1 with the timeline.
  await hook(transcript);
  await page.locator('[data-testid=timeline]').waitFor({ timeout: 15000 });
  const text = await page.locator('[data-testid=session-view]').innerText();
  for (const t of ['Fix the failing parser test', 'cargo test -p parser', 'Plan · 1 of 3 done', 'lib.rs, main.rs', 'all tests pass', 'Structured view']) check(text.includes(t), `the timeline lacks "${t}"`);
  console.log('ok  a hook turns the view into T1 and the timeline shows the transcript');

  // A path outside the Claude directory is never read: the view falls back.
  await hook(outside);
  await waitFor(async () => !(await page.locator('[data-testid=timeline]').count()), 'timeline gone for a path outside the Claude directory', 15000);
  check(/Generic|Profile/.test(await page.locator('[data-testid=tier]').innerText()), 'a refused path left the view at T1');
  console.log('ok  a transcript path outside the Claude directory gives no timeline');

  await page.getByRole('radio', { name: 'Terminal' }).click();
  await page.locator('[data-testid=session-view]').waitFor({ state: 'detached' });
  const back = await page.evaluate(() => ({ same: window.__xt === document.querySelector('.xterm') && window.__xt.isConnected, vis: getComputedStyle(document.querySelector('[data-pane=terminal]')).visibility }));
  check(back.same && back.vis === 'visible', `switching back rebuilt or hid the terminal: ${JSON.stringify(back)}`);
  check(errors.length === 0, `page errors: ${errors.slice(0, 3).join(' | ')}`);
  console.log('ok  switching back shows the same terminal');
  await ctx.close();
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
