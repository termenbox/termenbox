// Two real server processes behind the real app on a phone-sized browser (roadmap R03, plans G.10 and H.11).
// Real tmux (one private tmux server per termenbox server), a fake `aider` program so the windows look like agents.
// Usage: node e2e/two-servers.mjs   (needs a built server, a built client (pnpm build), a built harness (pnpm e2e:build) and Chrome)
//   TERMENBOX_BIN   server binary          (default ../../target/release/termenbox-server)
//   CHROME_PATH    Chrome/Chromium binary (default: Playwright's "chrome" channel)
// Not covered here (checked in e2e/navigation.mjs with injected states): the tab label frozen while a finger is down.
import { launch, contextOptions, tap } from './browser.mjs';
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, symlinkSync, writeFileSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
let failed = false;
const fail = (msg) => {
  console.error(`FAIL: ${msg}`);
  failed = true;
};
const ok = (msg) => console.log(`ok  ${msg}`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const listen = (server) => new Promise((r) => server.listen(0, '127.0.0.1', () => r(server.address().port)));
const randomPort = () => 30000 + Math.floor(Math.random() * 30000);
async function waitFor(fn, what, ms = 15000) {
  const start = Date.now();
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${what}`);
    await sleep(100);
  }
}

// Stand-in for the Pages function that issues certificates (as in run.mjs).
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
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const staticServer = (root) =>
  createServer((req, res) => {
    const path = req.url.split('?')[0];
    let file = join(root, path === '/' ? 'index.html' : path);
    if (!file.startsWith(root) || !existsSync(file)) file = join(root, 'index.html');
    res.writeHead(200, { 'Content-Type': mime[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
  });

if (!existsSync(BIN)) throw new Error(`server binary not found: ${BIN}`);
const appDist = resolve(here, '../dist');
const harnessDist = join(here, 'dist');
if (!existsSync(join(appDist, 'index.html'))) throw new Error('client not built: run pnpm build');
if (!existsSync(join(harnessDist, 'index.html'))) throw new Error('harness not built: run pnpm e2e:build');

const root = mkdtempSync(join(tmpdir(), 'termenbox-two-'));
// A fake agent: bash under the name `aider`, so the window's process name is an agent's and it waits on the terminal. (An agent name
// that is not installed: a shell profile may put the real one first on the PATH.), it prints a marker and stays in the foreground.
const fakeBin = join(root, 'bin');
mkdirSync(fakeBin);
symlinkSync('/bin/bash', join(fakeBin, 'aider'));
// Workspaces start the user's shell, whose profile may rewrite the PATH: a private HOME puts the fake agent first.
const homeDir = join(root, 'home');
mkdirSync(homeDir);
writeFileSync(join(homeDir, '.bashrc'), `export PATH=${fakeBin}:$PATH\nPS1='$ '\n`);
const repo = join(root, 'repo');
mkdirSync(repo);

const alice = { id: 1, login: 'alice' };
const servers = [];
async function startServer(label) {
  const home = join(root, label);
  mkdirSync(home);
  const httpPort = randomPort();
  const claim = randomBytes(32).toString('base64url');
  const proc = spawn(BIN, [], {
    env: {
      ...process.env,
      PATH: `${fakeBin}:${process.env.PATH}`,
      HOME: homeDir,
      TMUX_TMPDIR: home,
      TERMENBOX_CONFIG_DIR: join(home, 'config'),
      TERMENBOX_HOST: '127.0.0.1',
      TERMENBOX_PORT: String(httpPort),
      TERMENBOX_UDP_PORT: String(randomPort()),
      TERMENBOX_PUBLIC_IP: '127.0.0.1',
      TERMENBOX_DIRECTORY_URL: '',
      TERMENBOX_DEV_INFO: 'on',
      TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`,
      TERMENBOX_CLAIM_SECRET: claim,
    },
    stdio: ['ignore', 'ignore', 'inherit'],
  });
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), `${label} health`);
  const info = await (await fetch(`http://127.0.0.1:${httpPort}/rtc-info`)).json();
  const s = {
    label,
    home,
    proc,
    claim,
    record: { ip: info.ip, udp_port: info.port, fingerprint: info.fingerprint },
    serverId: serverIdOf(join(home, 'config')),
    tmux: (...args) => execFileSync('tmux', ['-L', 'termenbox', ...args], { env: { ...process.env, TMUX_TMPDIR: home }, encoding: 'utf8' }).trim(),
  };
  servers.push(s);
  return s;
}

