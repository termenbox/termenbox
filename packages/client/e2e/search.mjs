// R59 spike: where does text that has scrolled out of view live, and what could a Find search? Real Chromium, real
// termenbox-server, real tmux (the private socket); only the certificate issuer is a stand-in. The numbers it prints
// are recorded in docs/plans/assets/terminal-search/feasibility.md; the assertions fail CI if a premise there stops
// being true. It builds nothing for production: the "snapshot" is `capture-pane` run through `termenbox-server tmux`.
// Usage: node e2e/search.mjs   (needs a built server, `pnpm run build` and `pnpm run e2e:build`, tmux, and Chrome)
//   SEARCH_RESULTS=path   also writes the measurements as JSON
import { chromium } from 'playwright-core';
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, writeFileSync, existsSync, rmSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { gzipSync } from 'node:zlib';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const fail = (msg) => { console.error(`FAIL: ${msg}`); process.exitCode = 1; };
const check = (c, m) => { if (!c) fail(m); return !!c; };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const listen = (s) => new Promise((r) => s.listen(0, '127.0.0.1', () => r(s.address().port)));
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.json': 'application/json', '.png': 'image/png' };
const serve = (root, spa) => createServer((req, res) => {
  let f = join(root, req.url.split('?')[0].split('#')[0]);
  if (!f.startsWith(root)) return res.writeHead(404).end();
  if (existsSync(f) && statSync(f).isDirectory()) f = join(f, 'index.html');
  if (!existsSync(f)) { if (!spa) return res.writeHead(404).end(); f = join(root, 'index.html'); }
  res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
});
async function waitFor(fn, what, ms = 15000) {
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
const mint = (deviceKey, id, login) => {
  const payload = Buffer.from(JSON.stringify({ v: 1, kid: 'e2e', sub: id, login, dpk: deviceKey, exp: Math.floor(Date.now() / 1000) + 3600 }));
  return `${payload.toString('base64url')}.${sign(null, payload, signer.privateKey).toString('base64url')}`;
};
const serverIdOf = (configDir) => {
  const seed = readFileSync(join(configDir, 'server_key'));
  const der = Buffer.concat([Buffer.from('302e020100300506032b657004220420', 'hex'), seed]);
  return createPublicKey(createPrivateKey({ key: der, format: 'der', type: 'pkcs8' })).export({ format: 'der', type: 'spki' }).subarray(-32).toString('base64url');
};

if (!existsSync(BIN)) throw new Error(`server binary not found: ${BIN}`);
const appServer = serve(resolve(here, '../dist'), true);
const harness = serve(join(here, 'dist'), false);
const appPort = await listen(appServer);
const webPort = await listen(harness);
const home = mkdtempSync(join(tmpdir(), 'termenbox-search-'));
const httpPort = 30000 + Math.floor(Math.random() * 30000);
const claim = randomBytes(32).toString('base64url');
const env = {
  ...process.env, TMUX_TMPDIR: home, TERMENBOX_CONFIG_DIR: join(home, 'config'), TERMENBOX_HOST: '127.0.0.1', TERMENBOX_PORT: String(httpPort),
  TERMENBOX_UDP_PORT: String(httpPort + 1), TERMENBOX_PUBLIC_IP: '127.0.0.1', TERMENBOX_DIRECTORY_URL: '', TERMENBOX_DEV_INFO: 'on',
  TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`, LANG: 'C.UTF-8', LC_ALL: 'C.UTF-8', TERMENBOX_CLAIM_SECRET: claim, EDITOR: '', VISUAL: '',
};
const server = spawn(BIN, [], { env, stdio: ['ignore', 'inherit', 'inherit'] });
// Anything a server-side search would run: the same tmux, socket and config the server uses (decision 0120).
const tmux = (...args) => execFileSync(BIN, ['tmux', ...args], { env, encoding: 'utf8', maxBuffer: 256 << 20, stdio: ['ignore', 'pipe', 'pipe'] });
const timed = (fn) => { const t = performance.now(); const v = fn(); return [v, performance.now() - t]; };
const rssMb = (pid) => Number(/VmRSS:\s+(\d+)/.exec(readFileSync(`/proc/${pid}/status`, 'utf8'))[1]) / 1024;
const R = {}; // the measurements

let browser;
try {
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
  const info = await (await fetch(`http://127.0.0.1:${httpPort}/rtc-info`)).json();
  const record = { ip: info.ip, udp_port: info.port, fingerprint: info.fingerprint };
  const serverId = serverIdOf(join(home, 'config'));
  browser = await chromium.launch({
    ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: 'chrome' }),
    args: ['--no-sandbox', '--disable-features=WebRtcHideLocalIpsWithMdns', '--allow-loopback-in-peer-connection'],
  });

  // The harness page types into the session, as an agent's shell would receive it.
  const hp = await (await browser.newContext()).newPage();
  hp.on('pageerror', (e) => fail(`harness page error: ${e.message}`));
  await hp.goto(`http://127.0.0.1:${webPort}/`);
  const cert = mint(await hp.evaluate(() => window.__e2e.deviceKey()), 1, 'alice');
  await hp.evaluate((o) => window.__e2e.start(o), { record, cert, serverId, claim });
  await waitFor(() => hp.evaluate(() => window.__e2e.state === 'connected'), 'harness connected');
  const newSession = async (name) => {
    await hp.evaluate((n) => window.__e2e.send({ type: 'new_session', name: n }), name);
    return waitFor(() => hp.evaluate((n) => (window.__e2e.send({ type: 'list_sessions' }), window.__e2e.sessions.find((s) => s.name === n)?.id), name), `session ${name}`);
  };
  const id = await newSession('s1');
  await hp.evaluate((id) => window.__e2e.send({ type: 'attach', session_id: id }), id);
  await sleep(500);
  const run = async (cmd, marker) => {
    await hp.evaluate(() => window.__e2e.clear());
    await hp.evaluate((c) => window.__e2e.type(c), `${cmd}; echo ${marker}-$((1+1))\r`);
    await waitFor(() => hp.evaluate((m) => window.__e2e.output.includes(`${m}-2`), marker), `${marker} done`, 30000);
  };
  const target = `=${id}:`;
  const meta = (t = target) => {
    const [hs, hl, pane, win, alt, cols, rows, mode] = tmux('display-message', '-p', '-t', t, '#{history_size}|#{history_limit}|#{pane_id}|#{window_id}|#{alternate_on}|#{window_width}|#{window_height}|#{pane_in_mode}').trim().split('|');
    return { historySize: +hs, historyLimit: +hl, pane, window: win, alternateOn: alt === '1', cols: +cols, rows: +rows, inCopyMode: mode === '1' };
  };

  // The real app, on a phone, rendering with the DOM renderer so the visible rows can be read.
  const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true });
  await ctx.addInitScript(() => localStorage.setItem('termenbox_ios_hint', '1'));
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto(`http://127.0.0.1:${appPort}/`);
  const deviceKey = await page.evaluate(async () => {
    const pair = await crypto.subtle.generateKey({ name: 'ECDSA', namedCurve: 'P-256' }, false, ['sign', 'verify']);
    await new Promise((ok, no) => {
      const r = indexedDB.open('termenbox', 1);
      r.onupgradeneeded = () => r.result.createObjectStore('device');
      r.onsuccess = () => { const tx = r.result.transaction('device', 'readwrite'); tx.objectStore('device').put(pair, 'key'); tx.oncomplete = ok; tx.onerror = () => no(tx.error); };
      r.onerror = () => no(r.error);
    });
    const raw = new Uint8Array(await crypto.subtle.exportKey('raw', pair.publicKey));
    return btoa(String.fromCharCode(...raw)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  });
  await page.evaluate(([c, item]) => {
    localStorage.setItem('termenbox_auth', JSON.stringify({ cert: c, login: 'alice' }));
    localStorage.setItem('termenbox_servers', JSON.stringify([item]));
    localStorage.setItem('termenbox_settings', JSON.stringify({ renderer: 'dom' }));
  }, [mint(deviceKey, 1, 'alice'), { server_url: '', server_id: serverId, server_name: 'e2e-server', registered_at: Date.now(), record }]);
  const open = async (name) => {
    await page.reload();
    await page.locator('[data-shell=bottom-bar] [data-destination=sessions]').click();
    const row = page.locator('[data-session-groups] button').filter({ has: page.locator('b', { hasText: new RegExp(`^${name}$`) }) }).first();
    await row.waitFor({ timeout: 20000 });
    await row.click();
    await page.locator('.xterm-rows').waitFor({ timeout: 20000 });
    await sleep(800);
  };
  const screen = () => page.evaluate(() => [...document.querySelectorAll('.xterm-rows > div')].map((d) => d.textContent.replace(/\s+$/, '')));
  const tokens = (lines, re = /TOK-(\d{4})/) => lines.map((l) => re.exec(l)?.[1]).filter(Boolean).map(Number);
  await open('s1');

  // 1. Output that has left the screen, in the browser's view and in tmux.
  const N = 3000;
  await run(`printf 'TOK-%04d\\n' $(seq 1 ${N})`, 'printed');
  await sleep(500);
  const m1 = meta();
  // After a 3000-line burst the live view can stay blank on a slow runner (seen in CI, not locally): the rows were empty
  // 20 s later. A reattach repaints the current screen, so fall back to that and record that it was needed.
  let neededRepaint = false;
  try { await waitFor(async () => tokens(await screen()).includes(N), `the browser to paint TOK-${N}`, 8000); } catch {
    neededRepaint = true;
    await open('s1');
    await waitFor(async () => tokens(await screen()).includes(N), `the reattached browser to paint TOK-${N}`, 20000);
  }
  const shown = await screen();
  const seen = tokens(shown);
  R.viewport = { neededRepaint, rowsInDom: shown.length, tokenRange: [Math.min(...seen), Math.max(...seen)], tmuxRows: m1.rows, tmuxCols: m1.cols };
  check(seen.includes(N) && !seen.includes(1500), `the browser shows the newest lines only: ${seen[0]}..${seen.at(-1)}`);
  check(m1.historySize > 0 && m1.historyLimit === 2000 && !m1.alternateOn, `tmux holds history for the shell: ${JSON.stringify(m1)}`);
  console.log(`ok  1 browser DOM holds ${shown.length} rows (TOK-${R.viewport.tokenRange[0]}..${R.viewport.tokenRange[1]}); TOK-1500 is not in it`);
  // The server's own view: every client of the session shares one tmux history.
  const [snap, snapMs] = timed(() => tmux('capture-pane', '-p', '-J', '-S', '-', '-t', target));
  const snapTokens = tokens(snap.split('\n'));
  R.snapshot = { bytes: Buffer.byteLength(snap), gzipBytes: gzipSync(snap).length, lines: snap.split('\n').length, captureMs: +snapMs.toFixed(1), firstToken: snapTokens[0], lastToken: snapTokens.at(-1), historySize: m1.historySize, historyLimit: m1.historyLimit };
  check(snapTokens.includes(1500) && snapTokens.includes(N), 'tmux history holds a token that scrolled out of view');
  check(!snapTokens.includes(1), `history is bounded by history-limit: TOK-0001 should be gone, first is ${snapTokens[0]}`);
  console.log(`ok  1 tmux holds TOK-${snapTokens[0]}..${snapTokens.at(-1)} (${R.snapshot.lines} lines, ${R.snapshot.bytes} B, ${R.snapshot.captureMs} ms); older lines are dropped at history-limit ${m1.historyLimit}`);

  // 2. tmux's own search, driven by keystrokes through the same terminal (mouse on, emacs keys): C-b [ then C-r.
  const t0 = performance.now();
  await hp.evaluate(() => window.__e2e.type('\x02['));
  await waitFor(() => meta().inCopyMode, 'copy mode');
  for (const part of ['\x12', 'TOK-1500', '\r']) { await hp.evaluate((k) => window.__e2e.type(k), part); await sleep(300); }
  await waitFor(async () => tokens(await screen()).includes(1500), 'copy-mode search shows TOK-1500', 10000).catch(async (e) => {
    console.error(tmux('display-message','-p','-t',target,'[#{pane_search_string}] #{scroll_position}'), (await screen()).slice(-3).join('|'));
    throw e;
  });
  R.copyModeSearch = { toMatchMs: Math.round(performance.now() - t0) };
  const found = tokens(await screen());
  R.copyModeSearch.tokenRangeOnScreen = [Math.min(...found), Math.max(...found)];
  console.log(`ok  2 tmux copy-mode search brought TOK-1500 into the browser's rows in ${R.copyModeSearch.toMatchMs} ms (rows now TOK-${found[0]}..${found.at(-1)})`);
  await hp.evaluate(() => window.__e2e.type('q'));
  await waitFor(() => !meta().inCopyMode, 'leave copy mode');

  // 3. A browser-side search over the snapshot: cost in the page.
  const probe = await page.evaluate(async (text) => {
    const heap = () => (performance.memory ? performance.memory.usedJSHeapSize : 0);
    const before = heap();
    const t = performance.now();
    const lines = text.split('\n');
    const hits = [];
    const q = 'tok-1500';
    for (let i = 0; i < lines.length; i++) if (lines[i].toLowerCase().includes(q)) hits.push(i);
    return { ms: +(performance.now() - t).toFixed(2), hits: hits.length, lines: lines.length, heapDeltaKb: Math.round((heap() - before) / 1024) };
  }, snap);
  R.browserSearch = probe;
  check(probe.hits === 1, `the browser finds the token once in the snapshot: ${JSON.stringify(probe)}`);
  console.log(`ok  3 searching ${probe.lines} snapshot lines in the page: ${probe.ms} ms, ${probe.hits} hit`);

  // 4. Wrapped lines and Unicode.
  const pad = meta().cols - 4;
  await run(`printf '%*s' ${pad} '' | tr ' ' .; printf 'SPLIT%s\\n' TOKEN; printf 'UNI-\\303\\274-\\346\\227\\245\\346\\234\\254-\\360\\237\\230\\200-\\303\\251END\\n'`, 'wrapped');
  const plain = tmux('capture-pane', '-p', '-S', '-', '-t', target);
  const joined = tmux('capture-pane', '-p', '-J', '-S', '-', '-t', target);
  R.wrap = { splitWithoutJ: !plain.includes('SPLITTOKEN'), foundWithJ: joined.includes('SPLITTOKEN') };
  R.unicode = { utf8Intact: joined.includes('UNI-ü-日本-😀-éEND'), nfdFoldNeeded: !joined.includes('UNI-ü-日本-😀-eEND') };
  check(R.wrap.splitWithoutJ && R.wrap.foundWithJ, `capture-pane -J joins a wrapped token: ${JSON.stringify(R.wrap)}`);
  check(R.unicode.utf8Intact, 'UTF-8 (accents, CJK, emoji) comes through capture-pane intact');
  console.log(`ok  4 a token wrapped at ${m1.cols} columns is split without -J (${R.wrap.splitWithoutJ}) and whole with -J (${R.wrap.foundWithJ}); UTF-8 survives`);

  // 5. Shell history versus an application's alternate screen.
  // A stand-in for any full-screen program (less, vim, an agent TUI): it owns the alternate screen and keeps 400 lines
  // of its own text that only it can scroll to. `g` jumps to line 300; `q` quits.
  const app = join(home, 'alt-app.mjs');
  writeFileSync(app, `
    const lines = Array.from({ length: 400 }, (_, i) => 'ALT-' + String(i + 1).padStart(4, '0'));
    const draw = (top) => process.stdout.write('\\x1b[2J\\x1b[H' + lines.slice(top, top + (process.stdout.rows - 1)).join('\\r\\n'));
    process.stdout.write('\\x1b[?1049h'); draw(0);
    process.stdin.setRawMode(true);
    process.stdin.on('data', (d) => {
      if (String(d) === 'g') draw(299);
      if (String(d) === 'q') { process.stdout.write('\\x1b[?1049l'); process.exit(0); }
    });
  `);
  await hp.evaluate((c) => window.__e2e.type(c), `${process.execPath} ${app}\r`);
  await waitFor(() => meta().alternateOn, 'the app in the alternate screen');
  const mAlt = meta();
  const altCapture = tmux('capture-pane', '-p', '-S', '-', '-t', target);
  R.alternate = { alternateOn: mAlt.alternateOn, historySizeWhileInApp: mAlt.historySize, sawAlt0300InCapture: altCapture.includes('ALT-0300'), sawShellTokenInCapture: altCapture.includes('TOK-1500') };
  check(R.alternate.alternateOn && !R.alternate.sawAlt0300InCapture, 'an alternate-screen app keeps its own offscreen text out of tmux history');
  await hp.evaluate(() => window.__e2e.type('g'));
  await waitFor(async () => (await screen()).some((l) => l.includes('ALT-0300')), 'the app scrolls to ALT-0300', 10000);
  await hp.evaluate(() => window.__e2e.type('q'));
  await waitFor(() => !meta().alternateOn, 'leave the app');
  const afterApp = tmux('capture-pane', '-p', '-J', '-S', '-', '-t', target);
  R.alternate.shellHistoryAfterApp = afterApp.includes('TOK-1500');
  check(R.alternate.shellHistoryAfterApp, 'shell history is intact after the app exits');
  console.log(`ok  5 inside an alternate-screen app tmux history cannot see ALT-0300; only the app can scroll to it; shell history returns after q`);

  // 6. Reattach: the browser buffer is rebuilt from the repaint, the history stays in tmux.
  await open('s1');
  const re = tokens(await screen());
  R.reattach = { domHasToken1500: re.includes(1500), tmuxHasToken1500: tmux('capture-pane', '-p', '-S', '-', '-t', target).includes('TOK-1500'), historySize: meta().historySize };
  check(!R.reattach.domHasToken1500 && R.reattach.tmuxHasToken1500, `after a reload the browser has no history, tmux still does: ${JSON.stringify(R.reattach)}`);
  console.log('ok  6 after reload + reattach the browser buffer holds the current screen only; tmux still holds TOK-1500');

  // 7. Output arriving during a search, and what binds a result to its session/window/pane.
  const before = { meta: meta(), snap: tmux('capture-pane', '-p', '-J', '-S', '-', '-t', target) };
  await run(`printf 'LATE-%02d\\n' $(seq 1 25)`, 'late');
  const after = { meta: meta(), snap: tmux('capture-pane', '-p', '-J', '-S', '-', '-t', target) };
  const first = (s) => tokens(s.split('\n'))[0];
  R.liveOutput = {
    historySizeBefore: before.meta.historySize, historySizeAfter: after.meta.historySize,
    firstTokenBefore: first(before.snap), firstTokenAfter: first(after.snap),
    snapshotMissesNewLines: !before.snap.includes('LATE-25') && after.snap.includes('LATE-25'),
  };
  check(R.liveOutput.snapshotMissesNewLines, 'a snapshot is stale as soon as output arrives');
  check(R.liveOutput.firstTokenAfter > R.liveOutput.firstTokenBefore, `at history-limit the oldest line moves: ${JSON.stringify(R.liveOutput)}`);
  console.log(`ok  7 at the limit the oldest line moved TOK-${R.liveOutput.firstTokenBefore} -> TOK-${R.liveOutput.firstTokenAfter} while history_size went ${R.liveOutput.historySizeBefore} -> ${R.liveOutput.historySizeAfter}: tmux trims in blocks, so size cannot signal staleness, new output must`);

  // 8. Session and window binding.
  const id2 = await newSession('s2');
  tmux('send-keys', '-t', `=${id2}:`, "echo SWITCH-B-$((20+1))", 'Enter');
  await waitFor(() => tmux('capture-pane', '-p', '-t', `=${id2}:`).includes('SWITCH-B-21'), 'second session printed');
  const s2 = tmux('capture-pane', '-p', '-J', '-S', '-', '-t', `=${id2}:`);
  tmux('new-window', '-t', `=${id}:`);
  tmux('send-keys', '-t', `=${id}:1`, "echo WIN1-$((20+1))", 'Enter');
  await waitFor(() => tmux('capture-pane', '-p', '-t', `=${id}:1`).includes('WIN1-21'), 'window 1 printed');
  const w0 = tmux('capture-pane', '-p', '-J', '-S', '-', '-t', `=${id}:0`);
  const w1 = tmux('capture-pane', '-p', '-J', '-S', '-', '-t', `=${id}:1`);
  const ids = { s1w0: meta(`=${id}:0`), s1w1: meta(`=${id}:1`), s2: meta(`=${id2}:`) };
  R.binding = {
    otherSessionHasNoToken: !s2.includes('TOK-'), windowsAreSeparate: w0.includes('LATE-25') && !w0.includes('WIN1-21') && w1.includes('WIN1-21') && !w1.includes('LATE-'),
    paneIds: [ids.s1w0.pane, ids.s1w1.pane, ids.s2.pane], windowIds: [ids.s1w0.window, ids.s1w1.window, ids.s2.window],
  };
  check(R.binding.otherSessionHasNoToken && R.binding.windowsAreSeparate && new Set(R.binding.paneIds).size === 3, `results bind to session and window: ${JSON.stringify(R.binding)}`);
  // A pane id outlives a rename; a name does not.
  tmux('rename-session', '-t', `=${id2}`, 'renamed');
  let renamedFails = false;
  try { tmux('capture-pane', '-p', '-t', `=${id2}:`); } catch { renamedFails = true; }
  R.binding.renameBreaksNameTarget = renamedFails;
  R.binding.paneIdStillResolves = tmux('capture-pane', '-p', '-t', ids.s2.pane).includes('SWITCH-B-21');
  check(renamedFails && R.binding.paneIdStillResolves, 'a name target fails after a rename; a pane id still resolves');
  console.log('ok  8 a snapshot is bound by (server, session, window id, pane id); other sessions/windows never leak in; a rename breaks name targets, pane ids survive');

  // 9. Cost at a larger history: the resource budget (512 MB, FACT-0001).
  const tmuxPid = Number(tmux('display-message', '-p', '#{pid}').trim());
  const rssBefore = rssMb(tmuxPid);
  tmux('set-option', '-g', 'history-limit', '20000');
  tmux('new-session', '-d', '-s', 'big', '-x', '120', '-y', '30');
  tmux('send-keys', '-t', '=big:', `printf 'BIG-%05d some filler text to look like agent output\\n' $(seq 1 25000); echo bigdone-$((1+1))`, 'Enter');
  await waitFor(() => tmux('capture-pane', '-p', '-t', '=big:').includes('bigdone-2'), 'big session filled', 60000);
  const [bigSnap, bigMs] = timed(() => tmux('capture-pane', '-p', '-J', '-S', '-', '-t', '=big:'));
  const bigProbe = await page.evaluate((text) => {
    const t = performance.now();
    const lines = text.split('\n');
    let hits = 0;
    for (const l of lines) if (l.toLowerCase().includes('big-24000')) hits++;
    return { ms: +(performance.now() - t).toFixed(2), hits, lines: lines.length };
  }, bigSnap);
  R.big = {
    historyLimit: 20000, historyRowsHeld: meta('=big:').historySize, paneCols: meta('=big:').cols, firstLine: bigSnap.split('\n')[0].slice(0, 9), lines: bigSnap.split('\n').length, bytes: Buffer.byteLength(bigSnap), gzipBytes: gzipSync(bigSnap).length, captureMs: +bigMs.toFixed(1),
    tmuxRssMbBefore: +rssBefore.toFixed(1), tmuxRssMbAfter: +rssMb(tmuxPid).toFixed(1), serverRssMb: +rssMb(server.pid).toFixed(1), browserSearchMs: bigProbe.ms, browserHits: bigProbe.hits,
  };
  check(bigProbe.hits === 1 && R.big.historyRowsHeld > 19000, `a 20000-line history is searchable in the page: ${JSON.stringify(R.big)}`);
  console.log(`ok  9 20000-row history (${R.big.lines} logical lines at ${R.big.paneCols} columns, oldest ${R.big.firstLine}): ${(R.big.bytes / 1e6).toFixed(2)} MB (${(R.big.gzipBytes / 1e6).toFixed(2)} MB gzip), capture ${R.big.captureMs} ms, page search ${R.big.browserSearchMs} ms, tmux RSS ${R.big.tmuxRssMbBefore} -> ${R.big.tmuxRssMbAfter} MB`);

  check(errors.length === 0, `page errors: ${errors.slice(0, 3).join(' | ')}`);
  console.log(`RESULTS ${JSON.stringify(R)}`);
  if (process.env.SEARCH_RESULTS) writeFileSync(process.env.SEARCH_RESULTS, JSON.stringify(R, null, 2));
  await ctx.close();
} catch (e) {
  fail(e.stack ?? String(e));
} finally {
  await browser?.close().catch(() => {});
  try { tmux('kill-server'); } catch { /* already gone */ }
  server.kill();
  appServer.close();
  harness.close();
  rmSync(home, { recursive: true, force: true });
}
if (process.exitCode) process.exit(process.exitCode);
console.log('PASS');
process.exit(0);
