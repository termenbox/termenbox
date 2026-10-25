// Browser end-to-end test and resource gates (issue #21). Real Chromium, real termenbox-server, real tmux;
// only the hosted function that issues certificates is a stand-in (here: a key pair made in this script). Usage: node e2e/run.mjs   (needs a built server, a built harness, and Chrome)
//   TERMENBOX_BIN   server binary          (default ../../target/release/termenbox-server)
//   CHROME_PATH    Chrome/Chromium binary (default: Playwright's "chrome" channel)
import { launch } from './browser.mjs';
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const LIMITS = { rssMb: 300, cpuShare: 0.5 }; // issue #21: RSS with 10 workspaces, CPU for 10k lines/s
const TICKS = 100; // clock ticks per second on Linux

const fail = (msg) => {
  console.error(`FAIL: ${msg}`);
  process.exitCode = 1;
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const listen = (server) => new Promise((r) => server.listen(0, '127.0.0.1', () => r(server.address().port)));
const randomPort = () => 30000 + Math.floor(Math.random() * 30000);

// --- stand-ins and helpers ---------------------------------------------------------------------------
// Stand-in for the Pages function: mints device certificates exactly like functions/api/oauth/exchange.ts.
const signer = generateKeyPairSync('ed25519');
const signerPublic = signer.publicKey.export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
const mint = (deviceKey, id, login, exp = Math.floor(Date.now() / 1000) + 3600) => {
  const payload = Buffer.from(JSON.stringify({ v: 1, kid: 'e2e', sub: id, login, dpk: deviceKey, exp }));
  return `${payload.toString('base64url')}.${sign(null, payload, signer.privateKey).toString('base64url')}`;
};
// The server's id is its public key; the client pins it, and the signed challenge names it.
const serverIdOf = (configDir) => {
  const seed = readFileSync(join(configDir, 'server_key'));
  const der = Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]);
  return createPublicKey(createPrivateKey({ key: der, format: 'der', type: 'pkcs8' }))
    .export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
};
// The real client build (pnpm build), served with the single-page-app fallback.
const appDist = resolve(here, '../dist');
const appServer = createServer((req, res) => {
  const path = req.url.split('?')[0];
  let file = join(appDist, path);
  if (!file.startsWith(appDist) || !existsSync(file) || path === '/') file = join(appDist, 'index.html');
  res.writeHead(200, { 'Content-Type': mime[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css' };
const dist = join(here, 'dist');
const statics = createServer((req, res) => {
  const file = join(dist, req.url === '/' ? 'index.html' : req.url.split('?')[0]);
  if (!file.startsWith(dist) || !existsSync(file)) return res.writeHead(404).end();
  res.writeHead(200, { 'Content-Type': mime[extname(file)] ?? 'application/octet-stream' }).end(readFileSync(file));
});

function procStat(pid) {
  const status = readFileSync(`/proc/${pid}/status`, 'utf8');
  const rssKb = Number(/VmRSS:\s+(\d+)/.exec(status)[1]);
  const stat = readFileSync(`/proc/${pid}/stat`, 'utf8').split(') ')[1].split(' ');
  return { rssMb: rssKb / 1024, cpuSec: (Number(stat[11]) + Number(stat[12])) / TICKS };
}

async function waitFor(fn, what, ms = 15000) {
  const start = Date.now();
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${what}`);
    await sleep(100);
  }
}

// --- run ---------------------------------------------------------------------------------------------
if (!existsSync(BIN)) throw new Error(`server binary not found: ${BIN}`);
if (!existsSync(join(appDist, 'index.html'))) throw new Error('client not built: run pnpm build');
if (!existsSync(join(dist, 'index.html'))) throw new Error('harness not built: run vite build -c e2e/vite.config.ts');

const webPort = await listen(statics);
const appPort = await listen(appServer);
const home = mkdtempSync(join(tmpdir(), 'termenbox-e2e-'));
const httpPort = randomPort();
const udpPort = randomPort();
const claim = randomBytes(32).toString('base64url');

const server = spawn(BIN, [], {
  env: {
    ...process.env,
    TMUX_TMPDIR: home, // a private tmux server: never touches the developer's workspaces
    TERMENBOX_CONFIG_DIR: join(home, 'config'),
    TERMENBOX_HOST: '127.0.0.1',
    TERMENBOX_PORT: String(httpPort),
    TERMENBOX_UDP_PORT: String(udpPort),
    TERMENBOX_PUBLIC_IP: '127.0.0.1',
    TERMENBOX_DIRECTORY_URL: '',
    TERMENBOX_DEV_INFO: 'on',
    TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`,
    LANG: 'C', // workspaces must still get a UTF-8 locale from the server
    LC_ALL: 'C',
    TERMENBOX_CLAIM_SECRET: claim,
  },
  stdio: ['ignore', 'inherit', 'inherit'],
});

let browser;
try {
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
  const info = await (await fetch(`http://127.0.0.1:${httpPort}/rtc-info`)).json();
  const record = { ip: info.ip, udp_port: info.port, fingerprint: info.fingerprint };
  const serverId = serverIdOf(join(home, 'config'));

  browser = await launch(['--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection']);
  const open = async (who, claimSecret, pinnedId = serverId) => {
    const page = await (await browser.newContext()).newPage();
    page.on('pageerror', (e) => fail(`page error: ${e.message}`));
    await page.goto(`http://127.0.0.1:${webPort}/`);
    const cert = mint(await page.evaluate(() => window.__e2e.deviceKey()), who.id, who.login);
    await page.evaluate((o) => window.__e2e.start(o), { record, cert, serverId: pinnedId, claim: claimSecret });
    return page;
  };
  const e2e = (page, fn, arg) => page.evaluate(fn, arg);

  // 1. Claim: the first GitHub user to present the secret becomes the owner, over WebRTC.
  const alice = await open({ id: 1, login: 'alice' }, claim);
  await waitFor(() => e2e(alice, () => window.__e2e.state === 'connected'), 'alice connected');
  if ((await e2e(alice, () => window.__e2e.kind())) !== 'rtc') fail('expected the WebRTC transport');
  console.log('ok  login and claim over WebRTC');

  // 2. Someone else is refused.
  const bob = await open({ id: 2, login: 'bob' });
  await waitFor(() => e2e(bob, () => window.__e2e.fatal !== ''), 'bob refused');
  console.log('ok  a second GitHub user is refused');

  // 2b. A server that relays another server's challenge gets no signature: the client signs only for the id it pinned.
  const relayed = await open({ id: 1, login: 'alice' }, undefined, 'not-this-server');
  await waitFor(() => e2e(relayed, () => window.__e2e.fatal.includes('not the one you added')), 'pinned id enforced');
  console.log('ok  the challenge must name the server the client pinned');

  // 2c. Soft keyboard: with a faked visualViewport (what iOS reports), the key row must stay inside the
  // visible area both when the viewport only shrinks (Android) and when Safari also pans it (iPhone).
  {
    const ctx = await browser.newContext({ viewport: { width: 390, height: 800 }, hasTouch: true, isMobile: true });
    await ctx.addInitScript(() => {
      const vv = Object.assign(new EventTarget(), { height: innerHeight, width: innerWidth, offsetTop: 0, offsetLeft: 0, scale: 1 });
      Object.defineProperty(window, 'visualViewport', { value: vv });
      window.__vv = (patch, ev) => { Object.assign(vv, patch); vv.dispatchEvent(new Event(ev)); };
    });
    const page = await ctx.newPage();
    page.on('pageerror', (e) => fail(`page error: ${e.message}`));
    await page.goto(`http://127.0.0.1:${webPort}/shell.html`);
    const keyRowInView = () =>
      page.evaluate(() => {
        const r = document.querySelector('[role=toolbar]').getBoundingClientRect();
        const vv = window.visualViewport;
        return { top: r.top, bottom: r.bottom, headerTop: document.querySelector('header').getBoundingClientRect().top,
          toastTop: document.querySelector('.toast').getBoundingClientRect().top,
          toastBottom: document.querySelector('.toast').getBoundingClientRect().bottom, viewTop: vv.offsetTop, viewBottom: vv.offsetTop + vv.height };
      });
    // Flush with the keyboard (no gap below, nothing cut off), and the header still at the top of the view.
    // The toast must stay inside the visible area too (it used to follow the layout viewport and slid out of view).
    const inside = (m) => Math.abs(m.bottom - m.viewBottom) <= 1 && m.top >= m.viewTop - 0.5 && Math.abs(m.headerTop - m.viewTop) <= 1 && m.toastTop >= m.viewTop - 0.5 && m.toastBottom <= m.viewBottom + 0.5;
    const closed = await keyRowInView();
    if (!inside(closed)) fail(`key row outside the view with no keyboard: ${JSON.stringify(closed)}`);
    await page.evaluate(() => window.__vv({ height: 800 - 336 }, 'resize'));
    const shrunk = await keyRowInView();
    if (!inside(shrunk)) fail(`key row hidden when the viewport shrinks: ${JSON.stringify(shrunk)}`);
    await page.evaluate(() => window.__vv({ height: 800 - 336 - 44, offsetTop: 180 }, 'scroll'));
    const panned = await keyRowInView();
    if (!inside(panned)) fail(`key row hidden when Safari pans the viewport: ${JSON.stringify(panned)}`);
    // A dialog opened while the view is panned must sit inside the visible area as well.
    await page.evaluate(() => window.__showDialog());
    const card = await page.evaluate(() => {
      const r = document.querySelector('[role=dialog][aria-label="Report a problem"] .card').getBoundingClientRect();
      const vv = window.visualViewport;
      return { top: r.top, bottom: r.bottom, viewTop: vv.offsetTop, viewBottom: vv.offsetTop + vv.height };
    });
    if (card.top < card.viewTop - 0.5 || card.bottom > card.viewBottom + 0.5) fail(`dialog outside the visible area: ${JSON.stringify(card)}`);
    await page.evaluate(() => window.__hideDialog());
    await page.evaluate(() => window.__vv({ height: 800, offsetTop: 0 }, 'resize'));
    if (!inside(await keyRowInView())) fail('key row not restored after the keyboard closed');
    // Toggle keys must return to their normal colour after a tap on a touch screen (no sticky :hover).
    const ctrl = page.getByRole('button', { name: 'Ctrl' });
    const bg = () => ctrl.evaluate((el) => getComputedStyle(el).backgroundColor);
    const normal = await bg();
    await ctrl.tap();
    await sleep(400); // the colour change is a 150 ms transition
    const on = await bg();
    await ctrl.tap();
    await sleep(400);
    const off = await bg();
    if (on === normal) fail('Ctrl does not show it is on');
    if (off !== normal) fail(`Ctrl stays highlighted after being toggled off: ${off} instead of ${normal}`);
    await ctx.close();
    console.log('ok  the key row stays visible when the keyboard opens, shrinks the view or pans it');
  }

  // 3. Create a workspace, type, see the output.
  await e2e(alice, () => window.__e2e.send({ type: 'new_session', name: 's1' }));
  await waitFor(() => e2e(alice, () => (window.__e2e.send({ type: 'list_sessions' }), window.__e2e.workspaces.length > 0)), 'workspace listed');
  const id = await e2e(alice, () => window.__e2e.workspaces[0].id);
  await e2e(alice, (id) => window.__e2e.send({ type: 'attach', session_id: id }), id);
  await sleep(500);
  await e2e(alice, () => window.__e2e.type('echo marker-$((20+22))\r'));
  await waitFor(() => e2e(alice, () => window.__e2e.output.includes('marker-42')), 'typed output');
  console.log('ok  typed a command and saw its output');

  // 4. Drop the transport; the client must reconnect, re-attach, and the screen must come back.
  await e2e(alice, () => { window.__e2e.clear(); window.__e2e.dropTransport(); });
  await waitFor(() => e2e(alice, () => window.__e2e.ready >= 2 && window.__e2e.state === 'connected'), 'reconnected', 60000);
  await e2e(alice, (id) => window.__e2e.send({ type: 'attach', session_id: id }), id);
  await waitFor(() => e2e(alice, () => window.__e2e.output.includes('marker-42')), 'screen replay after reconnect');
  console.log('ok  reconnected and the earlier screen was replayed');

  // History (U12): the server answers a range query and a delete over the real transport. No agent ran, so it is empty.
  await e2e(alice, () => window.__e2e.send({ type: 'history', from: 0, to: Date.now() + 1000 }));
  await waitFor(() => e2e(alice, () => window.__e2e.received.some((m) => m.includes('"type":"history"') && m.includes('"runs":[]') && m.includes('"keep_days":90'))), 'history answer');
  await e2e(alice, () => window.__e2e.send({ type: 'history_delete' }));
  await waitFor(() => e2e(alice, () => window.__e2e.received.some((m) => m.includes('"type":"history_deleted"') && m.includes('"removed":0'))), 'history deleted');
  console.log('ok  the history range query and delete are answered over the real transport');

  // 5. Gate: server RSS with 10 workspaces open.
  for (let i = 2; i <= 10; i++) {
    await e2e(alice, (n) => window.__e2e.send({ type: 'new_session', name: `s${n}` }), i);
    await sleep(150);
  }
  await waitFor(() => e2e(alice, () => (window.__e2e.send({ type: 'list_sessions' }), window.__e2e.workspaces.length >= 10)), '10 workspaces');
  const ids = await e2e(alice, () => window.__e2e.workspaces.map((s) => s.id));
  for (const sid of ids) {
    await e2e(alice, (sid) => window.__e2e.send({ type: 'attach', session_id: sid }), sid);
    await sleep(150);
  }
  const rss = procStat(server.pid).rssMb;
  console.log(`    server RSS with ${ids.length} sessions: ${rss.toFixed(1)} MB (limit ${LIMITS.rssMb})`);
  if (rss > LIMITS.rssMb) fail(`server RSS ${rss.toFixed(1)} MB exceeds ${LIMITS.rssMb} MB`);

  // 6. Gate: 10 s of continuous terminal output must cost the server under half a CPU. (tmux repaints the
  // screen instead of forwarding every line, so the check is on CPU over time, not on line counts.)
  await e2e(alice, () => window.__e2e.clear());
  const before = procStat(server.pid).cpuSec;
  const t0 = Date.now();
  await e2e(alice, () => window.__e2e.type("timeout 10 sh -c 'while :; do seq 1 5000; done'; echo end-$((40+2))\r"));
  await waitFor(() => e2e(alice, () => window.__e2e.output.includes('end-42')), 'throughput output', 60000);
  const wall = (Date.now() - t0) / 1000;
  const cpu = procStat(server.pid).cpuSec - before;
  const bytes = await e2e(alice, () => window.__e2e.output.length);
  const share = cpu / wall;
  console.log(`    10 s of output (${(bytes / 1048576).toFixed(1)} MB reached the browser): ${cpu.toFixed(2)} CPU-s in ${wall.toFixed(1)} s = ${(share * 100).toFixed(0)}% of one vCPU (limit ${LIMITS.cpuShare * 100}%)`);
  if (bytes < 100000) fail(`only ${bytes} bytes of output reached the browser`);
  if (share > LIMITS.cpuShare) fail(`relaying output used ${(share * 100).toFixed(0)}% of a vCPU, over ${LIMITS.cpuShare * 100}%`);

  // 7. The real app on a phone: connect, open a workspace, go back to the server list and connect again.
  // Each lap used to risk a hang or a broken screen, so run several and fail on any page error.
  {
    const paneClients = () => execFileSync('tmux', ['-L', 'termenbox', 'list-clients', '-F', '#{client_control_mode} #{client_pid}'], { env: { ...process.env, TMUX_TMPDIR: home }, encoding: 'utf8' }).trim().split('\n').filter((l) => l.startsWith('0 ')).length; // the server's own control-mode clients (window updates) are not terminals
    const backgroundClients = paneClients();
    const ctx = await browser.newContext({ viewport: { width: 390, height: 800 }, hasTouch: true, isMobile: true, permissions: ['clipboard-read', 'clipboard-write'] });
    // A counting stand-in for Web Audio, so the bell can be checked without a sound card.
    await ctx.addInitScript(() => {
      window.__beeps = 0;
      window.AudioContext = class {
        state = 'suspended';
        currentTime = 0;
        destination = {};
        resume() { this.state = 'running'; return Promise.resolve(); }
        createOscillator() { window.__beeps++; return { type: '', frequency: { value: 0 }, connect: (x) => x, start() {}, stop() {} }; }
        createGain() { return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect: (x) => x }; }
      };
    });
    const page = await ctx.newPage();
    const errors = [];
    // Pretend the status bar, Dynamic Island and microphone indicator take 47 px at the top.
    await ctx.addInitScript(() => document.addEventListener('DOMContentLoaded', () => {
      const style = document.createElement('style');
      style.textContent = ':root { --safe-top: 47px; }';
      document.head.appendChild(style);
    }));
    page.on('pageerror', (e) => errors.push(e.message));
    page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
    await page.goto(`http://127.0.0.1:${appPort}/`);
    const deviceKey = await page.evaluate(async () => {
      const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
      await new Promise((ok, no) => {
        const r = indexedDB.open('termenbox', 1);
        r.onupgradeneeded = () => r.result.createObjectStore('device');
        r.onsuccess = () => {
          const tx = r.result.transaction('device', 'readwrite');
          tx.objectStore('device').put(pair, 'key');
          tx.oncomplete = ok;
          tx.onerror = () => no(tx.error);
        };
        r.onerror = () => no(r.error);
      });
      const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
      return btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
    });
    await page.evaluate(
      ([cert, item]) => {
        localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login: 'alice' }));
        localStorage.setItem('termenbox_servers', JSON.stringify([item]));
      },
      [mint(deviceKey, 1, 'alice'), { server_url: '', server_id: serverId, server_name: 'e2e-server', registered_at: Date.now(), record }],
    );
    await page.reload();
    if (await page.getByText('Your Servers').count()) fail('saved servers opened the old Connect screen');
    // Servers and Report a problem live in Settings now (U04): the bar's Settings, then the row.
    const openServers = async () => {
      await page.locator('[data-shell=bottom-bar] [data-destination=home]').click();
      await page.locator('[data-shell=bottom-bar] [data-destination=settings]').click();
      await page.getByRole('button', { name: 'Servers', exact: true }).click();
    };
    await openServers();
    // Add Server: where workspaces run. The container is the default; the host modes show a warning and need consent.
    await page.getByRole('button', { name: 'Add Server' }).first().click();
    const cmd = () => page.locator('pre.command').first().textContent();
    // The default is this machine as the current user (host-me); the command waits for consent, which is on the first screen.
    if (await page.locator('pre.command').count()) fail('a host-mode command was shown before consent');
    await page.getByTestId('consent').getByRole('checkbox').check();
    if (!/^curl -fsSL http\S+\/i \| sh$/.test((await cmd()) ?? '')) fail(`the default command is not the bare host-me install: ${await cmd()}`);
    await page.getByTestId('install-options').locator('summary').click(); // the other run modes are folded under Options (decision 0127)
    if (!(await page.getByText('This gives your browser a shell on your server').isVisible())) fail('the host-mode warning is missing');
    await page.getByRole('radio', { name: /As a dedicated user/ }).check();
    if (await page.locator('pre.command').count()) fail('choosing another host mode did not ask for consent again');
    await page.getByTestId('consent').getByRole('checkbox').check();
    if (!/ host-user$/.test((await cmd()) ?? '')) fail(`the dedicated-user command is wrong: ${await cmd()}`);
    await page.getByRole('radio', { name: /As root/ }).check();
    await page.getByTestId('consent').getByRole('checkbox').check();
    if (!/ host$/.test((await cmd()) ?? '')) fail(`the root command is wrong: ${await cmd()}`);
    await page.getByRole('radio', { name: /In an isolated container/ }).check();
    if (!/ \| sh -s -- container$/.test((await cmd()) ?? '')) fail(`the container command is wrong: ${await cmd()}`);
    await page.getByRole('button', { name: 'Close', exact: true }).click();

    // Report a problem: shows scrubbed diagnostics and opens a prefilled issue on the project's repository.
    await page.evaluate(() => {
      window.__opened = [];
      window.open = (u) => (window.__opened.push(String(u)), null);
    });
    await page.locator('[data-shell=bottom-bar] [data-destination=home]').click(); // off the servers page
    await page.locator('[data-shell=bottom-bar] [data-destination=settings]').click();
    await page.getByRole('button', { name: 'Report a problem', exact: true }).click();
    const reportText = await page.locator('[role=dialog][aria-label="Report a problem"] textarea.diagnostics').inputValue();
    if (!reportText.includes('app:') || !reportText.includes('window: 390x')) fail(`report text is missing diagnostics: ${reportText.slice(0, 120)}`);
    await page.getByRole('button', { name: 'Open GitHub issue' }).click();
    const opened = await page.evaluate(() => window.__opened[0] ?? '');
    if (!opened.startsWith('https://github.com/termenbox/termenbox/issues/new?template=problem.yml&diagnostics=')) fail(`unexpected report URL: ${opened.slice(0, 120)}`);
    if (await page.locator('[role=dialog][aria-label="Report a problem"]').count()) fail('the report dialog stayed open');
    // Removing a server asks first; declining keeps it.
    await openServers();
    page.once('dialog', (d) => d.dismiss());
    await page.getByRole('button', { name: /^Menu of / }).click();
    await page.getByRole('menuitem', { name: 'Remove' }).click();
    if (!(await page.getByRole('heading', { name: 'e2e-server' }).isVisible())) fail('the server was removed without confirmation');
    for (let lap = 1; lap <= 3; lap++) {
      await page.locator('[data-shell=bottom-bar] [data-destination=workspaces]').click(); // Home is the landing page now (U05)
      // The Workspaces destination lists workspaces in groups (U05); the row's name starts with its state and agent icons.
      const s1Row = page.locator('[data-workspace-groups] button').filter({ has: page.locator('b', { hasText: /^s1$/ }) }).first();
      const sheetRow = page.locator('.workspace-sheet').getByRole('button', { name: /^s1(\s|$)/ }).first(); // with a workspace open, the list is the sheet
      if (!(await s1Row.isVisible())) await page.locator('.workspaces-button').click();
      await s1Row.or(sheetRow).first().waitFor();
      await s1Row.or(sheetRow).first().click();
      await page.locator('.dot.ok').waitFor({ timeout: 20000 }).catch(async (e) => {
        console.log('page errors:', errors, '\n', (await page.locator('body').innerText()).slice(0, 400));
        throw e;
      });
      // The toast must clear the status bar area, and the title must not trigger double-tap zoom.
      const toastTop = await page.evaluate(() => document.querySelector('.toast')?.getBoundingClientRect().top ?? null);
      if (toastTop !== null && toastTop < 47) fail(`the toast is under the status bar: top ${toastTop}`);
      const taps = await page.evaluate(() => [getComputedStyle(document.querySelector('button[aria-label=Menu]')).touchAction, getComputedStyle(document.body).touchAction]);
      if (taps.some((t) => t !== 'manipulation')) fail(`double-tap zoom is not disabled: ${taps}`);
      // Connecting lands in a live terminal: the hidden input has focus without any tap.
      await page.waitForFunction(() => document.activeElement?.classList.contains('xterm-helper-textarea'), null, { timeout: 5000 });
      await page.getByRole('tab', { selected: true }).waitFor({ timeout: 5000 });
      // #99: iOS zooms the page into a focused field under 16px, and the screen then sits shifted. The terminal's
      // input must be 16px or more, the page must stay at scale 1, and nothing around the terminal may be scrolled.
      const lay = await page.evaluate(() => {
        const sc = (q) => { const e = document.querySelector(q); return e ? e.scrollLeft + e.scrollTop : 0; };
        return { font: parseFloat(getComputedStyle(document.querySelector('.xterm-helper-textarea')).fontSize), scale: window.visualViewport.scale,
          scrolled: ['.shell', 'main', '.workspace', '.terminal-pane', '.screen', '.xterm'].map(sc).some(Boolean) || window.scrollX + window.scrollY > 0 };
      });
      if (lay.font < 16) fail(`the terminal input is ${lay.font}px: iOS would zoom the page when it gets focus`);
      if (lay.scale !== 1) fail(`the page is zoomed (${lay.scale}) after connecting`);
      if (lay.scrolled) fail('a box around the terminal is scrolled after connecting');
      await page.locator('.workspaces-button').click();
      const heights = await page.evaluate(() => [...document.querySelectorAll('.sheet-header button.icon')].map((b) => Math.round(b.getBoundingClientRect().height)));
      if (heights.length !== 2 || heights[0] !== heights[1]) fail(`Refresh and New differ in size: ${heights}`);
      if (!(await page.getByText(/server 0\.1\.\d+\+/).count())) fail('the sidebar does not show the server version');
      // #105: every hub update (an agent changing state, a workspace list) used to rebuild the terminal, which blanked and
      // flickered the screen. Refresh makes the server answer with a workspace list; the same xterm must survive it.
      await page.evaluate(() => { document.querySelector('.xterm').dataset.same = '1'; });
      await page.locator('.workspace-sheet').getByRole('button', { name: 'Refresh', exact: true }).click();
      await sleep(600);
      if (!(await page.evaluate(() => document.querySelector('.xterm')?.dataset.same === '1'))) fail('the terminal was rebuilt by a hub update');
      await page.locator('.workspace-sheet').getByRole('button', { name: /^s1(\s|$)/ }).first().click(); // a tab now reads as the directory
      await page.locator('.xterm').waitFor({ timeout: 5000 });
      if (lap === 3) {
        // The swipe-typing recorder: silent normally, listing the textarea's events when the debug switch is on.
        if (await page.locator('.log').count()) fail('the input log shows without the debug switch');
        await page.evaluate(() => {
          localStorage.setItem('termenbox_debug', '1');
          window.dispatchEvent(new Event('termenbox-debug'));
        });
        const debugTop = await page.evaluate(() => document.querySelector('.debug')?.getBoundingClientRect().top ?? null);
        if (debugTop === null || debugTop < 47) fail(`the debug overlay is under the status bar: top ${debugTop}`);
        await page.locator('.xterm-helper-textarea').focus();
        await page.keyboard.type('hi');
        await page.waitForFunction(() => /keydown key="h"/.test(document.querySelector('.log pre')?.textContent ?? ''), null, { timeout: 5000 });
        // The panel is part of the layout and its buttons work: Copy puts the lines on the clipboard, Clear empties them.
        const box = await page.locator('.log .bar').boundingBox();
        if (!box || box.y < 0) fail(`the log buttons are off screen: ${JSON.stringify(box)}`);
        await page.locator('.log').getByRole('button', { name: 'Copy', exact: true }).click();
        const copied = await page.evaluate(() => navigator.clipboard.readText());
        if (!/keydown key="h"/.test(copied)) fail(`Copy did not copy the log: ${copied.slice(0, 80)}`);
        await page.locator('.log').getByRole('button', { name: 'Clear', exact: true }).click();
        // Using the panel's buttons must not take focus from the terminal.
        if (!(await page.evaluate(() => document.activeElement?.classList.contains('xterm-helper-textarea')))) fail('Copy/Clear took focus from the terminal');
        await page.waitForFunction(() => /input events \(0\)/.test(document.querySelector('.log .bar')?.textContent ?? ''), null, { timeout: 3000 });
        // iOS swipe typing: picking another suggestion fires insertReplacementText with the old word selected
        // (sequence captured on an iPhone, fact 0027). The terminal must end up with the replacement.
        await page.locator('.xterm-helper-textarea').focus();
        await page.keyboard.press('Control+u'); // clear the "hi" typed above
        await page.keyboard.type('echo hello');
        await page.evaluate(() => {
          const area = document.querySelector('.xterm-helper-textarea');
          area.value = 'hello';
          area.setSelectionRange(0, 5);
          area.dispatchEvent(new InputEvent('beforeinput', { inputType: 'insertReplacementText', data: 'world', bubbles: true, cancelable: true }));
        });
        await page.keyboard.press('Enter');
        await waitFor(
          () => /^world$/m.test(execFileSync('tmux', ['-L', 'termenbox', 'capture-pane', '-p', '-t', 's1'], { env: { ...process.env, TMUX_TMPDIR: home }, encoding: 'utf8' })),
          'the replacement word in the shell output',
          10000,
        ).catch((e) => {
          console.log('pane:', execFileSync('tmux', ['-L', 'termenbox', 'capture-pane', '-p', '-t', 's1'], { env: { ...process.env, TMUX_TMPDIR: home }, encoding: 'utf8' }).split('\n').filter(Boolean).slice(-6));
          throw e;
        });
        const pane = execFileSync('tmux', ['-L', 'termenbox', 'capture-pane', '-p', '-t', 's1'], { env: { ...process.env, TMUX_TMPDIR: home }, encoding: 'utf8' });
        if (/^hello$/m.test(pane)) fail('the old word reached the shell as well');
        // Other alphabets: Cyrillic typed on the phone must reach the shell as text, not as octal bytes.
        await page.keyboard.press('Control+u');
        await page.keyboard.type('echo привет');
        await page.keyboard.press('Enter');
        const utf8 = { ...process.env, TMUX_TMPDIR: home, LANG: 'C.UTF-8', LC_ALL: 'C.UTF-8' };
        await waitFor(() => /^привет$/m.test(execFileSync('tmux', ['-L', 'termenbox', 'capture-pane', '-p', '-t', 's1'], { env: utf8, encoding: 'utf8' })), 'Cyrillic text in the shell output', 10000);
        // Russian iOS keyboard: a swiped word arrives as keydown key="чем" followed by insertText (captured on an iPhone).
        // xterm drops such a word unless the keydown is kept from it.
        await page.keyboard.press('Control+u');
        await page.keyboard.type('echo ');
        await page.evaluate(() => {
          const area = document.querySelector('.xterm-helper-textarea');
          area.dispatchEvent(new KeyboardEvent('keydown', { key: 'чем', bubbles: true, cancelable: true }));
          area.value = 'чем';
          area.dispatchEvent(new InputEvent('beforeinput', { inputType: 'insertText', data: 'чем', bubbles: true, cancelable: true, composed: true }));
          area.dispatchEvent(new InputEvent('input', { inputType: 'insertText', data: 'чем', bubbles: true, composed: true })); // browsers' own events are composed
          area.dispatchEvent(new KeyboardEvent('keyup', { key: 'чем', bubbles: true }));
        });
        await page.keyboard.press('Enter');
        await waitFor(() => /^чем$/m.test(execFileSync('tmux', ['-L', 'termenbox', 'capture-pane', '-p', '-t', 's1'], { env: utf8, encoding: 'utf8' })), 'the swiped Cyrillic word in the shell output', 8000);
        // The hidden input mirrors the last two words typed (iOS builds its suggestions from it), never stale text.
        // The mirror writes in a task (after beforeinput's default action), so poll instead of reading immediately.
        const mirrorValue = () => page.evaluate(() => document.querySelector('.xterm-helper-textarea').value);
        await page.keyboard.press('Control+u');
        await page.evaluate(() => { document.querySelector('.xterm-helper-textarea').value = 'утромпа'; });
        await page.keyboard.type('па');
        await waitFor(async () => (await mirrorValue()) === 'па', 'the hidden input to hold the typed word', 3000);
        const mirrored = await mirrorValue();
        if (mirrored !== 'па') fail(`the hidden input holds ${JSON.stringify(mirrored)} instead of the typed word`);
        await page.keyboard.press('Backspace');
        await waitFor(async () => (await mirrorValue()) === 'п', 'Backspace to shorten the mirrored word', 3000);
        if ((await mirrorValue()) !== 'п') fail('Backspace did not shorten the mirrored word');
        await page.keyboard.type(' x');
        await waitFor(async () => (await mirrorValue()) === 'п x', 'the hidden input to hold the last two words', 3000);
        if ((await mirrorValue()) !== 'п x') fail('the hidden input does not hold the last two words');
        // The dictation language chip (only where the browser has a speech recognizer) cycles auto -> EN -> RU -> auto.
        if (await page.$('[data-testid="voice-lang"]')) {
          const chip = () => page.evaluate(() => document.querySelector('[data-testid="voice-lang"]').textContent.trim());
          const labels = [await chip()];
          for (let i = 0; i < 3; i++) {
            await page.evaluate(() => document.querySelector('[data-testid="voice-lang"]').dispatchEvent(new PointerEvent('pointerdown', { bubbles: true, cancelable: true })));
            labels.push(await chip());
          }
          if (labels.join(',') !== '🌐,EN,RU,🌐') fail(`the dictation language chip cycled ${labels.join(',')}`);
        }
        // iOS now answers a tapped suggestion with the missing tail (captured: "па" + "спорт"); it is typed as it comes.
        await page.keyboard.press('Control+u');
        await page.keyboard.type('echo па');
        await page.evaluate(() => {
          const area = document.querySelector('.xterm-helper-textarea');
          area.dispatchEvent(new InputEvent('beforeinput', { inputType: 'insertText', data: 'спорт', bubbles: true, cancelable: true, composed: true }));
          area.value = 'паспорт';
          area.dispatchEvent(new InputEvent('input', { inputType: 'insertText', data: 'спорт', bubbles: true, composed: true }));
        });
        await page.keyboard.press('Enter');
        await waitFor(() => /^паспорт$/m.test(execFileSync('tmux', ['-L', 'termenbox', 'capture-pane', '-p', '-t', 's1'], { env: utf8, encoding: 'utf8' })), 'the suggestion tail completing the typed word', 8000);
        // A suggestion that completes the typed word ("у" -> "утро") arrives as new text; the typed part must go first.
        await page.keyboard.press('Control+u');
        await page.keyboard.type('echo у');
        await page.evaluate(() => {
          const area = document.querySelector('.xterm-helper-textarea');
          area.value = '';
          area.dispatchEvent(new InputEvent('beforeinput', { inputType: 'insertText', data: 'утро', bubbles: true, cancelable: true, composed: true }));
          area.value = 'утро';
          area.dispatchEvent(new InputEvent('input', { inputType: 'insertText', data: 'утро', bubbles: true, composed: true }));
        });
        await page.keyboard.press('Enter');
        await waitFor(() => /^утро$/m.test(execFileSync('tmux', ['-L', 'termenbox', 'capture-pane', '-p', '-t', 's1'], { env: utf8, encoding: 'utf8' })), 'the completed word without the typed letter', 8000);
        if (/^уутро$/m.test(execFileSync('tmux', ['-L', 'termenbox', 'capture-pane', '-p', '-t', 's1'], { env: utf8, encoding: 'utf8' }))) fail('the typed letter stayed in front of the suggestion');
        // Hiding the keyboard leaves the terminal active (focused, no keyboard); a tap brings the keyboard back.
        await page.evaluate(() => document.querySelector('.xterm-helper-textarea').blur());
        await page.waitForFunction(() => document.activeElement?.classList.contains('xterm-helper-textarea') && document.activeElement.getAttribute('inputmode') === 'none', null, { timeout: 3000 }).catch(() => fail('the terminal went inactive when the keyboard was hidden'));
        await page.locator('.screen').click({ position: { x: 40, y: 40 } });
        await page.waitForFunction(() => document.activeElement?.getAttribute('inputmode') === 'text', null, { timeout: 3000 }).catch(() => fail('a tap did not bring the keyboard back'));
        // The terminal bell (what `printf '\a'` and agents ring) becomes a beep once audio was unlocked by a tap.
        await page.keyboard.press('Control+u');
        await page.keyboard.type("printf '\\a'");
        await page.keyboard.press('Enter');
        await page.waitForFunction(() => window.__beeps > 0, null, { timeout: 8000 }).catch(() => fail('the terminal bell did not make a sound'));
        // Fn: a keyboard grid takes the system keyboard's place (and sends keys), ABC brings the system keyboard back.
        await page.keyboard.press('Control+u');
        await page.keyboard.type('echo ');
        await page.getByRole('button', { name: 'Fn', exact: true }).click();
        const fnKb = page.getByRole('group', { name: 'Function keyboard' });
        await fnKb.waitFor({ timeout: 3000 });
        const fnBox = await fnKb.boundingBox();
        if (!fnBox || fnBox.height < 200) fail(`the Fn keyboard is too small to replace the system keyboard: ${fnBox?.height}`);
        if ((await page.locator('.xterm-helper-textarea').getAttribute('inputmode')) !== 'none') fail('the system keyboard is not suppressed while Fn is open');
        await fnKb.getByRole('button', { name: 'Symbols', exact: true }).click();
        await fnKb.getByRole('button', { name: '{', exact: true }).click();
        await waitFor(() => /echo \{/.test(execFileSync('tmux', ['-L', 'termenbox', 'capture-pane', '-p', '-t', 's1'], { env: utf8, encoding: 'utf8' })), 'the symbol typed from the Fn keyboard', 8000);
        await fnKb.getByRole('button', { name: 'Back to the system keyboard' }).click();
        if (await page.getByRole('group', { name: 'Function keyboard' }).count()) fail('the Fn keyboard did not close');
        if ((await page.locator('.xterm-helper-textarea').getAttribute('inputmode')) !== 'text') fail('the system keyboard was not restored');
        // After the keyboard was hidden (focus lost), a tap on the terminal makes it active again.
        await page.evaluate(() => document.querySelector('.xterm-helper-textarea').blur());
        await page.locator('.screen').click({ position: { x: 40, y: 40 } });
        await page.waitForFunction(() => document.activeElement?.classList.contains('xterm-helper-textarea'), null, { timeout: 3000 });
        // Long-press selects a word, the bar copies it.
        await page.keyboard.press('Control+u');
        await page.keyboard.type('clear; echo SELECTME');
        await page.keyboard.press('Enter');
        await waitFor(() => /^SELECTME$/m.test(execFileSync('tmux', ['-L', 'termenbox', 'capture-pane', '-p', '-t', 's1'], { env: utf8, encoding: 'utf8' })), 'SELECTME on the screen', 8000);
        await sleep(400);
        // Where SELECTME is on the screen, from tmux: its row, and the client's total rows (including the status line).
        const tm = (...args) => execFileSync('tmux', ['-L', 'termenbox', ...args], { env: utf8, encoding: 'utf8' });
        const row = tm('capture-pane', '-p', '-t', 's1').split('\n').findIndex((l) => l === 'SELECTME');
        const total = Number(tm('list-clients', '-F', '#{client_control_mode} #{client_height}').split('\n').find((l) => l.startsWith('0 ')).slice(2));
        await page.evaluate(
          ([row, total]) => {
            const r = document.querySelector('.xterm-screen').getBoundingClientRect();
            window.__press = { x: r.left + (r.width / 80) * 2 + 2, y: r.top + (r.height / total) * (row + 0.5) };
          },
          [row, total],
        );
        await page.evaluate(() => {
          const el = document.querySelector('.screen');
          const init = { pointerId: 7, pointerType: 'touch', isPrimary: true, bubbles: true, clientX: window.__press.x, clientY: window.__press.y };
          el.dispatchEvent(new PointerEvent('pointerdown', init));
          setTimeout(() => el.dispatchEvent(new PointerEvent('pointerup', init)), 700);
        });
        await page.locator('.selbar').waitFor({ timeout: 4000 });
        if ((await page.locator('.screenbox .handle').count()) !== 2) fail('the selection handles are missing');
        await page.locator('.selbar').getByRole('button', { name: 'Copy', exact: true }).click();
        const picked = await page.evaluate(() => navigator.clipboard.readText());
        if (picked.trim() !== 'SELECTME') fail(`long-press copied ${JSON.stringify(picked)} instead of the word under the finger`);
        await page.keyboard.press('Control+u');
        // Select: the screen text is available as selectable text.
        await page.keyboard.type('echo привет');
        await page.keyboard.press('Enter');
        await sleep(500);
        await page.getByRole('button', { name: 'Fn', exact: true }).click(); // Select lives on the Fn panel's More page
        const moreKb = page.getByRole('group', { name: 'Function keyboard' });
        await moreKb.getByRole('button', { name: 'More', exact: true }).click();
        await moreKb.getByRole('button', { name: 'Select', exact: true }).click();
        await page.waitForFunction(() => /привет/.test(document.querySelector('[aria-label="Select text"] pre')?.textContent ?? ''), null, { timeout: 5000 });
        await page.getByRole('button', { name: 'Close', exact: true }).click();
        await moreKb.getByRole('button', { name: 'Back to the system keyboard' }).click();
        await page.evaluate(() => {
          localStorage.setItem('termenbox_debug', '0');
          window.dispatchEvent(new Event('termenbox-debug'));
        });
      }
      await page.locator('.workspaces-button').click();
      await page.getByRole('button', { name: 'Menu', exact: true }).click();
      await page.getByRole('dialog', { name: 'Menu', exact: true }).getByRole('button', { name: 'Servers', exact: true }).click();
      await page.getByText('Your Servers').waitFor({ timeout: 5000 });
      await waitFor(() => paneClients() === backgroundClients, 'no background PTY stream in Servers', 5000);
      console.log(`    lap ${lap}: connect, workspace, back to servers`);
    }
    if (errors.length) fail(`app flow raised errors: ${errors.slice(0, 3).join(' | ')}`);
    await ctx.close();
    console.log('ok  connect, open a workspace and return to the server list, three times, on a phone');
  }
} catch (e) {
  fail(e.stack ?? String(e));
} finally {
  await browser?.close().catch(() => {});
  server.kill();
  statics.close();
  rmSync(home, { recursive: true, force: true });
}
if (process.exitCode) process.exit(process.exitCode);
console.log('browser end-to-end: all checks passed');
process.exit(0);
