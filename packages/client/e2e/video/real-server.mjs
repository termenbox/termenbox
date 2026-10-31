// Cases that need the real server binary (the add-server wizard: the server signs its own record, a directory stand-in
// stores it, the claim preview binds nobody until Add). `launchReal` gives render.mjs what startApp gives it for the scripted
// cases: a signed-in page in a recording context, plus `stop()`. Same stand-ins as e2e/add-server.mjs; scripted data only.
import { launch, contextOptions } from '../browser.mjs';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir, userInfo } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../../target/release/termenbox-server');
const dist = resolve(here, '../../dist');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.json': 'application/json', '.png': 'image/png' };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const randomPort = () => 30000 + Math.floor(Math.random() * 30000);

/** Starts the directory stand-in, the app's static server and one fresh termenbox-server with a claim secret. */
export async function startRealServer() {
  if (!existsSync(BIN)) throw new Error(`no server binary at ${BIN} (build it, or set TERMENBOX_BIN)`);
  const signer = generateKeyPairSync('ed25519');
  const signerPublic = signer.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
  const mint = (dpk, id, login) => {
    const payload = Buffer.from(JSON.stringify({ v: 1, kid: 'e2e', sub: id, login, dpk, exp: Math.floor(Date.now() / 1000) + 3600 }));
    return `${payload.toString('base64url')}.${sign(null, payload, signer.privateKey).toString('base64url')}`;
  };
  const records = new Map(), lookups = new Map();
  const reply = (res, r) => (r ? res.writeHead(200, { 'Content-Type': 'application/json' }).end(JSON.stringify({ record: r })) : res.writeHead(404).end());
  const web = createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    if (url.pathname === '/api/directory/announce' && req.method === 'POST') {
      let b = ''; req.on('data', (d) => (b += d)); req.on('end', () => {
        const r = JSON.parse(b);
        if (r.claim_hash) records.set(r.claim_hash, r);
        lookups.set(r.server_id, r);
        res.writeHead(200, { 'Content-Type': 'application/json' }).end('{}');
      });
      return;
    }
    if (url.pathname === '/api/directory/pending') return reply(res, records.get(url.searchParams.get('h')));
    if (url.pathname === '/api/directory/lookup') return reply(res, lookups.get(url.searchParams.get('id')));
    let f = join(dist, url.pathname);
    if (!f.startsWith(dist) || !existsSync(f) || f === dist + '/') f = join(dist, 'index.html');
    res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
  });
  await new Promise((r) => web.listen(0, '127.0.0.1', r));
  const appPort = web.address().port;
  const root = mkdtempSync(join(tmpdir(), 'termenbox-video-real-'));
  const home = join(root, 'server');
  mkdirSync(home);
  const httpPort = randomPort();
  const claim = randomBytes(32).toString('base64url');
  const proc = spawn(BIN, [], {
    env: { ...process.env, HOME: home, TMUX_TMPDIR: home, TERMENBOX_CONFIG_DIR: join(home, 'config'), TERMENBOX_HOST: '127.0.0.1', TERMENBOX_PORT: String(httpPort),
      TERMENBOX_UDP_PORT: String(randomPort()), TERMENBOX_PUBLIC_IP: '127.0.0.1', TERMENBOX_DIRECTORY_URL: `http://127.0.0.1:${appPort}`,
      TERMENBOX_SERVER_NAME: 'home-server', TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`, TERMENBOX_CLAIM_SECRET: claim },
    stdio: ['ignore', 'ignore', 'inherit'],
  });
  const t0 = Date.now();
  for (;;) {
    if (await fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false)) break;
    if (Date.now() - t0 > 20000) { proc.kill(); throw new Error('the server did not become healthy'); }
    await sleep(100);
  }
  return {
    claim, appPort, mint, config: join(home, 'config'),
    stop: async () => { proc.kill(); web.close(); rmSync(root, { recursive: true, force: true }); },
  };
}

/** A phone or desktop context with a device key and a signed-in user and no servers, on the real app + real server. */
export async function launchReal({ viewport, scale, phone, recordDir, overlay }) {
  const real = await startRealServer();
  const browser = await launch(['--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection']);
  const ctx = await browser.newContext(contextOptions({ viewport, deviceScaleFactor: scale, isMobile: phone, hasTouch: phone, recordVideo: { dir: recordDir, size: viewport } }));
  await ctx.addInitScript(overlay, phone);
  // The preview names the account the server runs as; a video shows a stand-in name, never the machine's real one.
  await ctx.addInitScript(([real, shown]) => {
    const swap = (n) => { if (n.nodeType === 3 && n.nodeValue.includes(real)) n.nodeValue = n.nodeValue.split(real).join(shown); else n.childNodes?.forEach(swap); };
    setInterval(() => { if (document.body) swap(document.body); }, 150);
  }, [userInfo().username, 'alex']);
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  // The pending lookup answers 404 until the server has announced: the page logs it, which is not an error.
  page.on('console', (m) => m.type() === 'error' && !/status of 404|Viewport argument key "interactive-widget"/.test(m.text()) && errors.push(m.text()));
  await page.goto(`http://127.0.0.1:${real.appPort}/`);
  const dpk = await page.evaluate(async () => {
    const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
    await new Promise((ok, no) => { const r = indexedDB.open('termenbox', 1); r.onupgradeneeded = () => r.result.createObjectStore('device');
      r.onsuccess = () => { const tx = r.result.transaction('device', 'readwrite'); tx.objectStore('device').put(pair, 'key'); tx.oncomplete = ok; tx.onerror = () => no(tx.error); }; r.onerror = () => no(r.error); });
    const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
    return btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  });
  await page.evaluate(([cert]) => { localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login: 'alice' })); localStorage.setItem('termenbox_servers', '[]'); localStorage.setItem('termenbox_ios_hint', '1'); }, [real.mint(dpk, 1, 'alice')]);
  await page.reload();
  await sleep(1500);
  return { page, ctx, errors, real, stop: async () => { await browser.close().catch(() => {}); await real.stop(); } };
}