const statics = staticServer(harnessDist);
const appServer = staticServer(appDist);
const webPort = await listen(statics);
const appPort = await listen(appServer);
let browser;
try {
  const [a, b] = [await startServer('alpha'), await startServer('beta')];
  browser = await launch(['--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection']);

  // The first connection to each server carries its claim secret and creates the workspaces, with a real agent window.
  const createWorkspaces = async (server, names) => {
    const page = await (await browser.newContext(contextOptions())).newPage();
    page.on('pageerror', (e) => fail(`page error: ${e.message}`));
    await page.goto(`http://127.0.0.1:${webPort}/`);
    const cert = mint(await page.evaluate(() => window.__e2e.deviceKey()), alice.id, alice.login);
    await page.evaluate((o) => window.__e2e.start(o), { record: server.record, cert, serverId: server.serverId, claim: server.claim });
    await waitFor(() => page.evaluate(() => window.__e2e.ready > 0), `${server.label} login`);
    for (const name of names) {
      await page.evaluate((o) => window.__e2e.send({ type: 'new_session', name: o.name, dir: o.dir, template: 'aider' }), { name, dir: repo });
    }
    await waitFor(() => page.evaluate((n) => window.__e2e.workspaces.length >= n, names.length), `${server.label} workspaces`);
    await page.close();
  };
  await createWorkspaces(a, ['api', 'web', 'docs']);
  await createWorkspaces(b, ['infra']);
  for (const s of [a, b]) {
    await waitFor(() => s.tmux('list-panes', '-a', '-F', '#{pane_current_command}').includes('aider'), `${s.label} agents`);
  }
  for (const [s, name] of [[a, 'api'], [a, 'web'], [a, 'docs'], [b, 'infra']]) s.tmux('send-keys', '-t', `=${name}:`, `READY-${name}`, 'Enter');

  // The real app on a phone with both servers saved.
  const ctx = await browser.newContext(contextOptions({ viewport: { width: 390, height: 800 }, hasTouch: true, isMobile: true }));
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto(`http://127.0.0.1:${appPort}/`);
  const deviceKey = await page.evaluate(async () => {
    const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
    await new Promise((done, no) => {
      const r = indexedDB.open('termenbox', 1);
      r.onupgradeneeded = () => r.result.createObjectStore('device');
      r.onsuccess = () => {
        const tx = r.result.transaction('device', 'readwrite');
        tx.objectStore('device').put(pair, 'key');
        tx.oncomplete = done;
        tx.onerror = () => no(tx.error);
      };
      r.onerror = () => no(r.error);
    });
    const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
    return btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  });
  await page.evaluate(
    ([cert, items]) => {
      localStorage.setItem('termenbox_settings', JSON.stringify({ renderer: 'dom' })); // the rows are text, so a blank frame can be seen
      localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login: 'alice' }));
      localStorage.setItem('termenbox_servers', JSON.stringify(items));
    },
    [mint(deviceKey, alice.id, alice.login), [a, b].map((s) => ({ server_url: '', server_id: s.serverId, server_name: s.label, registered_at: Date.now(), record: s.record }))],
  );
  await page.reload();

  // 1. Three workspaces in one repo, each running an agent, get distinct tabs; the second server's workspace is there too.
  await page.locator('[data-shell=bottom-bar] [data-destination=workspaces]').click(); // the app opens on Home (U05); the strip is on Workspaces
  const tabs = page.getByRole('tab');
  await waitFor(async () => (await tabs.count()) === 4, 'four tabs (3 + 1 on two servers)', 30000);
  const labels = await tabs.evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')));
  if (new Set(labels).size !== 4) fail(`tabs are not distinct: ${JSON.stringify(labels)}`);
  for (const name of ['api', 'web', 'docs']) if (!labels.some((l) => l.includes(name))) fail(`no tab names workspace ${name}: ${JSON.stringify(labels)}`);
  if (!labels.some((l) => l.includes('infra') && l.includes('beta'))) fail(`the second server's tab is missing: ${JSON.stringify(labels)}`);
  ok('three agent workspaces in one repo and one on a second server give four distinct tabs');

  const tab = (name) => page.locator(`[role=tab][aria-label*="${name}"]`).first();
  const menu = (name) => page.getByRole('dialog', { name: `Windows of ${name}` });
  await tab('api').click();
  await page.waitForFunction(() => document.querySelector('[role=tab][aria-selected=true]'));

  // 2. New window and window close reach the real tmux.
  const windowsOf = (s, name) => s.tmux('list-windows', '-t', `=${name}`, '-F', '#{window_index}').split('\n').filter(Boolean).length;
  await tab('api').click(); // the active tab opens its windows
  await menu('api').waitFor();
  await menu('api').getByRole('button', { name: '+ New window' }).click();
  await waitFor(() => windowsOf(a, 'api') === 2, 'a second tmux window');
  // The new window must stay the one on screen: attaching again used to jump back to the agent window.
  await sleep(1500);
  const activeWindow = () => a.tmux('list-windows', '-t', '=api', '-F', '#{window_index} #{window_active}').split('\n').map((l) => l.split(' ')).find((l) => l[1] === '1')[0];
  if (activeWindow() !== '1') fail(`the new window did not stay current: window ${activeWindow()} is`);
  await tab('api').click();
  await menu('api').waitFor();
  await menu('api').getByText('bash', { exact: false }).first().waitFor();
  const index = Number(a.tmux('list-windows', '-t', '=api', '-F', '#{window_index} #{window_active}').split('\n').map((l) => l.split(' ')).find((l) => l[1] === '1')[0]);
  await menu('api').getByRole('button', { name: `Close window ${index}` }).click();
  const confirmButton = menu('api').getByRole('button', { name: `Confirm closing window ${index}` });
  if (await confirmButton.count()) await confirmButton.click(); // an idle shell closes at once; a busy one asks
  await waitFor(() => windowsOf(a, 'api') === 1, 'the window to close');
  await page.keyboard.press('Escape');
  ok('+ New window and the per-window close act on the real tmux workspace');

  // 3. A workspace that needs you shows the banner without changing the tab the user is on.
  // The label ends with the agent's status, which moves by itself; the tab is told apart by what comes before it.
  const activeTab = async () => ((await page.locator('[role=tab][aria-selected=true]').getAttribute('aria-label')) ?? '').split(' · ').slice(0, -1).join(' · ');
  const activeBefore = await activeTab();
  const sendNeedsInput = (s, name) =>
    s.tmux(
      'new-window', '-d', '-t', `=${name}:`,
      `python3 -c 'import os,socket,json;c=socket.socket(socket.AF_UNIX);c.connect(os.environ["TERMENBOX_EVENT_SOCKET"]);c.send((json.dumps({"session":os.environ["TERMENBOX_SESSION"],"token":os.environ["TERMENBOX_EVENT_TOKEN"],"adapter":"claude","data":{"hook_event_name":"Notification","message":"Allow rm?"}})+"\\n").encode())'`,
    );
  sendNeedsInput(a, 'docs');
  await page.getByRole('status').filter({ hasText: /needs you/i }).first().waitFor({ timeout: 15000 });
  const activeAfter = await activeTab();
  if (activeAfter !== activeBefore) fail(`the banner changed the tab: ${activeBefore} -> ${activeAfter}`);
  ok('the needs-you banner appears without a tab change');

  // 4. A tab switch shows the last screen of the other workspace before the live repaint.
  await page.evaluate(() => {
    window.__previewSeen = false;
    new MutationObserver(() => {
      if (document.querySelector('.screen.previewing')) window.__previewSeen = true;
    }).observe(document.body, { subtree: true, attributes: true, childList: true });
  });
  // The finger goes down on the tab (the screen is fetched now, decision 0084), and lifts a moment later.
  await tab('web').dispatchEvent('pointerdown', { pointerId: 7, pointerType: 'touch', clientX: 100, clientY: 20 });
  await sleep(300); // under the 550 ms long press
  await tab('web').dispatchEvent('pointerup', { pointerId: 7, pointerType: 'touch', clientX: 100, clientY: 20 });
  await tab('web').click();
  await page.waitForFunction(() => document.querySelector('[role=tab][aria-selected=true]')?.getAttribute('aria-label')?.includes('web'));
  await page.waitForFunction(() => !!document.querySelector('.xterm-screen'), null, { timeout: 15000 });
  await sleep(500);
  if (!(await page.evaluate(() => window.__previewSeen))) fail('switching tabs never showed the preview of the other workspace');
  else ok('a workspace switch shows a snapshot before the live repaint');

  // 4b. Switching back to workspaces already visited never shows a blank screen: the saved screen is drawn in the
  // same write that clears the terminal, and the live repaint lands on the same cells.
  const switchTo = async (name) => {
    await tab(name).click();
    await page.waitForFunction((n) => document.querySelector('[role=tab][aria-selected=true]')?.getAttribute('aria-label')?.includes(n), name);
    await sleep(700);
  };
  await switchTo('docs');
  await switchTo('web'); // both workspaces have now been shown once
  await page.evaluate(() => {
    window.__blank = 0;
    window.__frames = 0;
    const tick = () => {
      const rows = document.querySelector('.xterm-rows');
      if (rows) {
        window.__frames++;
        if (!rows.textContent.trim()) window.__blank++;
      }
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await switchTo('docs');
  await switchTo('web');
  await switchTo('docs');
  await switchTo('web');
  const { blank, frames } = await page.evaluate(() => ({ blank: window.__blank, frames: window.__frames }));
  if (!frames) fail('no terminal rows to sample (the DOM renderer is not in use)');
  else if (blank) fail(`${blank} of ${frames} frames were blank while switching between visited workspaces`);
  else ok(`tab 1 - tab 2 - tab 1 shows no blank frame (${frames} frames sampled)`);

  // 5. Detach closes the tab and keeps the workspace; Close workspace confirms, then ends it.
  await tab('web').click();
  await menu('web').waitFor();
  await menu('web').getByRole('button', { name: 'Detach', exact: true }).click();
  await tab('web').waitFor({ state: 'detached' });
  if (!a.tmux('list-sessions', '-F', '#{session_name}').split('\n').includes('web')) fail('Detach ended the workspace');
  await tap(tab('docs'));
  await page.waitForFunction(() => document.querySelector('[role=tab][aria-selected=true]')?.getAttribute('aria-label')?.includes('docs'));
  await tab('docs').click();
  await menu('docs').waitFor();
  await menu('docs').getByRole('button', { name: 'Close workspace', exact: true }).click();
  await page.getByRole('dialog', { name: 'Workspace actions' }).getByRole('button', { name: 'Close workspace', exact: true }).click();
  await waitFor(() => !a.tmux('list-sessions', '-F', '#{session_name}').split('\n').includes('docs'), 'docs to end');
  if (!b.tmux('list-sessions', '-F', '#{session_name}').split('\n').includes('infra')) fail('the other server lost its workspace');
  ok('Detach keeps the workspace running; Close workspace asks and ends only that workspace');

  if (errors.length) fail(`page errors: ${errors.join(' | ')}`);
} catch (e) {
  fail(e instanceof Error ? e.stack : String(e));
} finally {
  await browser?.close();
  for (const s of servers) {
    s.proc.kill();
    try {
      s.tmux('kill-server');
    } catch {}
  }
  statics.close();
  appServer.close();
  // A killed server may still be writing into the directory for a moment.
  rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  process.exit(failed ? 1 : 0);
}
