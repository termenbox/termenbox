// Two real server processes behind the real app on a phone-sized browser (roadmap R03, plans G.10 and H.11).
// Real tmux (one private tmux server per termenbox server), a fake `aider` program so the windows look like agents.
// Usage: node e2e/two-servers.mjs   (needs a built server, a built client (pnpm build), a built harness (pnpm e2e:build) and Chrome)
//   TERMENBOX_BIN   server binary          (default ../../target/release/termenbox-server)
//   CHROME_PATH    Chrome/Chromium binary (default: Playwright's "chrome" channel)
// A workspace is a server record and each tab is a tmux window (decision 0147): workspaces are switched from the
// Workspaces page rows, a phone shows one tab plaque that opens the tab menu, and a wide screen shows the tab strip.
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
      await page.evaluate((o) => window.__e2e.send({ type: 'new_workspace', name: o.name, dir: o.dir, template: 'aider' }), { name, dir: repo });
    }
    await waitFor(() => page.evaluate((n) => window.__e2e.workspaces.length >= n, names.length), `${server.label} workspaces`);
    await page.close();
  };
  await createWorkspaces(a, ['api', 'web', 'docs']);
  await createWorkspaces(b, ['infra']);
  // Wait for the workspace panes to be alive (pane_dead == 0). The process name is unreliable: bash
  // invoked through the `aider` symlink may report a different name depending on the kernel and base image.
  for (const s of [a, b]) {
    await waitFor(() => {
      const cmds = s.tmux('list-panes', '-a', '-F', '#{pane_dead} #{pane_current_command}');
      return cmds.trim().split('\n').every((l) => l.startsWith('0 '));
    }, `${s.label} agents`);
  }
  // The server minted each id; the name typed is the @termenbox_name label.
  const idOf = (s, name) => s.tmux('list-sessions', '-F', '#{session_name}|#{@termenbox_name}').split('\n').map((l) => l.split('|')).find(([, label]) => label === name)?.[0] ?? fail(`no tmux session labelled ${name}`);
  for (const [s, name] of [[a, 'api'], [a, 'web'], [a, 'docs'], [b, 'infra']]) s.tmux('send-keys', '-t', `=${idOf(s, name)}:`, `READY-${name}`, 'Enter');

  // The real app on a phone with both servers saved.
  const ctx = await browser.newContext(contextOptions({ viewport: { width: 390, height: 800 }, hasTouch: true, isMobile: true }));
  const page = await ctx.newPage();
  const errors = [];
  // Closing a workspace disposes its terminal; xterm 5.3 can then throw from a queued renderer frame (the same
  // teardown throw Terminal.svelte guards). It fires on some runners only, so ignore that one library error and
  // keep the gate for errors from this app.
  const xtermTeardown = /^Cannot read properties of undefined \(reading 'dimensions'\)$/;
  page.on('pageerror', (e) => {
    if (xtermTeardown.test(e.message)) return;
    errors.push(e.message);
  });
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

  const bottom = (dest) => page.locator(`[data-shell=bottom-bar] [data-destination=${dest}]`);
  const plaque = () => page.getByTestId('tab-plaque');
  const menu = () => page.getByRole('dialog', { name: 'Tabs' });
  const newTabDialog = () => page.getByRole('dialog', { name: 'New tab' });
  const rows = () => page.locator('[data-workspace-groups] [data-row]');
  const rowFor = (name) => page.locator('[data-workspace-groups] [data-row]').filter({ hasText: name }).first();
  const windowsOf = (s, name) => s.tmux('list-windows', '-t', `=${idOf(s, name)}`, '-F', '#{window_index}').split('\n').filter(Boolean).length;
  const tabsOf = (s, name) => s.tmux('list-windows', '-t', `=${idOf(s, name)}`, '-F', '#{@termenbox_tab}|#{window_id}|#{window_index}').split('\n').filter(Boolean);
  const sessionsOf = (s) => s.tmux('list-sessions', '-F', '#{session_name}').split('\n').filter(Boolean);
  const openWorkspace = async (name) => {
    await bottom('workspaces').click();
    const row = rowFor(name);
    await row.waitFor();
    // The row also has the drag, rename and actions buttons either side; the one with the name opens it.
    await row.locator('button').filter({ hasText: name }).first().click();
    await page.waitForFunction((n) => document.querySelector('[data-testid=tab-plaque]')?.getAttribute('aria-label')?.includes(`Tabs of ${n}`), name);
  };
  const openMenu = async () => {
    await tap(plaque());
    await menu().waitFor();
  };
  const tabRow = (label) => menu().locator('.tab').filter({ hasText: label }).first();
  const switchTab = async (label) => {
    await openMenu();
    await tabRow(label).click();
    await menu().waitFor({ state: 'hidden' });
    await page.waitForFunction((l) => document.querySelector('[data-testid=tab-plaque]')?.getAttribute('aria-label')?.endsWith(`: ${l}`), label);
  };
  const currentTab = async () => ((await plaque().getAttribute('aria-label')) ?? '').split(': ')[1] ?? '';
  const terminalText = () => page.locator('.xterm-rows').innerText();
  const waitTerminal = async (text) => {
    try {
      await page.waitForFunction((t) => document.querySelector('.xterm-rows')?.textContent?.includes(t), text, { timeout: 15000 });
    } catch (e) {
      console.error(`terminal did not show ${text}: ${JSON.stringify(await terminalText())}`);
      throw e;
    }
  };

  // 1. Workspaces are the rows on the Workspaces page, from both real servers.
  await bottom('workspaces').click();
  await waitFor(async () => (await rows().count()) === 4, 'four workspace rows (3 + 1 on two servers)', 30000);
  const rowText = await rows().evaluateAll((els) => els.map((e) => e.textContent ?? ''));
  for (const name of ['api', 'web', 'docs']) if (!rowText.some((t) => t.includes(name))) fail(`no workspace row named ${name}: ${JSON.stringify(rowText)}`);
  if (!rowText.some((t) => t.includes('infra') && t.includes('beta'))) fail(`the second server's workspace row is missing: ${JSON.stringify(rowText)}`);
  ok('three agent workspaces in one repo and one on a second server give four rows');

  // 1b. A workspace row carries a cross next to its pencil: it asks first, naming the workspace and its server.
  const cross = rowFor('api').getByRole('button', { name: 'Close api', exact: true });
  if (!(await cross.count())) fail('the workspace row has no Close button next to the pencil');
  const actions = page.getByRole('dialog', { name: 'Workspace actions' });
  await cross.click();
  await actions.waitFor();
  if (!(await actions.getByText(/Close api on/).count())) fail('the confirmation does not name the workspace and its server');
  await actions.getByRole('button', { name: 'Cancel', exact: true }).click();
  await actions.waitFor({ state: 'hidden' });
  if ((await rows().count()) !== 4) fail('Cancel closed the workspace');
  ok('the workspace row cross asks first and names the workspace and its server');

  // 2. Opening a row shows one phone plaque for that workspace's tabs.
  await openWorkspace('api');
  if ((await plaque().count()) !== 1) fail('the phone has no single tab plaque');
  if ((await page.locator('[data-testid=tab-strip]').count()) !== 0) fail('the desktop tab strip is on the phone');
  const agentTab = await currentTab();
  if (!agentTab) fail('the plaque does not name the current tab');
  if ((await windowsOf(a, 'api')) !== 1) fail('api did not start with one tmux window');
  ok(`a phone shows one tab plaque (${agentTab}) for the open workspace`);

  // 3. A workspace that needs you shows the banner without changing the tab the user is on.
  const activeBefore = await currentTab();
  a.tmux(
    'new-window', '-d', '-t', `=${idOf(a, 'docs')}:`,
    // node instead of python3: node is guaranteed in the container and on the bare runner.
    `node -e 'const n=require("net");const c=n.connect(process.env.TERMENBOX_EVENT_SOCKET);c.write(JSON.stringify({session:process.env.TERMENBOX_SESSION,token:process.env.TERMENBOX_EVENT_TOKEN,adapter:"claude",data:{hook_event_name:"Notification",message:"Allow rm?"}})+"\\n");c.end()'`,
  );
  await page.getByRole('status').filter({ hasText: /needs you/i }).first().waitFor({ timeout: 15000 });
  if ((await currentTab()) !== activeBefore) fail(`the banner changed the tab: ${activeBefore} -> ${await currentTab()}`);
  ok('the needs-you banner appears without changing the current tab');

  // 4. The tab menu creates a tab in the open workspace; the server and tmux record it.
  const agentTabId = tabsOf(a, 'api')[0]?.split('|')[0];
  await openMenu();
  if (!(await menu().getByRole('button', { name: 'New tab in api', exact: true }).count())) fail('the tab menu has no + next to the workspace name');
  await menu().getByRole('button', { name: 'New tab in api', exact: true }).click();
  await newTabDialog().waitFor();
  const heading = (await newTabDialog().getByRole('heading').innerText()) ?? '';
  if (!heading.includes('New tab in api')) fail(`the new-tab dialog is not for api: ${heading}`);
  await newTabDialog().getByRole('button', { name: 'Create', exact: true }).click();
  await newTabDialog().waitFor({ state: 'hidden' });
  await waitFor(() => windowsOf(a, 'api') === 2, 'a second tmux window');
  await page.waitForFunction(() => document.querySelector('[data-testid=tab-plaque]')?.getAttribute('aria-label')?.endsWith(': Shell'));
  const shellTab = await currentTab();
  if (shellTab !== 'Shell') fail(`the new tab is not current: ${shellTab}`);
  const tabOptions = tabsOf(a, 'api');
  const tabIds = tabOptions.map((l) => l.split('|')[0]);
  if (tabIds.length !== 2 || tabIds.some((id) => !id) || new Set(tabIds).size !== 2) fail(`tmux has no distinct @termenbox_tab for both windows: ${JSON.stringify(tabOptions)}`);
  ok('the header + creates a tab in the current workspace and tmux carries @termenbox_tab');

  // 5. Switching tabs keeps the terminal, and each tab shows its own window.
  const shellWindow = tabOptions
    .map((l) => l.split('|'))
    .find(([tab]) => tab !== agentTabId)?.[1];
  a.tmux('send-keys', '-t', shellWindow, 'SHELL-MARK', 'Enter');
  await switchTab(agentTab);
  await waitTerminal('READY-api');
  await switchTab(shellTab);
  await waitTerminal('SHELL-MARK');
  if ((await page.locator('.xterm-rows').count()) !== 1) fail('switching tabs left no visible terminal');
  ok('switching tabs keeps the terminal and shows each tab’s own window');

  // 5b. Going back to a visited tab still shows what that window held; nothing is lost across switches.
  for (let i = 0; i < 3; i++) {
    await switchTab(agentTab);
    await waitTerminal('READY-api');
    await switchTab(shellTab);
    await waitTerminal('SHELL-MARK');
  }
  const settled = (await terminalText()).trim();
  if (!settled) fail('the terminal is blank after switching between visited tabs');
  else ok('returning to a visited tab keeps its terminal text');

  // 6. Deleting a tab frees its tmux window; the workspace keeps the other one.
  await openMenu();
  const shellRow = menu().locator('[data-row]').filter({ hasText: shellTab }).first();
  const rowBefore = await shellRow.boundingBox();
  await menu().getByRole('button', { name: `Actions for ${shellTab}`, exact: true }).click();
  const tabMenu = menu().getByRole('menu');
  await tabMenu.waitFor();
  await page.keyboard.press('Escape');
  await tabMenu.waitFor({ state: 'hidden' });
  if (!(await menu().isVisible())) fail('Escape closed the whole tab dialog instead of its row menu');
  await menu().getByRole('button', { name: `Actions for ${shellTab}`, exact: true }).click();
  await menu().getByRole('menuitem', { name: 'Delete tab…', exact: true }).click();
  const rowDuring = await shellRow.boundingBox();
  if (!rowBefore || !rowDuring || Math.abs(rowBefore.y - rowDuring.y) > 1 || Math.abs(rowBefore.height - rowDuring.height) > 1) {
    fail(`the row moved when its menu opened: ${JSON.stringify({ rowBefore, rowDuring })}`);
  }
  await menu().getByRole('menuitem', { name: 'Delete tab', exact: true }).click();
  await waitFor(() => windowsOf(a, 'api') === 1, 'the tab to close');
  await page.waitForFunction((l) => document.querySelector('[data-testid=tab-plaque]')?.getAttribute('aria-label')?.endsWith(`: ${l}`), agentTab);
  await menu().getByRole('button', { name: 'Close the tab menu' }).click().catch(() => {});
  if ((await currentTab()) !== agentTab) fail(`the remaining tab is not current: ${await currentTab()}`);
  ok('deleting a tab closes its tmux window and keeps the workspace');

  // 7. The second real server keeps its workspace across a switch; a workspace switch keeps the terminal text.
  await openWorkspace('infra');
  if (!sessionsOf(b).includes(idOf(b, 'infra'))) fail('the second server lost its workspace');
  if (!(await currentTab())) fail('the second server has no tab');
  await openWorkspace('api');
  await waitTerminal('READY-api');
  if (!sessionsOf(a).includes(idOf(a, 'api'))) fail('switching away ended the workspace');
  ok('workspace switches from the Workspaces page keep both servers’ terminals');

  // 8. A wide screen uses the tab strip; its + creates a tab in the current workspace, not a new workspace.
  await page.setViewportSize({ width: 1280, height: 900 });
  const strip = page.locator('[data-testid=tab-strip]');
  await strip.waitFor();
  if (await plaque().count()) fail('the phone plaque is still shown on a wide screen');
  const stripTabs = strip.getByRole('tab');
  await waitFor(async () => (await stripTabs.count()) === 1, 'one tab in the strip');
  const sessionsBefore = sessionsOf(a).length;
  await page.getByTestId('strip-new-tab').click();
  await newTabDialog().waitFor();
  if (!((await newTabDialog().getByRole('heading').innerText()) ?? '').includes('New tab in api')) fail('the strip + did not open a tab for the current workspace');
  await newTabDialog().getByRole('button', { name: 'Create', exact: true }).click();
  await newTabDialog().waitFor({ state: 'hidden' });
  await waitFor(async () => (await stripTabs.count()) === 2, 'two tabs in the strip');
  if (sessionsOf(a).length !== sessionsBefore) fail('the strip + started a new workspace instead of a tab');
  await waitFor(() => windowsOf(a, 'api') === 2, 'the strip tab in tmux');
  await stripTabs.filter({ hasText: agentTab }).click();
  await page.waitForFunction((l) => document.querySelector('[data-testid=tab-strip] [role=tab][aria-selected=true]')?.textContent?.includes(l), agentTab);
  await stripTabs.filter({ hasText: 'Shell' }).click();
  await page.waitForFunction(() => document.querySelector('[data-testid=tab-strip] [role=tab][aria-selected=true]')?.textContent?.includes('Shell'));
  // The current tab opens the same menu; delete the tab there and leave the workspace alone. The strip is rebuilt
  // after the selected tab changes, so retry the click once the selected node is actually there.
  const currentTabButton = page.locator('[data-testid=tab-strip] [role=tab][aria-selected=true]');
  await currentTabButton.waitFor();
  await waitFor(async () => {
    if (await menu().isVisible()) return true;
    await currentTabButton.click();
    await page.waitForTimeout(150);
    return menu().isVisible();
  }, 'the tab menu', 5000);
  await menu().waitFor();
  await menu().getByRole('button', { name: 'Actions for Shell', exact: true }).click();
  await menu().getByRole('menuitem', { name: 'Delete tab…', exact: true }).click();
  await menu().getByRole('menuitem', { name: 'Delete tab', exact: true }).click();
  await waitFor(() => windowsOf(a, 'api') === 1, 'the strip tab to close');
  await menu().getByRole('button', { name: 'Close the tab menu' }).click();
  await waitFor(async () => (await stripTabs.count()) === 1, 'one tab in the strip again');
  ok('the desktop strip and its + act on the current workspace’s tabs, not on workspaces');

  // 9. Back on the phone, Close workspace still asks and ends only that workspace.
  // Capture the ids before the close: removing the tab/workspace can finish before the next tmux query.
  const [apiId, infraId] = [idOf(a, 'api'), idOf(b, 'infra')];
  await page.setViewportSize({ width: 390, height: 800 });
  await plaque().waitFor();
  await openMenu();
  await menu().getByRole('button', { name: 'Close workspace', exact: true }).click();
  await menu().getByRole('button', { name: 'Close workspace', exact: true }).click();
  await waitFor(() => !sessionsOf(a).includes(apiId), 'api to end');
  if (!sessionsOf(b).includes(infraId)) fail('the other server lost its workspace');
  ok('Close workspace asks and ends only that workspace, leaving the other server alone');

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
