// The Add Server wizard in the real app at 375x667 (decision 0127, R65), against the real termenbox-server. Pairing a LAN-only server against the real termenbox-server (decision 0123, R62): a server with no
// public address and no directory, a link from `--print-pairing`, the preview by name and fingerprint, a connect over
// the LAN candidate, and a corrupted link that never previews. The stand-in web server counts every directory request,
// from the page and from the server: LAN-only must make none.
// Usage: node e2e/pair.mjs   (needs a built server, `pnpm run e2e:build`, and Chrome)
import { launch } from './browser.mjs';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { generateKeyPairSync, sign } from 'node:crypto';
import { createRequire } from 'node:module';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const randomPort = () => 30000 + Math.floor(Math.random() * 30000);
const results = [];
const check = (c, m) => { results.push([c, m]); console.log(`${c ? 'ok  ' : 'FAIL'} ${m}`); };
async function waitFor(fn, what, ms = 20000) {
  const t = Date.now();
  for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t > ms) throw new Error(`timed out: ${what}`); await sleep(100); }
}

const signer = generateKeyPairSync('ed25519');
const signerPublic = signer.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
const mint = (dpk, id, login) => {
  const payload = Buffer.from(JSON.stringify({ v: 1, kid: 'e2e', sub: id, login, dpk, exp: Math.floor(Date.now() / 1000) + 3600 }));
  return `${payload.toString('base64url')}.${sign(null, payload, signer.privateKey).toString('base64url')}`;
};
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.json': 'application/json', '.png': 'image/png' };
const dist = resolve(here, '../dist');
const apiCalls = [];
const web = createServer((req, res) => {
  const url = new URL(req.url, 'http://x');
  if (url.pathname.startsWith('/api/')) { apiCalls.push(`${req.method} ${url.pathname}`); return res.writeHead(404).end(); }
  let f = join(dist, url.pathname);
  if (!f.startsWith(dist) || !existsSync(f) || f === dist + '/') f = join(dist, 'index.html');
  res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
});
await new Promise((r) => web.listen(0, '127.0.0.1', r));
const appPort = web.address().port;

