// Upload to the active tab's working folder against the REAL server binary and real tmux (plan 3.1, DECISION-0194): a file picked
// lands in the tab's folder with mode 0600; two files dropped with Replace and Keep both; a pasted image; a `cd` during an upload
// leaves the file in the pinned folder; Cancel leaves no partial; a dropped connection resumes; pasted text still reaches the terminal.
// Every state is asserted before the next step; a failed check throws and the script exits non-zero. Desktop only.
//
// Usage: node e2e/upload-real.mjs   (after: pnpm run build && pnpm run e2e:build; needs tmux and a built server)
//   TERMENBOX_BIN   server binary (default ../../target/release/termenbox-server)
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, readdirSync, statSync, existsSync, rmSync, realpathSync } from 'node:fs';
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

// The server's whole world: a private tmux, a private config and a home with two project folders.
const root = mkdtempSync(join(tmpdir(), 'termenbox-upload-'));
const home = realpathSync(root);
const userHome = join(home, 'user');
const projA = join(userHome, 'proj-a');
const projB = join(userHome, 'proj-b');
for (const d of [projA, projB]) mkdirSync(d, { recursive: true });
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
/** Files in a folder, hidden ones included: a leftover `.tb-upload-*.part` must show. */
const names = (dir) => readdirSync(dir).sort();
const fixtures = join(home, 'fixtures');
mkdirSync(fixtures);
const make = (name, content) => { const p = join(fixtures, name); writeFileSync(p, content); return p; };

