// Add Server in the real app against a real termenbox-server and a stand-in directory (announce, pending, lookup): the
// claim preview shows what is being added and binds nobody (decision 0117), the edited name is what gets stored, and the
// reachability check ends in "can reach it" when the UDP port answers and in "could not reach it" when it does not
// (decision 0118). The server signs its own record; the directory lives in this script.
// Usage: node e2e/add-server.mjs   (needs a built server, `pnpm run build`, and Chrome)
import { chromium } from 'playwright-core';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const randomPort = () => 30000 + Math.floor(Math.random() * 30000);
const results = [];
const check = (c, m) => { results.push([c, m]); console.log(`${c ? 'ok  ' : 'FAIL'} ${m}`); };
const log = (m) => console.log(m);
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
const records = new Map(); // claim_hash -> record
const lookups = new Map(); // server_id -> record
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
  if (url.pathname === '/api/directory/pending') {
    const r = records.get(url.searchParams.get('h'));
    return r ? res.writeHead(200, { 'Content-Type': 'application/json' }).end(JSON.stringify({ record: r })) : res.writeHead(404).end();
  }
  if (url.pathname === '/api/directory/lookup') {
    const r = lookups.get(url.searchParams.get('id'));
    return r ? res.writeHead(200, { 'Content-Type': 'application/json' }).end(JSON.stringify({ record: r })) : res.writeHead(404).end();
  }
  let f = join(dist, url.pathname);
  if (!f.startsWith(dist) || !existsSync(f) || f === dist + '/') f = join(dist, 'index.html');
  res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
});
await new Promise((r) => web.listen(0, '127.0.0.1', r));
const appPort = web.address().port;

const root = mkdtempSync(join(tmpdir(), 'termenbox-ui-'));
const procs = [];
async function startServer(label) {
  const home = join(root, label);
  mkdirSync(home);
  const httpPort = randomPort();
  const claim = randomBytes(32).toString('base64url');
  const proc = spawn(BIN, [], {
    env: { ...process.env, HOME: home, TMUX_TMPDIR: home, TERMENBOX_CONFIG_DIR: join(home, 'config'), TERMENBOX_HOST: '127.0.0.1', TERMENBOX_PORT: String(httpPort),
      TERMENBOX_UDP_PORT: String(randomPort()), TERMENBOX_PUBLIC_IP: '127.0.0.1', TERMENBOX_DIRECTORY_URL: `http://127.0.0.1:${appPort}`,
      TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`, TERMENBOX_CLAIM_SECRET: claim },
    stdio: ['ignore', 'ignore', 'inherit'],
  });
  procs.push(proc);
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), `${label} health`);
  return { label, home, claim, proc, config: join(home, 'config') };
}

async function run(label, srv, extraArgs, freeze, expected) {
  const browser = await chromium.launch({
    ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: 'chrome' }),
    args: ['--no-sandbox', ...extraArgs],
  });
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  const page = await ctx.newPage();
  const errors = [];
  page.on('response', (r) => r.status() === 404 && log('   404: ' + r.url().replace(/claim=[^&]*/, 'claim=…').replace(/h=[^&]*/, 'h=…')));
  page.on('pageerror', (e) => errors.push(e.message));
  // The pending lookup answers 404 until the server has announced: the page logs it, which is not an error.
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
  // Reach Add Server.
  await page.getByRole('button', { name: 'Add Server', exact: true }).first().click();
  await page.getByRole('dialog').waitFor();
  // Install is step 1; the server announces itself while the command is being run, so go on to Connect.
  await page.getByTestId('install-next').click();
  // Feed the claim code of the running server (the dialog's own random secret would need a server started with it).
  await page.locator('#have-link').fill(srv.claim);
  await page.getByRole('button', { name: 'Add', exact: true }).first().click();
  const t0 = Date.now();
  await page.getByText('This server is ready to add').waitFor({ timeout: 40000 });
  check(true, `${label}: preview appeared after ${Date.now() - t0} ms`);
  await page.getByRole('group', { name: 'What you are adding' }).scrollIntoViewIfNeeded();
  if (freeze) { srv.proc.kill('SIGSTOP'); log('   server suspended (UDP port answers nothing)'); }
  const text = await page.getByRole('group', { name: 'What you are adding' }).innerText();
  check(/isolated container|on the machine itself/.test(text) && /Version/.test(text), `${label}: preview shows where sessions run and the version`);
  check(!existsSync(join(srv.config, 'owner.json')), `${label}: nothing claimed before Add (owner.json absent)`);
  check((await page.locator('#preview-name').inputValue()).length > 0, `${label}: name prefilled`);
  await page.locator('#preview-name').fill(`ui-${label}`);
  await page.getByRole('group', { name: 'What you are adding' }).getByRole('button', { name: 'Add', exact: true }).click();
  await page.getByText('Server added').waitFor();
  await page.getByText(/This device can reach it|could not reach it|Could not tell/).first().waitFor({ timeout: 60000 });
  await page.locator('.done[role=status]').scrollIntoViewIfNeeded();
  const panel = await page.locator('.done[role=status]').innerText();
  check(expected.test(panel), `${label}: the result panel says ${expected} (got ${JSON.stringify(panel.replace(/\n+/g, ' | ').slice(0, 160))})`);
  check(errors.length === 0, `${label}: no page errors (${JSON.stringify(errors.slice(0, 3))})`);
  check((await page.evaluate(() => document.documentElement.scrollWidth - innerWidth)) <= 0, `${label}: no horizontal overflow`);
  if (freeze) srv.proc.kill('SIGCONT');
  await page.getByRole('button', { name: 'Done' }).click();
  await page.getByRole('dialog').waitFor({ state: 'detached' });
  await sleep(3000);
  await page.locator('[data-shell=bottom-bar] [data-destination=settings]').click();
  await page.getByRole('button', { name: 'Servers', exact: true }).click();
  await sleep(500);
  const listText = await page.locator('body').innerText();
  check(listText.includes(`ui-${label}`), `${label}: server is in the list under the edited name`);
  await browser.close();
  return panel;
}

const A = await startServer('open');
await run('open', A, ['--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection'], false, /This device can reach it/);
const B = await startServer('udpblocked');
// The server is suspended after the preview, so its UDP port answers nothing: what a network that drops UDP looks like.
await run('udpblocked', B, ['--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection'], true, /could not reach it/);

procs.forEach((p) => p.kill());
web.close();
const bad = results.filter(([c]) => !c);
console.log(bad.length ? `\n${bad.length} FAILED` : '\nALL OK');
process.exit(bad.length ? 1 : 0);