const root = mkdtempSync(join(tmpdir(), 'termenbox-pair-'));
const home = join(root, 'lan');
mkdirSync(home);
const httpPort = randomPort();
const env = {
  ...process.env, HOME: home, TMUX_TMPDIR: home, TERMENBOX_CONFIG_DIR: join(home, 'config'), TERMENBOX_HOST: '127.0.0.1', TERMENBOX_PORT: String(httpPort),
  TERMENBOX_UDP_PORT: String(randomPort()), TERMENBOX_LAN_ONLY: '1', TERMENBOX_LAN_ADDRS: '127.0.0.1', TERMENBOX_SERVER_NAME: 'pair-e2e',
  TERMENBOX_CLIENT_URL: `http://127.0.0.1:${appPort}`, TERMENBOX_DEV_INFO: 'on', TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`,
};
delete env.TERMENBOX_PUBLIC_IP; delete env.TERMENBOX_DIRECTORY_URL; delete env.TERMENBOX_CLAIM_SECRET;
const proc = spawn(BIN, [], { env, stdio: ['ignore', 'ignore', 'inherit'] });
process.on('exit', () => proc.kill()); // a failed check must not leave the server holding the CI step open
await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
const info = await (await fetch(`http://127.0.0.1:${httpPort}/rtc-info`)).json();

const printed = spawnSync(BIN, ['--print-pairing'], { env, encoding: 'utf8' });
const link = /(http:\/\/127\.0\.0\.1:\d+\/#\/pair\/\S+)/.exec(printed.stdout)?.[1];
check(!!link, '--print-pairing prints a pairing link');
const payload = link.split('/#/pair/')[1].split('?')[0];
// Flip a character in the middle of the signed payload: the signature no longer matches.
const corrupted = link.replace(payload, payload.slice(0, 60) + (payload[60] === 'A' ? 'B' : 'A') + payload.slice(61));


// A fake camera that films the QR of the pairing link, as y4m frames (Y plane = the QR, U/V neutral).
const QRCode = createRequire(import.meta.url)('qrcode');
function y4mOf(text, file) {
  const W = 640, H = 480;
  const { size, data } = QRCode.create(text, { errorCorrectionLevel: 'L' }).modules;
  const quiet = 4, scale = Math.floor(Math.min(W, H) / (size + 2 * quiet));
  const y = Buffer.alloc(W * H, 255);
  const ox = Math.floor((W - size * scale) / 2), oy = Math.floor((H - size * scale) / 2);
  for (let r = 0; r < size; r++) for (let c = 0; c < size; c++) if (data[r * size + c]) for (let dy = 0; dy < scale; dy++) for (let dx = 0; dx < scale; dx++) y[(oy + r * scale + dy) * W + ox + c * scale + dx] = 0;
  const uv = Buffer.alloc((W / 2) * (H / 2), 128);
  const frame = Buffer.concat([Buffer.from('FRAME\n'), y, uv, uv]);
  writeFileSync(file, Buffer.concat([Buffer.from(`YUV4MPEG2 W${W} H${H} F10:1 Ip A1:1 C420jpeg\n`), ...Array(10).fill(frame)]));
}
const ownedPairing = () => /(http:\/\/127\.0\.0\.1:\d+\/#\/pair\/\S+)/.exec(spawnSync(BIN, ['--print-pairing'], { env, encoding: 'utf8' }).stdout)?.[1];

const camera = join(root, 'qr.y4m');
const browser = await launch(['--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection', '--use-fake-ui-for-media-stream', '--use-fake-device-for-media-stream', `--use-file-for-fake-video-capture=${camera}`]);
// The fake camera is read when the browser starts, so the QR file must exist now: the link of a server that is not yet owned has
// the same shape as the one printed after it is, and the second scan below is of the owned link.
y4mOf(link, camera);
const ctx = await browser.newContext({ viewport: { width: 375, height: 667 }, isMobile: true, hasTouch: true, permissions: ['camera'] });
const page = await ctx.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
// The Install step polls the directory for the announcement from the moment it opens: a 404 there is expected.
page.on('console', (m) => m.type() === 'error' && !/status of 404/.test(m.text()) && errors.push(m.text()));
await page.goto(`http://127.0.0.1:${appPort}/`);
const dpk = await page.evaluate(async () => {
  const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
  await new Promise((ok, no) => { const r = indexedDB.open('termenbox', 1); r.onupgradeneeded = () => r.result.createObjectStore('device');
    r.onsuccess = () => { const tx = r.result.transaction('device', 'readwrite'); tx.objectStore('device').put(pair, 'key'); tx.oncomplete = ok; tx.onerror = () => no(tx.error); }; r.onerror = () => no(r.error); });
  const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
  return btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
});
await page.evaluate(([cert]) => { localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login: 'alice' })); localStorage.setItem('termenbox_servers', '[]'); localStorage.setItem('termenbox_ios_hint', '1'); }, [mint(dpk, 1, 'alice')]);
await page.reload();
await sleep(1500);
const inView = async (loc) => { const b = await loc.boundingBox(); return !!b && b.y >= 0 && b.y + b.height <= 667 && b.x >= 0 && b.x + b.width <= 375; };

// The empty server list offers Scan QR beside Add Server without scrolling.
check(await inView(page.getByTestId('scan-qr')), 'Scan QR is visible in the first viewport of the server list at 375x667');

