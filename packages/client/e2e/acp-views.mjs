// The View choice and a restart in the real app against a real termenbox-server, with no acp-agents.toml: the server offers
// its built-in `claude` because a fake `claude-agent-acp` (a wrapper around e2e/fake-acp-agent.mjs) is on its PATH.
//   1. New session: the dialog offers Terminal (the default) and Agent view for Claude; one workspace is made in each, and
//      the choice is remembered for that agent.
//   2. The server restarts while an ACP turn is open (the agent is waiting for a permission): the workspace comes back by
//      session/load, says that the restart interrupted the turn, and the terminal workspace is untouched.
// Desktop Chromium: this is NOT an iPhone or an Android check (guardrail a-phone-check-names-its-platform).
// Usage: node e2e/acp-views.mjs   (needs a built server, `pnpm run build` and `pnpm run e2e:build`, node, tmux, and Chrome)
import { launch, contextOptions } from './browser.mjs';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, chmodSync, existsSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve, delimiter } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const fail = (msg) => { console.error(`FAIL: ${msg}`); process.exitCode = 1; };
const check = (c, m) => { if (!c) fail(m); };
const ok = (m) => console.log(`ok  ${m}`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const listen = (s) => new Promise((r) => s.listen(0, '127.0.0.1', () => r(s.address().port)));
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.json': 'application/json', '.png': 'image/png' };
const serve = (root) => createServer((req, res) => {
  let f = join(root, req.url.split('?')[0].split('#')[0]);
  if (!f.startsWith(root)) return res.writeHead(404).end();
  if (existsSync(f) && statSync(f).isDirectory()) f = join(f, 'index.html');
  if (!existsSync(f)) f = join(root, 'index.html');
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
const appServer = serve(resolve(here, '../dist'));
const harness = serve(join(here, 'dist'));
const appPort = await listen(appServer);
const webPort = await listen(harness);
const home = mkdtempSync(join(tmpdir(), 'termenbox-acp-views-'));
const configDir = join(home, 'config');
const bin = join(home, 'bin');
mkdirSync(configDir, { recursive: true });
mkdirSync(bin);
const agentLog = join(home, 'agent.log');
const work = join(home, 'work-dir');
mkdirSync(work);
// The adapter the owner "installed": no acp-agents.toml, no TERMENBOX_BACKEND; TERMENBOX_ACP=on because the default is off. The built-in `claude` profile
// runs this command from PATH.
writeFileSync(join(bin, 'claude-agent-acp'), `#!/bin/sh\nexec node "${join(here, 'fake-acp-agent.mjs')}" "$@"\n`);
chmodSync(join(bin, 'claude-agent-acp'), 0o755);
const httpPort = 30000 + Math.floor(Math.random() * 30000);
const claim = randomBytes(32).toString('base64url');
const serverEnv = {
  ...process.env, PATH: `${bin}${delimiter}${process.env.PATH}`, FAKE_ACP_LOG: agentLog, TMUX_TMPDIR: home, TERMENBOX_CONFIG_DIR: configDir, TERMENBOX_ACP: 'on',
  TERMENBOX_HOST: '127.0.0.1', TERMENBOX_PORT: String(httpPort), TERMENBOX_UDP_PORT: String(httpPort + 1), TERMENBOX_PUBLIC_IP: '127.0.0.1',
  TERMENBOX_DIRECTORY_URL: '', TERMENBOX_DEV_INFO: 'on', TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`, LANG: 'C', LC_ALL: 'C', TERMENBOX_CLAIM_SECRET: claim,
};
let server;
const startServer = async () => {
  server = spawn(BIN, [], { env: serverEnv, stdio: ['ignore', 'inherit', 'inherit'] });
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
};
/** The service goes away and comes back with the same identity, state files and tmux: what a container restart looks like. */
const restartServer = async () => {
  server.kill();
  await new Promise((r) => server.once('exit', r));
  await startServer();
};
const logged = () => (existsSync(agentLog) ? readFileSync(agentLog, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l)) : []);

let browser;
let shown;
try {
  await startServer();
  const info = await (await fetch(`http://127.0.0.1:${httpPort}/rtc-info`)).json();
  const record = { ip: info.ip, udp_port: info.port, fingerprint: info.fingerprint };
  const serverId = serverIdOf(configDir);
  browser = await launch(['--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection']);

  // The first device claims the server for the owner (as acp.mjs does); the page under test is a second device of the same owner.
  const hp = await (await browser.newContext(contextOptions())).newPage();
  hp.on('pageerror', (e) => fail(`harness page error: ${e.message}`));
  await hp.goto(`http://127.0.0.1:${webPort}/`);
  const hcert = mint(await hp.evaluate(() => window.__e2e.deviceKey()), 1, 'alice');
  await hp.evaluate((o) => window.__e2e.start(o), { record, cert: hcert, serverId, claim });
  await waitFor(() => hp.evaluate(() => window.__e2e.state === 'connected'), 'harness connected');

  const ctx = await browser.newContext(contextOptions({ viewport: { width: 1280, height: 800 } }));
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto(`http://127.0.0.1:${appPort}/`);
  const deviceKey = await page.evaluate(async () => {
    const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
    await new Promise((done, no) => {
      const r = indexedDB.open('termenbox', 1);
      r.onupgradeneeded = () => r.result.createObjectStore('device');
      r.onsuccess = () => { const tx = r.result.transaction('device', 'readwrite'); tx.objectStore('device').put(pair, 'key'); tx.oncomplete = done; tx.onerror = () => no(tx.error); };
      r.onerror = () => no(r.error);
    });
    const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
    return btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  });
  await page.evaluate(([c, item]) => {
    localStorage.setItem('termenbox_ios_hint', '1');
    localStorage.setItem('termenbox_auth', JSON.stringify({ cert: c, login: 'alice' }));
    localStorage.setItem('termenbox_servers', JSON.stringify([item]));
  }, [mint(deviceKey, 1, 'alice'), { server_url: '', server_id: serverId, server_name: 'e2e-server', registered_at: Date.now(), record }]);
  await page.reload();

  shown = page;
  const dialog = page.locator('dialog[aria-label="New workspace"]');
  const viewOn = (id) => dialog.locator(`[data-testid=view-${id}]`).getAttribute('aria-checked').then((v) => v === 'true');
  const openDialog = async () => {
    // The button does nothing until the server is connected, so tap again until the sheet opens.
    await waitFor(async () => {
      if (await dialog.isVisible()) return true;
      await page.getByRole('button', { name: /^(\+ New|Start a workspace)$/ }).first().click({ timeout: 5000 }).catch(() => {});
      await sleep(500);
      return dialog.isVisible();
    }, 'the New workspace sheet', 40000);
    await dialog.locator('[data-testid=view]').waitFor({ timeout: 20000 });
  };
  const create = async (name) => {
    // The folder is chosen in the picker; the path bar takes the absolute path, then Select uses the listing.
    await dialog.locator('[data-testid=folder-change]').click();
    await dialog.locator('[data-testid=picker-path]').fill(work);
    await dialog.locator('[data-testid=picker-go]').click();
    await dialog.locator('[data-testid=picker-select]:not([disabled])').click();
    await dialog.locator('[data-testid=picker-path]').waitFor({ state: 'detached' });
    await dialog.getByLabel('Name').fill(name);
    await dialog.getByRole('button', { name: 'Create' }).click();
    await dialog.waitFor({ state: 'detached' });
  };

  // 1. The View choice. The server lists claude as an ACP agent because the adapter is on its PATH.
  await openDialog();
  check(!(await dialog.innerText()).includes('Manager'), 'the dialog still shows a Manager dropdown for tmux and acp');
  const text = await dialog.locator('[data-testid=view]').innerText();
  check(text.includes('Terminal') && text.includes('Agent view'), `the View control lacks its two choices: ${text}`);
  check(await viewOn('terminal'), 'the default view is not Terminal');
  await create('term-one');
  await page.locator('.xterm').first().waitFor({ timeout: 30000 });
  const termId = (await page.locator('[data-shell=sidebar] [data-key*="/tmux:"]').filter({ hasText: 'term-one' }).getAttribute('data-key', { timeout: 30000 })).split('/').pop();
  check(/^tmux:ws-[0-9a-f]{32}$/.test(termId), `the terminal workspace id is not a minted one: ${termId}`);
  ok('New session: with the adapter on PATH and no config, Claude offers Terminal (default) and Agent view; a Terminal workspace opens the agent\'s own terminal');

  await openDialog();
  check(await viewOn('terminal'), 'the view was not Terminal on the second open (nothing chosen yet)');
  await dialog.locator('[data-testid=view-acp]').click();
  check(await viewOn('acp'), 'a tap on Agent view does not select it');
  await create('agent-one');
  // The server minted the id; the sidebar row for the label carries it in its key (`<server>/<id>`).
  const agentRow = page.locator('[data-shell=sidebar] [data-key*="/acp:"]').filter({ hasText: 'agent-one' });
  const agentId = (await agentRow.getAttribute('data-key', { timeout: 30000 })).split('/').pop();
  check(/^acp:ws-[0-9a-f]{32}$/.test(agentId), `the workspace id is not a minted one: ${agentId}`);
  const agentView = page.locator(`[data-testid=acp-view][data-workspace$="/${agentId}"]`);
  await agentView.waitFor({ timeout: 30000 });
  check((await page.locator('[data-testid=tier]').innerText()) === 'T0 · ACP', 'the agent view does not say T0 · ACP');
  await openDialog();
  check(await viewOn('acp'), 'the last choice (Agent view) was not remembered for Claude on this server');
  await dialog.getByRole('button', { name: 'Cancel' }).click();
  await dialog.waitFor({ state: 'detached' });
  ok('New session: Agent view starts an ACP workspace in termenbox\'s own view, and the choice is remembered per agent');

  // 2. Restart the server during an open turn: "edit" makes the fake agent wait for a permission answer, so the turn stays open.
  const field = page.locator('textarea[aria-label=Prompt]');
  await field.fill('edit a.txt');
  await field.press('Enter');
  await page.getByTestId('stop').waitFor({ timeout: 20000 });
  await restartServer();
  await page.reload();
  // After the restart the same id comes back: a row with the label and that exact id.
  const row = page.locator(`[data-shell=sidebar] [data-key$="/${agentId}"]`).filter({ hasText: 'agent-one' });
  await row.waitFor({ timeout: 40000 });
  await row.click();
  await agentView.waitFor({ timeout: 30000 });
  const notice = (t) => page.locator('[data-kind=notice]').filter({ hasText: t });
  await notice('A server restart interrupted the running turn.').waitFor({ timeout: 40000 });
  await notice('Restored after a server restart').waitFor({ timeout: 10000 });
  check((await notice('A server restart interrupted the running turn.').count()) === 1, 'the interruption is said more than once');
  check(logged().some((m) => m.method === 'session/resume' || m.method === 'session/load'), `the agent was not asked to resume or load the workspace after the restart; it received: ${logged().map((m) => m.method).filter(Boolean).join(', ')}`);
  check((await page.getByTestId('stop').count()) === 0, 'the restored workspace still looks busy');
  ok('restart during an open turn: the workspace comes back by session/resume (or load) and says the restart interrupted the running turn, once');

  // The terminal workspace is untouched: still listed, still attachable.
  const termRow = page.locator(`[data-shell=sidebar] [data-key$="/${termId}"]`).filter({ hasText: 'term-one' });
  await termRow.waitFor({ timeout: 20000 });
  await termRow.click();
  await page.locator('.xterm').first().waitFor({ timeout: 30000 });
  ok('restart: the terminal workspace is still there and attaches');
  check(errors.length === 0, `page errors: ${errors}`);
  await ctx.close();
} catch (e) {
  fail(e.stack ?? String(e));
  if (shown) console.error(`page text: ${(await shown.locator('body').innerText().catch(() => '?')).slice(0, 1500)}\nbuttons: ${await shown.locator('button').evaluateAll((b) => b.map((x) => x.innerText.trim() || x.ariaLabel).join(' | ')).catch(() => '?')}`);
} finally {
  await browser?.close().catch(() => {});
  server?.kill();
  appServer.close();
  harness.close();
  rmSync(home, { recursive: true, force: true });
}
if (process.exitCode) process.exit(process.exitCode);
console.log('PASS');
process.exit(0);
