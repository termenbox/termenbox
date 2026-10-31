// The New workspace dialog's folder picker against the REAL server binary and real tmux (roadmap 5.2): the scripted server in
// newworkspace.mjs proves the UI, this proves the wire contract. The server runs with HOME set to a private tree, the picker lists
// that tree's subdirectories (directories only, no files), a chosen folder becomes the new workspace's working directory
// (asked of tmux, not of the page), and a `list_dir` outside home is refused (that half is in run.mjs, over the raw socket).
// Every state is asserted before the next step; a missing element throws and the script exits non-zero.
//
// Usage: node e2e/newworkspace-real.mjs   (after: pnpm run build && pnpm run e2e:build; needs tmux and a built server)
//   TERMENBOX_BIN   server binary (default ../../target/release/termenbox-server)
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, existsSync, rmSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';
import { launch, viewport, contextOptions } from './browser.mjs';
import { freeTcpPort, freeUdpPort } from './ports.mjs';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
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
const mint = (dpk) => {
  const payload = Buffer.from(JSON.stringify({ v: 1, kid: 'e2e', sub: 1, login: 'alice', dpk, exp: Math.floor(Date.now() / 1000) + 3600 }));
  return `${payload.toString('base64url')}.${sign(null, payload, signer.privateKey).toString('base64url')}`;
};
const serverIdOf = (dir) => {
  const seed = readFileSync(join(dir, 'server_key'));
  const der = Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]);
  return createPublicKey(createPrivateKey({ key: der, format: 'der', type: 'pkcs8' })).export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
};
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.json': 'application/json', '.png': 'image/png', '.wasm': 'application/wasm' };
const appDist = resolve(here, '../dist');
const appServer = createServer((req, res) => {
  const path = req.url.split('?')[0];
  let file = join(appDist, path);
  if (!file.startsWith(appDist) || !existsSync(file) || path === '/') file = join(appDist, 'index.html');
  res.writeHead(200, { 'Content-Type': mime[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});

if (!existsSync(BIN)) throw new Error(`server binary not found: ${BIN}`);
if (!existsSync(join(appDist, 'index.html'))) throw new Error('client not built: run pnpm build');

// The server's whole world: a private tmux, a private config and a home whose subdirectories the picker must show.
const root = mkdtempSync(join(tmpdir(), 'termenbox-newws-'));
const home = realpathSync(root);
const userHome = join(home, 'user');
for (const d of ['code/termenbox', 'code/app', 'docs']) mkdirSync(join(userHome, d), { recursive: true });
writeFileSync(join(userHome, 'code', 'notes.txt'), 'a file is not a folder\n');
const httpPort = await freeTcpPort();
const claim = randomBytes(32).toString('base64url');
const env = {
  ...process.env,
  HOME: userHome,
  TMUX_TMPDIR: home,
  TERMENBOX_CONFIG_DIR: join(home, 'config'),
  TERMENBOX_HOST: '127.0.0.1',
  TERMENBOX_PORT: String(httpPort),
  TERMENBOX_UDP_PORT: String(await freeUdpPort()),
  TERMENBOX_PUBLIC_IP: '127.0.0.1',
  TERMENBOX_DIRECTORY_URL: '',
  TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`,
  TERMENBOX_CLAIM_SECRET: claim,
  LANG: 'C.UTF-8',
};
const tmux = (...args) => execFileSync('tmux', ['-L', 'termenbox', ...args], { env: { ...process.env, TMUX_TMPDIR: home }, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
/** Working directories of every pane of the server's tmux: what a workspace really started in. */
const paneDirs = () => { try { return tmux('list-panes', '-a', '-F', '#{pane_current_path}').split('\n').filter(Boolean); } catch { return []; } };

const appPort = await new Promise((r) => appServer.listen(0, '127.0.0.1', () => r(appServer.address().port)));
const server = spawn(BIN, [], { env, stdio: ['ignore', 'ignore', 'inherit'] });
let browser;
let failed = null;
try {
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
  browser = await launch();
  const ctx = await browser.newContext(contextOptions({ viewport: viewport('1280x800') }));
  const page = await ctx.newPage();
  page.setDefaultTimeout(20000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));

  // Sign in as the owner (as realserver.mjs does): a device key, a certificate for it, the server saved with its claim secret.
  await page.goto(`http://127.0.0.1:${appPort}/`);
  const dpk = await page.evaluate(async () => {
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
  const saved = [{ server_url: `ws://127.0.0.1:${httpPort}/ws`, server_id: serverIdOf(join(home, 'config')), server_name: 'one', registered_at: Date.now(), claim }];
  await page.evaluate(([cert, items]) => {
    localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login: 'alice' }));
    localStorage.setItem('termenbox_servers', JSON.stringify(items));
  }, [mint(dpk), saved]);
  await page.reload();

  const dialog = page.locator('dialog[aria-label="New workspace"]');
  const q = (id) => dialog.locator(`[data-testid="${id}"]`);
  const see = (locator, what) => locator.first().waitFor({ state: 'visible', timeout: 10000 }).catch(() => { throw new Error(`expected ${what} to be visible`); });
  const row = (name) => q('picker-list').locator(`[data-dir="${name}"]`);

  // Open the dialog from the sidebar's "+ New" (a desktop window, so there is a sidebar).
  await page.locator('[data-shell=sidebar]').waitFor();
  const plus = page.getByRole('button', { name: /^(\+ New|New workspace( on .*)?)$/ }).first();
  await see(plus, 'the + New button of a connected server');
  await plus.click();
  await see(dialog, 'the New workspace dialog');

  // The picker opens at the server user's real home: its subdirectories are listed, a file and the missing names are not.
  await q('folder-change').click();
  await see(q('picker-list'), 'the folder list');
  await see(row('code'), 'code in the real home');
  await see(row('docs'), 'docs in the real home');
  const root0 = await q('picker-list').locator('[data-dir]').evaluateAll((els) => els.map((e) => e.dataset.dir));
  if (root0.join() !== 'code,docs') throw new Error(`the real home lists ${root0.join()}, expected code,docs`);
  console.log('ok  the picker lists the real home\'s subdirectories');

  // Enter code: its folders show, its file does not; then termenbox is selected.
  await row('code').click();
  await see(row('termenbox'), 'termenbox inside code');
  const inCode = await q('picker-list').locator('[data-dir]').evaluateAll((els) => els.map((e) => e.dataset.dir));
  if (inCode.join() !== 'app,termenbox') throw new Error(`code lists ${inCode.join()} (a file must not appear), expected app,termenbox`);
  await row('termenbox').click();
  await page.waitForFunction(() => /termenbox/.test(document.querySelector('[data-testid="picker-select"] .path')?.textContent ?? ''));
  await q('picker-select').click();
  await see(q('folder-value'), 'the folder field');
  const shown = await q('folder-value').innerText();
  if (!/termenbox/.test(shown)) throw new Error(`the folder field reads "${shown}" after Select`);
  console.log('ok  entered code, no file listed, selected code/termenbox');

  // Create: the new workspace's directory, as tmux reports it, is the chosen path.
  const chosen = join(userHome, 'code', 'termenbox');
  const before = paneDirs().length;
  await dialog.getByRole('button', { name: 'Create' }).click();
  await dialog.waitFor({ state: 'detached', timeout: 10000 }).catch(() => { throw new Error('the dialog stayed open after Create'); });
  await waitFor(() => paneDirs().includes(chosen), `a tmux pane in ${chosen} (panes: ${paneDirs().join(', ') || 'none'}, before: ${before})`);
  console.log('ok  Create started the workspace in the chosen folder');

  if (errors.length) throw new Error(`console errors: ${errors.slice(0, 3).join(' | ')}`);
} catch (e) {
  failed = e;
  console.error(`FAIL: ${e instanceof Error ? e.stack : e}`);
} finally {
  await browser?.close().catch(() => {});
  server.kill();
  try { tmux('kill-server'); } catch {}
  appServer.close();
  rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
console.log(failed ? 'FAIL' : 'PASS');
process.exit(failed ? 1 : 0);