const appPort = await new Promise((r) => appServer.listen(0, '127.0.0.1', () => r(appServer.address().port)));
const server = spawn(BIN, [], { env, stdio: ['ignore', 'ignore', 'inherit'] });
let browser;
let failed = null;
try {
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
  tmux('new-session', '-d', '-s', 'work', '-x', '100', '-y', '30', '-c', projA, `bash --norc -c 'PS1="$ "; echo READY-work; exec bash --norc -i'`);
  browser = await launch();
  const ctx = await browser.newContext(contextOptions({ viewport: viewport('1280x800') }));
  // Keep every socket the page opens, so a test can cut the connection the way a network does.
  await ctx.addInitScript(() => {
    const Real = window.WebSocket;
    window.__sockets = [];
    window.WebSocket = class extends Real { constructor(...a) { super(...a); window.__sockets.push(this); } };
  });
  const page = await ctx.newPage();
  page.setDefaultTimeout(20000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));

  // Sign in as the owner (as newworkspace-real.mjs does).
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
    localStorage.setItem('termenbox_settings', JSON.stringify({ renderer: 'dom' })); // rows are text, so the test can read the screen
    localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login: 'alice' }));
    localStorage.setItem('termenbox_servers', JSON.stringify(items));
    localStorage.setItem('termenbox_ios_hint', '1');
  }, [mint(dpk), saved]);
  await page.reload();

  const sheet = page.locator('dialog[aria-label="Upload files"]');
  const item = (name) => sheet.locator('li').filter({ hasText: name });
  const screen = () => page.locator('.xterm-rows').innerText();
  const input = page.locator('textarea[aria-label="Terminal input"]');
  const see = (locator, what, timeout = 10000) => locator.first().waitFor({ state: 'visible', timeout }).catch(async () => { throw new Error(`expected ${what} to be visible; the sheet reads: ${await sheet.innerText().catch(() => '(no sheet)')}`); });
  const typeLine = async (line) => { await input.focus(); await page.keyboard.type(line); await page.keyboard.press('Enter'); };
  const closeSheet = async () => { await sheet.getByRole('button', { name: /^(Close|Cancel)$/ }).first().click(); await sheet.waitFor({ state: 'detached' }); };

  // Open the workspace on its terminal tab.
  await page.locator('[data-shell=sidebar]').waitFor();
  await page.locator('[data-shell=sidebar] [data-key$="/work"], [data-shell=sidebar] [data-key$=":work"]').first().click();
  await page.locator('.xterm-rows').waitFor();
  await waitFor(async () => (await screen()).includes('READY-work'), 'the attached screen');

  /** Drop files on the page the way a browser delivers them. */
  const drop = (files) => page.evaluate((list) => {
    const dt = new DataTransfer();
    for (const f of list) dt.items.add(new File([new Uint8Array(f.bytes)], f.name, { type: f.type }));
    document.body.dispatchEvent(new DragEvent('dragover', { dataTransfer: dt, bubbles: true, cancelable: true }));
    const e = new DragEvent('drop', { dataTransfer: dt, bubbles: true, cancelable: true });
    document.body.dispatchEvent(e);
    return e.defaultPrevented;
  }, files);
  const openActions = async () => {
    await input.focus();
    await page.keyboard.press('Control+k'); // the command palette: a desktop window has no workspace actions button
    await page.keyboard.type('upload files');
    await page.keyboard.press('Enter');
    await see(sheet, 'the Upload sheet');
  };

  // 1. The picker: the sheet opens from the actions, nothing is sent before Upload, the file lands 0600 in the tab's folder.
  await openActions();
  if (!/server picks the folder/.test(await sheet.locator('[data-testid="upload-folder"]').innerText())) throw new Error('the sheet names a folder before upload_ready');
  await sheet.locator('input[type=file]').setInputFiles(make('hello.txt', 'hello upload\n'));
  await see(item('hello.txt'), 'hello.txt in the sheet');
  await sleep(500);
  if (names(projA).length) throw new Error(`files appeared before the user confirmed: ${names(projA)}`);
  await sheet.getByRole('button', { name: /^Upload/ }).click();
  await see(sheet.locator('li[data-state=done]'), 'hello.txt done');
  if (readFileSync(join(projA, 'hello.txt'), 'utf8') !== 'hello upload\n') throw new Error('hello.txt has the wrong content');
  if ((statSync(join(projA, 'hello.txt')).mode & 0o777) !== 0o600) throw new Error(`hello.txt mode is ${(statSync(join(projA, 'hello.txt')).mode & 0o777).toString(8)}, expected 600`);
  if (names(projA).join() !== 'hello.txt') throw new Error(`the folder holds ${names(projA)}, expected only hello.txt`);
  if (!(await sheet.locator('[data-testid="upload-folder"]').innerText()).includes(projA)) throw new Error('the sheet does not show the pinned folder');
  await closeSheet();
  console.log('ok  the picker: nothing before Upload, the file is in the tab folder with mode 600');

  // 2. Drop two files, one name taken: Replace for one, Keep both for the other.
  writeFileSync(join(projA, 'a.txt'), 'old a');
  writeFileSync(join(projA, 'b.txt'), 'old b');
  const prevented = await drop([{ name: 'a.txt', type: 'text/plain', bytes: [...Buffer.from('new a')] }, { name: 'b.txt', type: 'text/plain', bytes: [...Buffer.from('new b')] }]);
  if (!prevented) throw new Error('the drop was not cancelled, so the browser would navigate to the file');
  await see(sheet, 'the sheet opened by a drop');
  await item('a.txt').getByRole('radio', { name: 'Replace' }).click();
  await item('b.txt').getByRole('radio', { name: 'Keep both' }).click();
  await sheet.getByRole('button', { name: /^Upload/ }).click();
  await waitFor(async () => (await sheet.locator('li[data-state=done]').count()) === 2, 'both dropped files done');
  if (readFileSync(join(projA, 'a.txt'), 'utf8') !== 'new a') throw new Error('Replace did not replace a.txt');
  if (readFileSync(join(projA, 'b.txt'), 'utf8') !== 'old b') throw new Error('Keep both touched the existing b.txt');
  if (readFileSync(join(projA, 'b (1).txt'), 'utf8') !== 'new b') throw new Error('Keep both did not write b (1).txt');
  await closeSheet();
  console.log('ok  drop: Replace replaced, Keep both kept both');

  // 3. A pasted image (no text) opens the sheet and lands under a name with the time.
  await input.focus();
  await page.evaluate(() => {
    const dt = new DataTransfer();
    dt.items.add(new File([new Uint8Array([137, 80, 78, 71])], 'image.png', { type: 'image/png' }));
    document.activeElement.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  await see(sheet, 'the sheet opened by a pasted image');
  await sheet.getByRole('button', { name: /^Upload/ }).click();
  await see(sheet.locator('li[data-state=done]'), 'the image done');
  const pasted = names(projA).filter((n) => /^pasted-\d{8}-\d{6}\.png$/.test(n));
  if (pasted.length !== 1) throw new Error(`expected one pasted-*.png in ${projA}, found ${names(projA)}`);
  await closeSheet();
  console.log('ok  a pasted image is uploaded as pasted-<time>.png');

  // 4. Plain text pasted into the terminal still reaches the shell and opens no sheet.
  await input.focus();
  await page.evaluate(() => {
    const dt = new DataTransfer();
    dt.setData('text/plain', 'echo pasted-text-$((6*7))');
    document.activeElement.dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
  });
  await page.keyboard.press('Enter');
  await waitFor(async () => (await screen()).includes('pasted-text-42'), 'the pasted text to run in the shell');
  if (await sheet.count()) throw new Error('a text paste opened the Upload sheet');
  console.log('ok  a text paste goes to the terminal');

  // 5. A big file, so the transfer takes a moment: cd elsewhere meanwhile, then the file is in the folder pinned at the start.
  const BIG = 40 * 1024 * 1024;
  const big = join(fixtures, 'big.bin');
  writeFileSync(big, Buffer.alloc(BIG, 7));
  await openActions();
  await sheet.locator('input[type=file]').setInputFiles(big);
  const t0 = Date.now();
  await sheet.getByRole('button', { name: /^Upload/ }).click();
  await see(sheet.locator('li[data-state=uploading]'), 'big.bin uploading');
  await see(sheet.locator('[data-testid="upload-folder"] code'), 'the pinned folder');
  await typeLine(`cd ${projB}`);
  await see(sheet.locator('li[data-state=done]'), 'big.bin done', 120000);
  console.log(`measured  ${(BIG / 1048576 / ((Date.now() - t0) / 1000)).toFixed(1)} MiB/s over loopback with a 1 MiB window (${BIG / 1048576} MiB in ${Date.now() - t0} ms)`);
  if (!existsSync(join(projA, 'big.bin')) || existsSync(join(projB, 'big.bin'))) throw new Error('big.bin did not land in the folder pinned at the start');
  if (statSync(join(projA, 'big.bin')).size !== BIG) throw new Error('big.bin has the wrong size');
  await closeSheet();
  rmSync(join(projA, 'big.bin'));
  console.log('ok  a cd during the upload did not move the file');

  // 6. Cancel mid-transfer: no file and no partial remain.
  const before = names(projA);
  await openActions();
  await sheet.locator('input[type=file]').setInputFiles(big);
  await sheet.getByRole('button', { name: /^Upload/ }).click();
  await see(sheet.locator('li[data-state=uploading]'), 'big.bin uploading before cancel');
  await sheet.getByRole('button', { name: 'Cancel upload' }).click();
  await waitFor(() => names(projA).join() === before.join() && names(projB).length === 0, `no partial left (folder: ${names(projA)})`);
  await closeSheet().catch(() => {});
  console.log('ok  Cancel left no partial');

  // 7. Cut the connection mid-transfer: the client reconnects and resumes, and the file is whole.
  await openActions();
  await sheet.locator('input[type=file]').setInputFiles(big);
  await sheet.getByRole('button', { name: /^Upload/ }).click();
  await waitFor(async () => { const t = await item('big.bin').locator('.status').innerText().catch(() => ''); const m = /Uploading: (\d+)%/.exec(t); return m && Number(m[1]) >= 5; }, 'big.bin past 5%');
  await page.evaluate(() => window.__sockets.forEach((s) => s.close()));
  await see(sheet.locator('li[data-state=done]'), 'big.bin done after the reconnect', 120000);
  const got = readFileSync(join(projA, 'big.bin'));
  if (got.length !== BIG || got.some((b) => b !== 7)) throw new Error('the resumed file differs from the original');
  if (names(projA).some((n) => n.endsWith('.part'))) throw new Error(`a partial is left: ${names(projA)}`);
  await closeSheet();
  console.log('ok  a dropped connection resumed and the file is whole');

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
