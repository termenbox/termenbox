// Cross-session search against the REAL server binary and real tmux (roadmap R78, decision 0156). A private HOME holds
// fixture Claude Code and Codex session files; one workspace is open (a real tmux window), others are dormant records.
// The Search sheet finds a phrase in both agents' files, scrubbed; a forced cap (50 hits, 3 per session) shows the cut line;
// a tap on an open hit switches to its tab; a tap on a dormant hit asks first and then resumes; and the query appears in
// no server log line. An ACP workspace (the fixture agent) lands on the turn and marks the match.
// What this proves is the page in this engine on a desktop window. It does not prove a phone or a real Safari.
//
// Usage: node e2e/session-search.mjs   (after: pnpm run build && pnpm run e2e:build; needs tmux and a built server)
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

// --- fixtures: the agents' own files under a private HOME ---------------------------------------------------------
const PHRASE = 'zebrafish migration';
const SECRET = 'sk-FAKE-0123456789abcdef0123456789abcdef';
const root = mkdtempSync(join(tmpdir(), 'termenbox-sessearch-'));
const home = realpathSync(root);
const userHome = join(home, 'user');
const dirOf = (name) => { const d = join(userHome, 'code', name); mkdirSync(d, { recursive: true }); return d; };
const claudeProjects = join(userHome, '.claude', 'projects');
const uuid = (n) => `c1a0de00-0000-4000-8000-${String(n).padStart(12, '0')}`;
const iso = (s) => `2026-10-31T10:${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}.000Z`;
/** A Claude Code file with `n` user turns that all hold the phrase, the shape measured in step 1. */
function writeClaude(dir, id, n, secret = false) {
  const lines = [];
  let parent = null;
  for (let i = 0; i < n; i++) {
    const u = `u${i}`, a = `a${i}`;
    const text = `${PHRASE} number ${i}${secret && i === 0 ? ` with key ${SECRET}` : ''}`;
    lines.push({ parentUuid: parent, type: 'user', uuid: u, timestamp: iso(i * 2 + 1), message: { role: 'user', content: text }, isSidechain: false, userType: 'external', cwd: dir, sessionId: id, version: '2.1.308' });
    lines.push({ parentUuid: u, type: 'assistant', uuid: a, timestamp: iso(i * 2 + 2), message: { role: 'assistant', content: [{ type: 'text', text: `Noted, number ${i}.` }] }, isSidechain: false, cwd: dir, sessionId: id, version: '2.1.308' });
    parent = a;
  }
  const enc = dir.replace(/[^A-Za-z0-9]/g, '-');
  mkdirSync(join(claudeProjects, enc), { recursive: true });
  writeFileSync(join(claudeProjects, enc, `${id}.jsonl`), lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
}
/** A Codex rollout file, the shape measured in step 1. */
function writeCodex(dir, id) {
  const turn = `${id.slice(0, -2)}a1`;
  const rec = (ordinal, type, payload) => ({ timestamp: iso(ordinal), ordinal, type, payload });
  const lines = [
    rec(0, 'session_meta', { session_id: id, id, timestamp: iso(0), cwd: dir, originator: 'codex_exec', cli_version: '0.166.0', source: 'exec', model_provider: 'openai' }),
    rec(1, 'event_msg', { type: 'task_started', turn_id: turn, root_turn_id: turn }),
    rec(2, 'response_item', { type: 'message', id: 'm1', role: 'user', content: [{ type: 'input_text', text: `Plan the ${PHRASE} for the codex side` }], internal_chat_message_metadata_passthrough: { turn_id: turn } }),
    rec(3, 'response_item', { type: 'message', id: 'm2', role: 'assistant', content: [{ type: 'output_text', text: 'Planned.' }], internal_chat_message_metadata_passthrough: { turn_id: turn } }),
  ];
  const d = join(userHome, '.codex', 'sessions', '2026', '10', '31');
  mkdirSync(d, { recursive: true });
  writeFileSync(join(d, `rollout-2026-10-31T10-00-00-${id}.jsonl`), lines.map((l) => JSON.stringify(l)).join('\n') + '\n');
}

const openDir = dirOf('open-app');
const downClaude = dirOf('down-claude');
const downCodex = dirOf('down-codex');
const acpDir = dirOf('acp-app');
const ACP_SESSION = 'fake-workspace-1'; // the id the fixture ACP agent reports
const OPEN_ID = uuid(1), DOWN_CLAUDE_ID = uuid(2), DOWN_CODEX_ID = 'c0de0000-0000-7000-8000-000000000003';
writeClaude(openDir, OPEN_ID, 1, true);
writeClaude(downClaude, DOWN_CLAUDE_ID, 1);
writeCodex(downCodex, DOWN_CODEX_ID);
writeClaude(acpDir, ACP_SESSION, 1);
const acpFile = join(claudeProjects, acpDir.replace(/[^A-Za-z0-9]/g, '-'), `${ACP_SESSION}.jsonl`);
writeFileSync(acpFile, readFileSync(acpFile, 'utf8').replace(`${PHRASE} number 0`, 'a wombat itinerary for the agent view'));
mkdirSync(join(home, 'config'), { recursive: true });
writeFileSync(join(home, 'config', 'acp-agents.toml'), `max_sessions = 3\n[agents.claude]\ncommand = "node"\nargs = ["${join(here, 'fake-acp-agent.mjs')}"]\n`);
// Enough dormant sessions of three hits each to pass the server's cap of 50 hits (3 per session): 18 x 3 = 54.
const FILLERS = Array.from({ length: 18 }, (_, i) => ({ dir: dirOf(`filler-${i}`), id: uuid(100 + i) }));
for (const f of FILLERS) writeClaude(f.dir, f.id, 3);

const httpPort = await freeTcpPort();
const claim = randomBytes(32).toString('base64url');
const logFile = join(home, 'server.log');
const env = {
  ...process.env,
  HOME: userHome,
  CLAUDE_CONFIG_DIR: '',
  CODEX_HOME: '',
  TMUX_TMPDIR: home,
  TERMENBOX_CONFIG_DIR: join(home, 'config'),
  TERMENBOX_HOST: '127.0.0.1',
  TERMENBOX_PORT: String(httpPort),
  TERMENBOX_UDP_PORT: String(await freeUdpPort()),
  TERMENBOX_PUBLIC_IP: '127.0.0.1',
  TERMENBOX_DIRECTORY_URL: '',
  TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`,
  TERMENBOX_CLAIM_SECRET: claim,
  TERMENBOX_BACKEND: 'tmux,acp',
  TERMENBOX_LOG: 'trace', // the loudest level: the query must be absent even there
  LANG: 'C.UTF-8',
};
const tmux = (...args) => execFileSync('tmux', ['-L', 'termenbox', ...args], { env: { ...process.env, TMUX_TMPDIR: home }, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
const appPort = await new Promise((r) => appServer.listen(0, '127.0.0.1', () => r(appServer.address().port)));
let server;
const startServer = async () => {
  const out = (await import('node:fs')).openSync(logFile, 'a');
  server = spawn(BIN, [], { env, stdio: ['ignore', out, out] });
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
};
const stopServer = async () => { server.kill(); await new Promise((r) => server.once('exit', r)); };
const registryPath = join(home, 'config', 'workspaces.json');

let browser;
let page;
let failed = null;
try {
  await startServer();
  browser = await launch();
  const ctx = await browser.newContext(contextOptions({ viewport: viewport('1280x800') }));
  page = await ctx.newPage();
  page.setDefaultTimeout(20000);
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => m.type() === 'error' && errors.push(m.text()));

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
    localStorage.setItem('termenbox_settings', JSON.stringify({ renderer: 'dom' }));
    localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login: 'alice' }));
    localStorage.setItem('termenbox_servers', JSON.stringify(items));
    localStorage.setItem('termenbox_ios_hint', '1');
  }, [mint(dpk), saved]);
  await page.reload();
  const see = (locator, what) => locator.first().waitFor({ state: 'visible', timeout: 10000 }).catch(() => { throw new Error(`expected ${what} to be visible`); });

  // The open workspace: created in the UI (a real tmux window), then its tab is recorded as a Claude tab with its session.
  await page.locator('[data-shell=sidebar]').waitFor();
  await page.getByRole('button', { name: /^(\+ New|New workspace( on .*)?)$/ }).first().click();
  const dialog = page.locator('dialog[aria-label="New workspace"]');
  await see(dialog, 'the New workspace dialog');
  await dialog.locator('[data-testid="folder-change"]').click();
  await see(dialog.locator('[data-testid="picker-list"] [data-dir="code"]'), 'code in the picker');
  await dialog.locator('[data-testid="picker-list"] [data-dir="code"]').click();
  await dialog.locator('[data-testid="picker-list"] [data-dir="open-app"]').click();
  await dialog.locator('[data-testid="picker-select"]').click();
  await dialog.getByRole('button', { name: 'Create' }).click();
  await dialog.waitFor({ state: 'detached', timeout: 10000 });
  await waitFor(() => existsSync(registryPath) && JSON.parse(readFileSync(registryPath, 'utf8')).workspaces?.length === 1, 'the open workspace in the registry');
  console.log('ok  an open workspace exists with a real tmux window');

  // Record the sessions the way the agents' hooks would (decision 0152): the open tab, and dormant workspaces without windows.
  await stopServer();
  const reg = JSON.parse(readFileSync(registryPath, 'utf8'));
  const open = reg.workspaces[0];
  open.dir = openDir;
  open.tabs[0].kind = 'claude';
  open.tabs[0].agent_session = OPEN_ID;
  const dormant = (id, label, dir, kind, session) => ({ id: `tmux:${id}`, label, dir, backend: 'tmux', order: 0, next_tab: 2, tabs: [{ id: 't1', label: kind, kind, dir: '', order: 0, agent_session: session }] });
  const extra = [
    dormant('ws-down-claude', 'down-claude', downClaude, 'claude', DOWN_CLAUDE_ID),
    dormant('ws-down-codex', 'down-codex', downCodex, 'codex', DOWN_CODEX_ID),
    ...FILLERS.map((f, i) => dormant(`ws-filler-${i}`, `filler-${i}`, f.dir, 'claude', f.id)),
  ];
  extra.forEach((w, i) => { w.order = i + 1; });
  reg.workspaces.push(...extra);
  writeFileSync(registryPath, JSON.stringify(reg));
  await startServer();
  await page.reload();
  await page.locator('[data-shell=sidebar]').waitFor();

  const sheet = page.locator('[data-testid="session-search"]');
  const input = sheet.locator('[data-testid="search-input"]');
  const status = sheet.locator('[data-testid="search-status"]');
  const hits = sheet.locator('[data-testid="search-hit"]');
  const openSheet = async () => { await page.locator('[data-shell=sidebar]').getByRole('button', { name: /^Search/ }).click(); await page.keyboard.type('Search conversations'); await page.keyboard.press('Enter'); await see(input, 'the search input'); };
  const search = async (q) => { await input.fill(q); await page.waitForTimeout(400); };

  // 1. The notice is shown, and a rare phrase finds the open Claude session, the dormant Claude and Codex sessions.
  await openSheet();
  await see(sheet.locator('[data-testid="search-notice"]'), 'the server notice');
  const notice = await sheet.locator('[data-testid="search-notice"]').innerText();
  if (!/sent to your server/.test(notice)) throw new Error(`the notice reads "${notice}"`);
  await search('codex side');
  await see(hits, 'a Codex hit');
  const codexRows = await hits.evaluateAll((els) => els.map((e) => e.textContent));
  if (codexRows.length !== 1 || !/Resume/.test(codexRows[0]) || !/codex side/.test(codexRows[0])) throw new Error(`Codex hit rows: ${JSON.stringify(codexRows)}`);
  console.log('ok  a phrase in a Codex rollout file of a dormant workspace is found, badged Resume');

  // 2. Scrubbing: the secret in the open session's file is never in a snippet.
  await search('with key');
  await see(hits, 'the hit on the secret line');
  const secretRows = (await hits.evaluateAll((els) => els.map((e) => e.textContent))).join('\n');
  if (secretRows.includes(SECRET) || secretRows.includes('sk-FAKE')) throw new Error(`a secret reached the sheet: ${secretRows}`);
  console.log('ok  snippets are scrubbed');

  // 3. The cap: the phrase is in 21 sessions, more than 50 hits at 3 per session, so the search is cut and says so.
  await search(PHRASE);
  await see(sheet.locator('[data-testid="search-cut"]'), 'the search was cut line');
  const count = await hits.count();
  if (count > 50 || count < 40) throw new Error(`the capped search shows ${count} hits, expected 40-50`);
  console.log(`ok  the capped search shows ${count} hits and the cut line: "${await sheet.locator('[data-testid="search-cut"]').first().innerText()}"`);

  // 4. A tap on an open hit switches to its tab and closes the sheet.
  await search('with key');
  await see(hits, 'the open hit');
  const openBadge = await hits.first().locator('[data-testid="search-badge"]').innerText();
  if (openBadge !== 'Open') throw new Error(`the open session's badge reads "${openBadge}"`);
  await hits.first().click();
  await input.waitFor({ state: 'detached', timeout: 10000 }).catch(() => { throw new Error('the sheet stayed open after a tap on an open hit'); });
  await page.locator('.xterm').first().waitFor({ state: 'visible', timeout: 10000 }).catch(() => { throw new Error('no terminal after a tap on an open hit'); });
  console.log('ok  a tap on an open hit lands on the terminal of its tab');

  // 4b. An ACP workspace (the fixture agent): a tap on its hit lands on the turn and marks the match.
  await page.getByRole('button', { name: /^(\+ New|New workspace( on .*)?)$/ }).first().click();
  await see(dialog, 'the New workspace dialog');
  await dialog.locator('[data-testid=view-acp]').click();
  await dialog.locator('[data-testid=folder-change]').click();
  await dialog.locator('[data-testid=picker-path]').fill(acpDir);
  await dialog.locator('[data-testid=picker-go]').click();
  await dialog.locator('[data-testid=picker-select]:not([disabled])').click();
  await dialog.locator('[data-testid=picker-path]').waitFor({ state: 'detached' });
  await dialog.getByLabel('Name').fill('agent-view');
  await dialog.getByRole('button', { name: 'Create' }).click();
  await dialog.waitFor({ state: 'detached' });
  await page.locator('[data-testid=acp-view]').first().waitFor({ timeout: 30000 });
  await openSheet();
  // The server learns the agent's session id when the agent answers session/new, which can be just after the view shows.
  await waitFor(async () => { await search('wombat itinerary'); return (await hits.count()) > 0; }, 'the ACP hit', 30000)
    .catch(async (e) => { throw new Error(`${e.message}; the sheet reads: ${(await sheet.innerText()).replace(/\s+/g, ' ').slice(0, 400)}; the view reads: ${(await page.locator('[data-testid=acp-view]').first().innerText().catch(() => 'no view')).replace(/\s+/g, ' ').slice(0, 500)}; server: ${readFileSync(logFile, 'utf8').split('\n').filter((l) => /acp|agent/i.test(l) && !/TRACE|DEBUG/.test(l)).slice(-6).join(' // ').replace(/\x1b\[[0-9;]*m/g, '').slice(0, 900)}`); });
  await hits.first().click();
  await input.waitFor({ state: 'detached', timeout: 10000 });
  const mark = page.locator('[data-testid=acp-view] [data-hit]').first();
  await see(mark, 'the match marked in the agent view');
  if (!/wombat itinerary/i.test(await mark.innerText())) throw new Error('the marked text is not the match');
  console.log('ok  a tap on an Agent view hit lands on the turn and marks the match');

  // 5. A tap on a dormant hit asks first, Not now leaves it dormant, Resume brings the workspace back.
  await openSheet();
  await search('codex side');
  await see(hits, 'the dormant hit');
  await hits.first().click();
  await see(sheet.locator('[data-testid="search-resume"]'), 'the Resume question');
  await sheet.locator('[data-testid="search-resume-cancel"]').click();
  await sheet.locator('[data-testid="search-resume"]').waitFor({ state: 'detached' });
  const windowsBefore = tmux('list-windows', '-a', '-F', '#{window_id}').split('\n').filter(Boolean).length;
  await hits.first().click();
  await sheet.locator('[data-testid="search-resume-go"]').click();
  await waitFor(() => tmux('list-windows', '-a', '-F', '#{window_id}').split('\n').filter(Boolean).length > windowsBefore, 'a tmux window for the resumed workspace');
  const resumeLine = await waitFor(() => tmux('list-panes', '-a', '-F', '#{pane_current_path}').includes(downCodex) || null, 'the resumed pane in its directory');
  if (!resumeLine) throw new Error('the resumed workspace did not start in its directory');
  console.log('ok  a tap on a dormant hit asks first and Resume reopens the workspace in its directory');

  // 6. The privacy check: neither the phrases nor the secret appear in any server log line, at trace level.
  const log = readFileSync(logFile, 'utf8');
  for (const needle of [PHRASE, 'codex side', 'with key', SECRET, 'zebrafish']) {
    if (log.toLowerCase().includes(needle.toLowerCase())) throw new Error(`the server log holds "${needle}"`);
  }
  console.log(`ok  the server log (${log.split('\n').length} lines at trace level) holds no query and no secret`);

  if (errors.length) throw new Error(`console errors: ${errors.slice(0, 3).join(' | ')}`);
} catch (e) {
  failed = e;
  console.error(`FAIL: ${e instanceof Error ? e.stack : e}`);
} finally {
  await browser?.close().catch(() => {});
  server?.kill();
  try { tmux('kill-server'); } catch {}
  appServer.close();
  rmSync(root, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
}
console.log(failed ? 'FAIL' : 'PASS');
process.exit(failed ? 1 : 0);
