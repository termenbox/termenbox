// Pairing a LAN-only server in the real app against the real termenbox-server (decision 0123, R62): a server with no
// public address and no directory, a link from `--print-pairing`, the preview by name and fingerprint, a connect over
// the LAN candidate, and a corrupted link that never previews. The stand-in web server counts every directory request,
// from the page and from the server: LAN-only must make none.
// Usage: node e2e/pair.mjs   (needs a built server, `pnpm run e2e:build`, and Chrome)
import { chromium } from 'playwright-core';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { generateKeyPairSync, sign } from 'node:crypto';

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

const browser = await chromium.launch({
  ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: 'chrome' }),
  args: ['--no-sandbox', '--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection'],
});
const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
const page = await ctx.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
await page.goto(`http://127.0.0.1:${appPort}/`);
const dpk = await page.evaluate(async () => {
  const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
  await new Promise((ok, no) => { const r = indexedDB.open('termenbox', 1); r.onupgradeneeded = () => r.result.createObjectStore('device');
    r.onsuccess = () => { const tx = r.result.transaction('device', 'readwrite'); tx.objectStore('device').put(pair, 'key'); tx.oncomplete = ok; tx.onerror = () => no(tx.error); }; r.onerror = () => no(r.error); });
  const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
  return btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
});
await page.evaluate(([cert]) => { localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login: 'alice' })); localStorage.setItem('termenbox_servers', '[]'); localStorage.setItem('termenbox_ios_hint', '1'); }, [mint(dpk, 1, 'alice')]);

// 1. A corrupted link never previews.
// The page is not signed in yet: the link is kept across the (simulated) login, then handled on the reload.
await page.goto(corrupted);
await page.reload();
await page.getByTestId('pair-invalid').waitFor({ timeout: 15000 });
check((await page.getByTestId('pair-invalid').innerText()).toLowerCase().includes('invalid link'), 'a corrupted signature shows "Invalid link"');
check((await page.getByTestId('pair-preview').count()) === 0, 'a corrupted signature never shows the preview');
check(!existsSync(join(home, 'config', 'owner.json')), 'a corrupted link claimed nothing');
await page.getByTestId('pair-invalid').getByRole('button', { name: 'Close' }).click();

// 2. The real link previews by name and fingerprint, then connects over the LAN candidate.
// Signed in now: pasting the link into the open app only changes the fragment.
await page.goto(link);
await page.getByTestId('pair-preview').waitFor({ timeout: 15000 });
check((await page.getByTestId('pair-name').innerText()) === 'pair-e2e', 'the preview shows the server name');
check((await page.getByTestId('pair-fingerprint').innerText()) === info.fingerprint.replace('sha-256 ', ''), "the preview shows the server's DTLS fingerprint");
check(!existsSync(join(home, 'config', 'owner.json')), 'previewing claims nothing');
await page.getByTestId('pair-confirm').click();
await page.getByText('Connected to pair-e2e').waitFor({ timeout: 40000 });
check(true, 'connected after confirming');
check(existsSync(join(home, 'config', 'owner.json')), 'the claim token in the link made alice the owner');
await sleep(500);
await page.locator('[data-shell=bottom-bar] [data-destination=settings]').click();
await page.getByRole('button', { name: 'Servers', exact: true }).click();
await sleep(500);
const list = await page.locator('body').innerText();
check(/connected\s*·\s*Direct, LAN/i.test(list), `the active path is shown (${JSON.stringify(list.slice(0, 300))})`);
const saved = await page.evaluate(() => JSON.parse(localStorage.getItem('termenbox_servers') ?? '[]')[0]);
check(!!saved?.bundle && saved.winner?.startsWith('lan 127.0.0.1:'), `the bundle and the winning path are remembered (${saved?.winner})`);
check(apiCalls.length === 0, `LAN-only: zero directory requests from the server and the page (${JSON.stringify(apiCalls)})`);
check(errors.length === 0, `no page errors (${JSON.stringify(errors.slice(0, 3))})`);

proc.kill();
await browser.close();
web.close();
const bad = results.filter(([c]) => !c);
console.log(bad.length ? `\n${bad.length} FAILED` : '\nALL OK');
process.exit(bad.length ? 1 : 0);
