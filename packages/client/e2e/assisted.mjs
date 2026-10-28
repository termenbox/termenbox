// The assisted path end to end (DECISION-0141, R77): a real termenbox-server that is not directly reachable, a real
// Chromium, and loopback stand-ins for the owner's Worker, the STUN server, the TURN server and Cloudflare's credential
// endpoint (assisted-fakes.mjs). The server announces an address nothing can reach, so the only ways in are the punch
// through the stand-in NAT, the relay when the NAT is cut, and nothing when the Worker is gone.
// Usage: node e2e/assisted.mjs   (needs a built server, `pnpm run e2e:build`, Chrome and openssl)
import { launch, contextOptions } from './browser.mjs';
import { startNat, startTurn, startWorker, startCredentials } from './assisted-fakes.mjs';
import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs';
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


const nat = await startNat();
const turn = await startTurn();
const worker = await startWorker();
const creds = await startCredentials(turn.port);
const ROUTE = 'e2eRouteHash0123456789';

const root = mkdtempSync(join(tmpdir(), 'termenbox-assisted-'));
const home = join(root, 'home');
mkdirSync(join(home, 'config'), { recursive: true });
// What `--setup-cloudflare` leaves: the Worker's URL and the TURN key (the account token is never kept).
writeFileSync(join(home, 'config', 'relay.json'), JSON.stringify({ signal: `wss://127.0.0.1:${worker.securePort}/s/${ROUTE}`, turn_key_id: 'e2e-key', turn_key_token: 'e2e-token' }), { mode: 0o600 });
const httpPort = randomPort();
const env = {
  ...process.env, HOME: home, TMUX_TMPDIR: home, TERMENBOX_CONFIG_DIR: join(home, 'config'), TERMENBOX_HOST: '127.0.0.1', TERMENBOX_PORT: String(httpPort),
  TERMENBOX_UDP_PORT: String(randomPort()), TERMENBOX_SERVER_NAME: 'assisted-e2e', TERMENBOX_CLIENT_URL: `http://127.0.0.1:${appPort}`,
  TERMENBOX_DEV_INFO: 'on', TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`,
  // 192.0.2.1 (TEST-NET-1) is announced as the public address and answers nobody; STUN says the real one is the NAT's alias.
  TERMENBOX_PUBLIC_IP: '192.0.2.1', TERMENBOX_ASSUME_NAT: '1', TERMENBOX_LAN_ADDRS: '192.0.2.2', TERMENBOX_OVERLAY_ADDRS: '192.0.2.3', TERMENBOX_NO_IPV6: '1', TERMENBOX_STUN: `127.0.0.1:${nat.port}`,
  TERMENBOX_SIGNAL_DIAL: `ws://127.0.0.1:${worker.plainPort}/s/${ROUTE}`, TERMENBOX_RELAY_API: `http://127.0.0.1:${creds.port}`,
};
delete env.TERMENBOX_LAN_ONLY; delete env.TERMENBOX_DIRECTORY_URL; delete env.TERMENBOX_CLAIM_SECRET;
const proc = spawn(BIN, [], { env, stdio: ['ignore', 'ignore', 'inherit'] });
const stopAll = () => { proc.kill(); nat.stop(); turn.stop(); worker.stop(); creds.stop(); web.close(); };
process.on('exit', () => proc.kill()); // a failed check must not leave the server holding the CI step open
await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
await waitFor(() => nat.aliases > 0, 'the server asked STUN for its mapped address');
check(true, 'the server asked the STUN stand-in for its mapped address');
await waitFor(() => worker.conns.size > 0, 'the server dialed the Worker');
check(true, 'the server holds a socket to the Worker');

const printed = spawnSync(BIN, ['--print-pairing'], { env, encoding: 'utf8' });
const link = /(http:\/\/127\.0\.0\.1:\d+\/#\/pair\/\S+)/.exec(printed.stdout)?.[1];
check(!!link, `--print-pairing prints a pairing link (${printed.stdout.slice(0, 200)}${printed.stderr.slice(0, 200)})`);
const payload = link.split('/#/pair/')[1].split('?')[0];
const corrupted = link.replace(payload, payload.slice(0, 60) + (payload[60] === 'A' ? 'B' : 'A') + payload.slice(61));

const browser = await launch(['--allow-loopback-in-peer-connection', '--ignore-certificate-errors']);
const ctx = await browser.newContext(contextOptions({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true }));
const page = await ctx.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
page.on('console', (m) => m.type() === 'error' && !/Viewport argument key "interactive-widget"|status of 404/.test(m.text()) && errors.push(m.text()));
await page.goto(`http://127.0.0.1:${appPort}/`);
const dpk = await page.evaluate(async () => {
  const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
  await new Promise((ok, no) => { const r = indexedDB.open('termenbox', 1); r.onupgradeneeded = () => r.result.createObjectStore('device');
    r.onsuccess = () => { const tx = r.result.transaction('device', 'readwrite'); tx.objectStore('device').put(pair, 'key'); tx.oncomplete = ok; tx.onerror = () => no(tx.error); }; r.onerror = () => no(r.error); });
  const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
  return btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
});
await page.evaluate(([cert, stun]) => { localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login: 'alice' })); localStorage.setItem('termenbox_servers', '[]'); localStorage.setItem('termenbox_ios_hint', '1'); localStorage.setItem('termenbox_stun', stun); }, [mint(dpk, 1, 'alice'), `127.0.0.1:${nat.port}`]);


