// Touch behaviour of the terminal against real tmux (alternate screen, with a mouse-reporting stand-in): a tap reaches
// a mouse-aware app as a click, and a one-finger drag scrolls. Real Chrome with touch emulation, real server.
// Usage: node e2e/touch.mjs   (needs the built client and the e2e harness)
import { chromium } from 'playwright-core';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const SHOTS = process.env.SHOTS ?? '';
const fail = (m) => { console.error(`FAIL: ${m}`); process.exitCode = 1; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const listen = (s) => new Promise((r) => s.listen(0, '127.0.0.1', () => r(s.address().port)));
const signer = generateKeyPairSync('ed25519');
const signerPublic = signer.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
const mint = (dk, id, login) => {
  const payload = Buffer.from(JSON.stringify({ v: 1, kid: 'e2e', sub: id, login, dpk: dk, exp: Math.floor(Date.now() / 1000) + 3600 }));
  return `${payload.toString('base64url')}.${sign(null, payload, signer.privateKey).toString('base64url')}`;
};
const serverIdOf = (dir) => {
  const der = Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), readFileSync(join(dir, 'server_key'))]);
  return createPublicKey(createPrivateKey({ key: der, format: 'der', type: 'pkcs8' })).export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
};
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const dist = join(here, 'dist');
const statics = createServer((req, res) => {
  const file = join(dist, req.url === '/' ? 'index.html' : req.url.split('?')[0]);
  if (!file.startsWith(dist) || !existsSync(file)) return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': mime[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
async function waitFor(fn, what, ms = 15000) {
  const t = Date.now();
  for (;;) { const v = await fn(); if (v) return v; if (Date.now() - t > ms) throw new Error(`timed out: ${what}`); await sleep(100); }
}

const webPort = await listen(statics);
const home = mkdtempSync(join(tmpdir(), 'termenbox-touch-'));
const httpPort = 30000 + Math.floor(Math.random() * 30000);
const udpPort = 30000 + Math.floor(Math.random() * 30000);
const claim = randomBytes(32).toString('base64url');
const cleanEnv = { ...process.env, HOME: home, XDG_CONFIG_HOME: join(home, 'xdg'), TMUX_TMPDIR: home };
for (const k of Object.keys(cleanEnv)) if (k === 'TERM_PROGRAM') delete cleanEnv[k];
const server = spawn(BIN, [], {
  env: { ...cleanEnv, TERMENBOX_BACKEND: 'tmux', TERMENBOX_CONFIG_DIR: join(home, 'config'), TERMENBOX_HOST: '127.0.0.1', TERMENBOX_PORT: String(httpPort),
    TERMENBOX_UDP_PORT: String(udpPort), TERMENBOX_PUBLIC_IP: '127.0.0.1', TERMENBOX_DIRECTORY_URL: '', TERMENBOX_DEV_INFO: 'on',
    TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`, TERMENBOX_CLAIM_SECRET: claim },
  stdio: ['ignore', 'inherit', 'inherit'],
});
let browser;
try {
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
  const info = await (await fetch(`http://127.0.0.1:${httpPort}/rtc-info`)).json();
  const record = { ip: info.ip, udp_port: info.port, fingerprint: info.fingerprint };
  browser = await chromium.launch({
    ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: 'chrome' }),
    args: ['--no-sandbox', '--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection'],
  });
  const ctx = await browser.newContext({ viewport: { width: 390, height: 780 }, hasTouch: true, isMobile: true, deviceScaleFactor: 2 });
  // No WebGL: xterm falls back to its DOM renderer, whose rows can be read back as text.
  await ctx.addInitScript(() => {
    const orig = HTMLCanvasElement.prototype.getContext;
    HTMLCanvasElement.prototype.getContext = function (t, ...a) { return String(t).startsWith('webgl') ? null : orig.call(this, t, ...a); };
  });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => fail(`page error: ${e.message}`));
  await page.goto(`http://127.0.0.1:${webPort}/`);
  const cert = mint(await page.evaluate(() => window.__e2e.deviceKey()), 1, 'alice');
  await page.evaluate((o) => window.__e2e.start(o), { record, cert, serverId: serverIdOf(join(home, 'config')), claim });
  await waitFor(() => page.evaluate(() => window.__e2e.state === 'connected'), 'connected');
  await page.evaluate(() => window.__e2e.send({ type: 'new_session', name: 'h1' }));
  await waitFor(() => page.evaluate(() => window.__e2e.sessions.some((s) => s.id === 'h1')), 'session listed');
  await page.evaluate(() => { window.__e2e.mountTerminal('h1'); window.__e2e.send({ type: 'attach', session_id: 'h1' }); });

  const screenText = () => page.evaluate(() => [...document.querySelectorAll('.xterm-rows > div')].map((r) => r.textContent.replace(/\s+$/, '')).join('\n'));
  const cellXY = (col, row) => page.evaluate(({ col, row }) => {
    const r = document.querySelector('.xterm-screen').getBoundingClientRect();
    const rows = document.querySelector('.xterm-rows').children;
    return { x: r.left + (col + 0.5) * (r.width / rows[0].textContent.length), y: r.top + (row + 0.5) * (r.height / rows.length) };
  }, { col, row });

  // Real touch events through the browser's input pipeline.
  const cdp = await ctx.newCDPSession(page);
  const touch = (type, pts) => cdp.send('Input.dispatchTouchEvent', { type, touchPoints: pts.map((p, i) => ({ x: p.x, y: p.y, id: i })) });
  const tap = async (x, y) => { await touch('touchStart', [{ x, y }]); await sleep(60); await touch('touchEnd', []); };
  const drag = async (x, y0, y1, steps = 12) => {
    await touch('touchStart', [{ x, y: y0 }]);
    for (let i = 1; i <= steps; i++) { await touch('touchMove', [{ x, y: y0 + ((y1 - y0) * i) / steps }]); await sleep(16); }
    await touch('touchEnd', []);
  };
  const lastLine = async () => (await screenText()).split('\n').filter((l) => l.trim()).slice(-1)[0] ?? '';

  // tmux asks the terminal for its 256-colour palette on attach; the replies used to flood the server and
  // drop the connection every time, which showed as a reconnect every ~10 s. One connection, no reconnects:
  await sleep(4000);
  const readies = await page.evaluate(() => window.__e2e.ready);
  if (readies !== 1) fail(`the connection was re-established ${readies - 1} time(s) within 4 s of attaching`);
  else console.log('ok  attaching to a session does not drop the connection');

  // A stand-in for a mouse-first app: it switches on SGR mouse reporting and echoes what it receives.
  await sleep(1500);
  await page.evaluate(() => window.__e2e.type("printf '\\033[?1000h\\033[?1006h'; stty raw -echo; cat -v\r"));
  await sleep(800);
  const bytes = async () => (await screenText()).replace(/\s+/g, '');
  const cellOf = (col, row) => cellXY(col, row);

  // 1. A tap arrives as a press and a release at the cell under the finger.
  const p = await cellOf(10, 8);
  await tap(p.x, p.y);
  await sleep(500);
  const afterTap = await bytes();
  const press = /\^\[\[<0;(\d+);(\d+)M/.exec(afterTap);
  const release = /\^\[\[<0;(\d+);(\d+)m/.exec(afterTap);
  if (!press || !release) fail(`a tap did not arrive as press and release: ${afterTap.slice(0, 200)}`);
  else if (press[1] !== '11' || press[2] !== '9') fail(`the tap landed on the wrong cell: ${press[0]} (want col 11, row 9)`);
  else console.log('ok  a tap arrives as a press and a release on the cell under the finger');

  // 2. A drag up sends wheel-down reports, a drag down wheel-up, about one per row dragged.
  await page.evaluate(() => (window.__e2e.sent.length = 0));
  const rowPx = p.y - (await cellOf(10, 7)).y;
  await drag(p.x, p.y + rowPx * 6, p.y, 12);
  await sleep(600);
  const up = (await bytes()).match(/\^\[\[<65;/g)?.length ?? 0;
  if (up < 4) fail(`a drag up sent ${up} wheel-down reports, want about 6`);
  else console.log(`ok  a drag up scrolls forward (${up} wheel reports)`);
  await drag(p.x, p.y, p.y + rowPx * 6, 12);
  await sleep(600);
  const down = (await bytes()).match(/\^\[\[<64;/g)?.length ?? 0;
  if (down < 4) fail(`a drag down sent ${down} wheel-up reports, want about 6`);
  else console.log(`ok  a drag down scrolls back (${down} wheel reports)`);

  // 3. Soft-keyboard input (issue #250). Chrome Android delivers typing as composition or with keyCode 229, and
  //    xterm reads the hidden textarea later, in a setTimeout. The mirror used to write the previous phrase first,
  //    so every character was read as empty. The iOS insertText path must keep sending its data too.
  const sentNow = () => page.evaluate(() => window.__e2e.sent.splice(0).join(''));
  await sentNow(); // the drags above queued wheel reports
  await page.locator('.xterm-helper-textarea').focus();

  await page.evaluate(() => {
    const ta = document.querySelector('.xterm-helper-textarea');
    ta.value = '';
    ta.dispatchEvent(new CompositionEvent('compositionstart', { data: '', bubbles: true }));
    ta.value = 'z';
    ta.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: 'z', isComposing: true }));
    ta.dispatchEvent(new CompositionEvent('compositionupdate', { data: 'z', bubbles: true }));
    ta.dispatchEvent(new CompositionEvent('compositionend', { data: 'z', bubbles: true }));
  });
  await sleep(150);
  const composed = await sentNow();
  if (composed !== 'z') fail(`a composed character sent ${JSON.stringify(composed)}, want "z" (issue #250)`);
  else console.log('ok  a composed soft-keyboard character reaches the terminal once');

  await page.evaluate(() => {
    const ta = document.querySelector('.xterm-helper-textarea');
    const before = ta.value;
    ta.dispatchEvent(new KeyboardEvent('keydown', { key: 'Unidentified', keyCode: 229, which: 229, bubbles: true, cancelable: true }));
    ta.value = before + 'a';
    ta.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertCompositionText', data: 'a', isComposing: true }));
    ta.dispatchEvent(new KeyboardEvent('keyup', { key: 'Unidentified', keyCode: 229, bubbles: true }));
  });
  await sleep(150);
  const legacy = await sentNow();
  if (legacy !== 'a') fail(`a keyCode 229 character sent ${JSON.stringify(legacy)}, want "a" (issue #250)`);
  else console.log('ok  a keyCode 229 character reaches the terminal once');

  await page.evaluate(() => {
    const ta = document.querySelector('.xterm-helper-textarea');
    ta.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText', data: 'q' }));
  });
  await sleep(150);
  const ios = await sentNow();
  if (ios !== 'q') fail(`an insertText character sent ${JSON.stringify(ios)}, want "q" (iOS path)`);
  else console.log('ok  the iOS insertText path still reaches the terminal');
  if (process.env.HOLD) await sleep(Number(process.env.HOLD));
} finally {
  await browser?.close();
  server.kill();
  statics.close();
  // The static server and the Chrome pipes would otherwise keep this process (and the CI job) alive.
  setTimeout(() => process.exit(process.exitCode ?? 0), 100);
}