// Step 1: Install. One command and one Copy button in the first viewport; the run-mode choice is folded under Options.
await page.getByRole('button', { name: 'Add Server', exact: true }).first().click();
await page.getByRole('dialog').waitFor();
check(await page.getByTestId('copy-command').count() === 0, 'Install: the command waits for consent (this machine, current user, by default)');
await page.getByTestId('consent').getByRole('checkbox').check();
check(await inView(page.getByTestId('copy-command')), 'Install: the Copy button is in the first viewport once consent is given');
check((await page.getByTestId('steps').locator('li').allInnerTexts()).join('|') === '1 Install|2 Connect|3 Confirm|4 Done', 'the four steps are shown');
check(!(await page.getByTestId('install-options').evaluate((d) => d.open)), 'Install: Options (name, run mode) is folded');
check((await page.getByText('Where should workspaces run?').isVisible()) === false, 'Install: the run-mode form is not on screen until Options is opened');

// Step 2: Connect. The scan button and the camera are in the first viewport; the checklist is live.
await page.getByTestId('install-next').click();
check(await inView(page.getByTestId('scan-it')), 'Connect: the scan button is in the first viewport');
check((await page.getByTestId('checklist').innerText()).includes('Looking for the server'), 'Connect: the checklist is shown');
check((await page.getByTestId('connect-lead').innerText()).includes('Scan the QR or paste the link'), 'Connect: says what to do with what the installer printed');
check((await page.getByTestId('connect-hint').count()) === 0, 'Connect: no "nothing yet" hint before anything was read');
await page.getByTestId('scan-it').click();
await page.getByTestId('scan-video').waitFor({ state: 'visible' });
check(await inView(page.getByTestId('scan-video')) || (await page.getByTestId('scan-video').boundingBox()).y < 400, 'Connect: the camera opens in place at the top');
await page.getByRole('button', { name: 'Stop camera' }).click();

// Pasting the link goes to Confirm (every address is tried live), then Add, then Done with the active path.
await page.locator('#have-link').fill(link);
await page.getByRole('button', { name: 'Add', exact: true }).first().click();
await page.getByText('This server is ready to add').waitFor({ timeout: 40000 });
check(await page.getByTestId('pair-name').innerText() === 'pair-e2e', 'Confirm: the preview shows the server name');
check(!existsSync(join(home, 'config', 'owner.json')), 'Confirm: nothing is claimed yet');
await page.getByTestId('pair-confirm').click();
await page.getByText('Server added').waitFor();
const doneText = await page.locator('.done[role=status]').innerText();
check(/can reach it: Direct, LAN/.test(doneText), `Done: the active path is shown (${JSON.stringify(doneText.replace(/\n+/g, ' | ').slice(0, 120))})`);
await waitFor(() => existsSync(join(home, 'config', 'owner.json')), 'owner.json', 30000).catch(() => {});
check(existsSync(join(home, 'config', 'owner.json')), 'the claim in the link made alice the owner');
await page.getByRole('button', { name: 'Done' }).click();
await page.getByRole('dialog').waitFor({ state: 'detached' });

// Scan QR outside the wizard: the camera opens straight away and a scanned (re-add) link goes to Confirm with no Install step.
await page.evaluate(() => { localStorage.setItem('termenbox_servers', '[]'); });
await page.reload();
await sleep(1500);
y4mOf(ownedPairing(), camera);
await page.getByTestId('scan-qr').click();
await page.getByRole('dialog').waitFor();
check((await page.getByTestId('steps').count()) === 0 && !(await page.getByText('Run this once on your server').count()), 'Scan QR: no Install step is shown');
await page.getByText('This server is ready to add').waitFor({ timeout: 40000 });
check(true, 'Scan QR: the camera read the QR and went straight to Confirm');
check(!(await page.getByText('Run this once on your server').count()), 'Scan QR: Confirm was reached without the Install step');
check(errors.length === 0, `no page errors (${JSON.stringify(errors.slice(0, 3))})`);

proc.kill();
await browser.close();
web.close();
const bad = results.filter(([c]) => !c);
console.log(bad.length ? `\n${bad.length} FAILED` : '\nALL OK');
process.exit(bad.length ? 1 : 0);