// A timeout says where the fakes stood, which is what a CI log needs to tell a client fault from a server one.
const connected = async (ms) => {
  try { await page.getByText('Connected to assisted-e2e').waitFor({ timeout: ms }); } catch (e) {
    const body = await page.locator('body').innerText().catch(() => '');
    throw new Error(`not connected after ${ms} ms: worker ${JSON.stringify(worker.messages)}, nat ${nat.aliases} aliases ${nat.forwarded} packets, turn ${turn.allocations} allocations, ${creds.calls} credentials; page ${JSON.stringify(body.slice(0, 300))}; console ${JSON.stringify(errors.slice(-3))}`, { cause: e });
  }
};
const pathsText = async () => {
  await page.locator('[data-shell=bottom-bar] [data-destination=settings]').click();
  await page.getByRole('button', { name: 'Servers', exact: true }).click();
  await sleep(500);
  return page.locator('body').innerText();
};
const reopen = async () => { await page.goto(`http://127.0.0.1:${appPort}/#/`); await page.reload(); };

// 1. A corrupted link never previews, and the Worker is never asked to start a session for it.
await page.goto(corrupted);
await page.reload();
await page.getByTestId('pair-invalid').waitFor({ timeout: 15000 });
check((await page.getByTestId('pair-preview').count()) === 0, 'a corrupted signature never shows the preview');
check(!existsSync(join(home, 'config', 'owner.json')), 'a corrupted link claimed nothing');
await page.getByTestId('pair-invalid').getByRole('button', { name: 'Close' }).click();

// 2. The punch: the announced address is dead, so the session can only open through the Worker and the NAT.
await page.goto(link);
await page.getByTestId('pair-preview').waitFor({ timeout: 15000 });
await page.getByTestId('pair-confirm').click();
await connected(60000);
check(true, 'the session opened with the announced address dead');
check(worker.messages.some((m) => m.role === 'client') && worker.messages.some((m) => m.role === 'server'), 'the offer and the answer went through the Worker');
check(nat.forwarded > 0, `the media path went through the NAT stand-in (${nat.forwarded} packets)`);
check(turn.allocations === 0 && creds.calls === 0, 'the punch asked for no relay and minted no credential');
check(existsSync(join(home, 'config', 'owner.json')), 'the claim token in the link made alice the owner');
let list = await pathsText();
check(/connected\s*·\s*Punched/i.test(list), `the active path is shown as Punched (${JSON.stringify(list.slice(0, 300))})`);

// 3. The relay: the punch is blocked, so the browser waits out its window, asks for credentials and both sides allocate.
nat.cut = true;
const forwardedBefore = nat.forwarded;
await reopen();
await connected(90000);
list = await pathsText();
check(/connected\s*·\s*Relay/i.test(list), `the active path is shown as Relay (${JSON.stringify(list.slice(0, 300))})`);
check(creds.calls >= 2 && turn.allocations >= 2, `credentials were minted and both ends allocated (${creds.calls} credentials, ${turn.allocations} allocations)`);
check(nat.forwarded === forwardedBefore, 'no packet crossed the cut NAT');
check(turn.relayed > 0, `the session ran over the relay (${turn.relayed} bytes)`);

// 4. The signal is down: the report names the cause and the fix.
const errorsBefore = errors.length; // a refused WebSocket is logged by the browser, and expected from here on
worker.stop();
nat.cut = true;
await reopen();
await pathsText();
// The row gives up after one full round: the issue icon turns red, and its popover holds the path report.
await page.locator('[data-testid=server-issue][data-severity=error]').waitFor({ timeout: 90000 });
await page.getByTestId('server-issue').click();
const report = await page.getByTestId('all-failed').innerText();
check(/signaling Worker/i.test(report) && /--setup-cloudflare/.test(report), `with the Worker gone the report names the cause and the fix (${JSON.stringify(report.slice(0, 400) || (await page.locator('body').innerText()).slice(0, 400))})`);
check(errors.slice(0, errorsBefore).length === 0, `no page errors before the Worker went away (${JSON.stringify(errors.slice(0, errorsBefore))})`);

stopAll();
await browser.close();
const bad = results.filter(([c]) => !c);
console.log(bad.length ? `\n${bad.length} FAILED` : '\nALL OK');
process.exit(bad.length ? 1 : 0);
