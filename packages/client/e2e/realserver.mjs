// Browser matrix, real server (roadmap R36): the real client, built, against a real isolated termenbox-server and real
// tmux, in the engine named by E2E_ENGINE (chromium | firefox | webkit). Viewport and pixel ratio come from E2E_VIEWPORT
// (default 1280x800) and E2E_DPR; a width of 720 px or less is a phone, anything wider a desktop window (mouse and
// keyboard, no touch). The breakpoint scenario runs in desktop only.
// The client connects over WebSocket (the iOS Safari baseline, DECISION-0112); the WebRTC path is covered by run.mjs on Chromium.
// What this proves is what the page does in that engine. It does not prove a real Safari, Edge or iPhone: see docs/skills/browser-support-check.md.
//
// Usage: node e2e/realserver.mjs   (after: pnpm run build && pnpm run e2e:build; needs tmux and a built server)
//   TERMENBOX_BIN   server binary (default ../../target/release/termenbox-server)
//   E2E_ENGINE, E2E_VIEWPORT, E2E_DPR, CHROME_PATH, E2E_ARTIFACTS: see browser.mjs
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, readFileSync, existsSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, extname, resolve } from 'node:path';
import { randomBytes, generateKeyPairSync, sign, createPrivateKey, createPublicKey } from 'node:crypto';
import { launch, viewport, contextOptions, trace, keepIfFailed, ENGINE, DPR } from './browser.mjs';

const here = import.meta.dirname;
const BIN = process.env.TERMENBOX_BIN ?? resolve(here, '../../../target/release/termenbox-server');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const randomPort = () => 30000 + Math.floor(Math.random() * 30000);
async function waitFor(fn, what, ms = 20000) {
  const start = Date.now();
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() - start > ms) throw new Error(`timed out waiting for ${what}`);
    await sleep(100);
  }
}

// --- stand-ins: the function that issues certificates (as in run.mjs), the built client, an isolated server ---
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

const home = mkdtempSync(join(tmpdir(), 'termenbox-matrix-'));
const httpPort = randomPort();
const claim = randomBytes(32).toString('base64url');
const serverEnv = {
  ...process.env,
  TMUX_TMPDIR: home, // a private tmux server: never touches the developer's sessions
  TERMENBOX_CONFIG_DIR: join(home, 'config'),
  TERMENBOX_HOST: '127.0.0.1',
  TERMENBOX_PORT: String(httpPort),
  TERMENBOX_UDP_PORT: String(randomPort()),
  TERMENBOX_PUBLIC_IP: '127.0.0.1',
  TERMENBOX_DIRECTORY_URL: '',
  TERMENBOX_CERT_KEYS: `e2e=${signerPublic}`,
  TERMENBOX_CLAIM_SECRET: claim,
  LANG: 'C.UTF-8',
};
let server;
const startServer = async () => {
  server = spawn(BIN, [], { env: serverEnv, stdio: ['ignore', 'ignore', 'inherit'] });
  await waitFor(() => fetch(`http://127.0.0.1:${httpPort}/health`).then((r) => r.ok, () => false), 'server health');
};
/** The server process goes away and comes back with the same identity and the same tmux sessions: what a reboot of the service looks like. */
const restartServer = async () => {
  server.kill();
  await new Promise((r) => server.once('exit', r));
  await startServer();
};
const tmux = (...args) => execFileSync('tmux', ['-L', 'termenbox', ...args], { env: { ...process.env, TMUX_TMPDIR: home }, encoding: 'utf8' }).trim();
const appPort = await new Promise((r) => appServer.listen(0, '127.0.0.1', () => r(appServer.address().port)));

// --- one context's run -----------------------------------------------------------------------------------------
/** Sign the page in as the owner of the test server: a device key in IndexedDB, a certificate for it, the server saved. */
async function signIn(page) {
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
    localStorage.setItem('termenbox_settings', JSON.stringify({ renderer: 'dom' })); // rows are text, so a test can read the screen
    localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login: 'alice' }));
    localStorage.setItem('termenbox_servers', JSON.stringify(items));
    localStorage.setItem('termenbox_ios_hint', '1');
  }, [mint(dpk), saved]);
  await page.reload();
}


/**
 * Whether the page can be scrolled sideways, which is what a user feels. scrollWidth alone counts elements a user cannot
 * reach (xterm parks its measuring nodes far off screen), so the page is asked to scroll and the first element that
 * sticks out is named for the report.
 */
const sideways = (page) => page.evaluate(() => {
  // xterm's DOM renderer parks a 50000px wide measuring box at top:-50000px; Firefox lets it widen the page, a user cannot
  // see or reach it. It is hidden for the measurement so the page's own overflow is what is reported.
  const parked = [...document.querySelectorAll('[style*="-50000px"]')];
  parked.forEach((e) => { e.dataset.was = e.style.display; e.style.display = 'none'; });
  window.scrollTo(100000, 0);
  const scrollable = window.scrollX > 0;
  window.scrollTo(0, 0);
  const out = [...document.body.querySelectorAll('*')].find((e) => e.getBoundingClientRect().right > innerWidth + 1 && e.getBoundingClientRect().width > 0 && !e.closest('.xterm-helpers'));
  const result = { scrollable, px: document.documentElement.scrollWidth - innerWidth, culprit: out ? `${out.tagName.toLowerCase()}.${String(out.className).slice(0, 60)}` : null };
  parked.forEach((e) => { e.style.display = e.dataset.was; delete e.dataset.was; });
  return result;
});

/** Run every scenario in one context and return what failed. */
async function runContext(browser, label) {
  const engine = ENGINE;
  const vp = viewport('1280x800');
  const phone = vp.width <= 720;
  const failures = [];
  const check = (cond, msg) => { if (!cond) { failures.push(msg); console.error(`FAIL [${engine} ${label}]: ${msg}`); } return !!cond; };
  const ok = (msg) => console.log(`ok  [${engine} ${label}] ${msg}`);
  const ctx = await browser.newContext(contextOptions({ viewport: vp, isMobile: phone, hasTouch: phone && ENGINE === 'chromium' }));
  await trace(ctx, label);
  const tag = label.replace(/\W/g, '');
  try {
    const page = await ctx.newPage();
    page.setDefaultTimeout(20000);
    page.on('pageerror', (e) => check(false, `page error: ${e.message}`));
    await signIn(page);
    const sidebar = page.locator('[data-shell=sidebar]');
    const bottom = page.locator('[data-shell=bottom-bar]');
    await (phone ? bottom : sidebar).waitFor();
    

    // 1. Layout of the context: sidebar on a desktop window, bottom bar on a phone; the destinations reach their pages.
    check((await sidebar.count()) === (phone ? 0 : 1), `sidebar presence is wrong for ${label}`);
    check((await bottom.count()) === (phone ? 1 : 0), `bottom bar presence is wrong for ${label}`);
    const nav = phone ? bottom : sidebar;
    for (const [id, heading] of [['activity', 'Activity'], ['settings', 'Settings'], ['home', 'Home']]) {
      await nav.locator(`[data-destination=${id}]`).click();
      await page.getByRole('heading', { name: heading, exact: true }).waitFor();
      { const o = await sideways(page); check(!o.scrollable, `${id} scrolls sideways (${o.px}px, ${o.culprit})`); }
    }
    ok('shell chrome and the destinations');

    // 2. Open a session; attach shows a real xterm, typing reaches the shell, a second session switches.
    const open = async (session) => {
      if (phone) {
        await bottom.locator('[data-destination=sessions]').click();
        await page.locator(`[role=tab][aria-label*="${session}"]`).first().click();
      } else {
        await sidebar.locator(`[data-key$="/${session}"]`).click();
      }
      await page.locator('.xterm-rows').waitFor();
    };
    const screen = () => page.locator('.xterm-rows').innerText();
    const input = page.locator('textarea[aria-label="Terminal input"]');
    await open('alpha');
    await waitFor(async () => (await screen()).includes('READY-alpha'), 'the attached screen of alpha');
    const term = await page.locator('.xterm').elementHandle();
    await input.focus();
    await page.keyboard.type(`echo typed-${tag}-$((20+22))`);
    await page.keyboard.press('Enter');
    await waitFor(async () => (await screen()).includes(`typed-${tag}-42`), 'typed input to run in the shell');
    ok('attach, keyboard input reaches the shell');

    await open('beta');
    await waitFor(async () => (await screen()).includes('READY-beta'), 'the screen of beta');
    check(!(await screen()).includes(`typed-${tag}-42`), 'switching session kept the other session\'s screen');
    await open('alpha');
    await waitFor(async () => (await screen()).includes(`typed-${tag}-42`), 'alpha screen back after switching');
    ok('switching sessions shows each session\'s own screen');

    // 3. Reconnect: the server restarts; the client re-attaches and the screen and typing still work.
    await restartServer();
    // Keys typed while the client is still reconnecting are dropped, as they are for a user; retry until the shell answers.
    const answered = await waitFor(async () => {
      await input.focus();
      await page.keyboard.type(`echo after-${tag}-$((1+1))`);
      await page.keyboard.press('Enter');
      await sleep(1500);
      return (await screen()).includes(`after-${tag}-2`);
    }, 'the shell to answer after the server restarted', 40000).catch(() => false);
    check(answered, 'input did not reach the shell after the server restarted');
    check((await screen()).includes(`typed-${tag}-42`), 'the screen from before the restart was not replayed');
    ok('reconnect re-attaches with the earlier screen and input works');

    if (!phone) {
      // 4. Keyboard-only: the palette opens from the terminal and Escape gives focus back; an ordinary key reaches the shell.
      await input.focus();
      await page.keyboard.press('Control+k');
      const palette = page.getByRole('combobox', { name: 'Search' });
      await palette.waitFor();
      await page.keyboard.type('beta');
      await page.keyboard.press('Enter');
      await waitFor(async () => (await screen()).includes('READY-beta'), 'the palette to open beta');
      await input.focus(); // the terminal was rebuilt for the other session
      await page.keyboard.press('Control+k');
      await palette.waitFor();
      await page.keyboard.press('Escape');
      await palette.waitFor({ state: 'detached' });
      check(await input.evaluate((e) => document.activeElement === e), 'focus did not return to the terminal after Escape');
      await open('alpha');
      ok('keyboard only: Ctrl+K palette opens a session, Escape returns focus to the terminal');

      // 5. Dialog: the shortcut list is a dialog that Escape closes.
      await page.keyboard.press('Control+k');
      await page.keyboard.type('keyboard shortcuts');
      await page.keyboard.press('Enter');
      const help = page.getByRole('dialog', { name: 'Keyboard shortcuts' });
      await help.waitFor();
      check((await help.innerText()).includes('Ctrl+K'), 'the shortcut list does not name Ctrl+K');
      await page.keyboard.press('Escape');
      await help.waitFor({ state: 'detached' });
      ok('dialog: opens from the palette and Escape closes it');

      // 6. Pointer selection and clipboard. A double-click or a drag selects text; Ctrl+Shift+C copies it.
      await input.focus();
      await page.keyboard.type(`echo SELECT-${tag}-ME`);
      await page.keyboard.press('Enter');
      await waitFor(async () => (await page.locator('.xterm-rows').innerText()).split(`SELECT-${tag}-ME`).length > 2, 'the echoed line');
      const row = page.locator('.xterm-rows > div', { hasText: new RegExp(`^SELECT-${tag}-ME`) }).first();
      const box = await row.boundingBox();
      // xterm keeps its own selection (not the page's), so what a selection holds is read by copying it. Start inside the text:
      // the sidebar's 44 px resize target overlaps the first ~22 px of the terminal.
      const word = async () => { await page.mouse.dblclick(box.x + 60, box.y + box.height / 2); await input.focus(); };
      const drag = async () => {
        await page.mouse.move(box.x + 60, box.y + box.height / 2);
        await page.mouse.down();
        await page.mouse.move(box.x + box.width * 0.9, box.y + box.height / 2, { steps: 6 });
        await page.mouse.up();
        await input.focus();
      };

      // Clipboard allowed: a recording Clipboard API stands in for a granted permission in every engine (only Chromium can be
      // granted for real, below). Clipboard denied: the API rejects, and the app must fall back to execCommand("copy").
      const stubClipboard = (mode) => page.evaluate((m) => {
        window.__copied = [];
        window.__execCopies = [];
        const exec = Document.prototype.execCommand;
        document.execCommand = function (cmd, ...rest) {
          if (cmd === 'copy') window.__execCopies.push(document.activeElement?.value ?? window.getSelection()?.toString() ?? '');
          return exec.call(this, cmd, ...rest);
        };
        const api = m === 'allowed'
          ? { writeText: async (t) => { window.__copied.push(t); }, readText: async () => window.__copied.at(-1) ?? '' }
          : { writeText: () => Promise.reject(new DOMException('denied', 'NotAllowedError')), readText: () => Promise.reject(new DOMException('denied', 'NotAllowedError')) };
        Object.defineProperty(navigator, 'clipboard', { value: api, configurable: true });
      }, mode);
      await stubClipboard('allowed');
      await word();
      await page.keyboard.press('Control+Shift+C');
      await waitFor(() => page.evaluate(() => window.__copied.length > 0), 'a clipboard write');
      check((await page.evaluate(() => window.__copied[0])).includes(`SELECT-${tag}-ME`), 'a double-click selection was not copied by Ctrl+Shift+C when the clipboard is allowed');
      await page.evaluate(() => { window.__copied.length = 0; });
      await drag();
      await page.keyboard.press('Control+Shift+C');
      await waitFor(() => page.evaluate(() => window.__copied.length > 0), 'a clipboard write after a drag');
      check((await page.evaluate(() => window.__copied[0])).includes('-ME'), 'a pointer drag selection was not copied');
      await stubClipboard('denied');
      await word();
      await page.keyboard.press('Control+Shift+C');
      await waitFor(() => page.evaluate(() => window.__execCopies.length > 0), 'the execCommand fallback when the clipboard is denied');
      check((await page.evaluate(() => window.__execCopies[0])).includes(`SELECT-${tag}-ME`), 'the denied-clipboard fallback did not carry the selection');
      ok('pointer selection (word and drag); clipboard allowed: written; clipboard denied: falls back to execCommand copy');
    }

    // 7. Terminal mouse reporting: a program that asks for the mouse gets a click as an escape sequence, not a selection.
    tmux('send-keys', '-t', '=alpha:', 'clear; printf "\\033[?1000h\\033[?1006h"; cat -v', 'Enter');
    await sleep(500);
    const area = await page.locator('.xterm-screen').boundingBox();
    await page.mouse.click(area.x + 40, area.y + 40);
    await waitFor(async () => /\^\[\[<0;\d+;\d+M/.test(await screen()), 'a mouse report from a click', 15000)
      .then(() => ok('mouse reporting: a click reaches a program that asked for the mouse'))
      .catch(() => check(false, 'a click did not reach a program that enabled mouse reporting'));
    tmux('send-keys', '-t', '=alpha:', 'C-c');
    await sleep(200);
    tmux('send-keys', '-t', '=alpha:', 'printf "\\033[?1000l\\033[?1006l"; clear; echo READY-alpha', 'Enter');
    await waitFor(async () => (await screen()).includes('READY-alpha'), 'the shell back after mouse mode');

    // 8. Resize with the session active: xterm is retained, the rows follow the window and the fit leaves less than a row.
    const fit = () => page.evaluate(() => {
      const rows = document.querySelector('.xterm-rows');
      const screenEl = document.querySelector('.xterm-screen');
      const host = document.querySelector('.xterm').parentElement.getBoundingClientRect();
      const s = screenEl.getBoundingClientRect();
      const rowH = rows.children[0].getBoundingClientRect().height;
      return { rows: rows.children.length, slack: host.height - s.height, rowH, dpr: devicePixelRatio };
    });
    const size = page.viewportSize();
    const before = await fit();
    check(before.slack >= -1 && before.slack < before.rowH + 1, `fit leaves ${before.slack}px of ${before.rowH}px rows at ${size.width}x${size.height}`);
    await page.setViewportSize({ width: size.width, height: size.height - 200 });
    await waitFor(async () => (await fit()).rows < before.rows, 'fewer rows after the window got shorter');
    const shorter = await fit();
    check(shorter.slack >= -1 && shorter.slack < shorter.rowH + 1, `fit after shrinking leaves ${shorter.slack}px`);
    await page.setViewportSize(size);
    await waitFor(async () => (await fit()).rows === before.rows, 'the rows back after restoring the window');
    check(await term.evaluate((e) => e.isConnected), 'the xterm element was replaced by a resize');
    { const o = await sideways(page); check(!o.scrollable, `the page scrolls sideways after a resize (${o.px}px, ${o.culprit})`); }
    ok(`resize with an active session: xterm retained, ${before.rows} rows -> ${shorter.rows} -> ${before.rows}`);
    check(before.dpr === DPR, `devicePixelRatio is ${before.dpr}, not ${DPR}`);
  } catch (e) {
    check(false, `scenario aborted: ${e instanceof Error ? e.message : e}`);
  } finally {
    await keepIfFailed(ctx, label, failures.length > 0).catch((e) => console.error(`could not save evidence: ${e.message}`));
    await ctx.close();
  }
  return failures;
}

/** The 720/721 transition on one page (the sidebar stays through 1023 px, the phone layout starts at 720): the layout flips at the breakpoint and the session survives. */
async function runBreakpoints(browser) {
  const engine = ENGINE;
  const failures = [];
  const check = (cond, msg) => { if (!cond) { failures.push(msg); console.error(`FAIL [${engine} breakpoints]: ${msg}`); } return !!cond; };
  const ctx = await browser.newContext(contextOptions({ viewport: { width: 1440, height: 900 }, isMobile: false, hasTouch: false }));
  await trace(ctx, 'breakpoints');
  try {
    const page = await ctx.newPage();
    page.on('pageerror', (e) => check(false, `page error: ${e.message}`));
    await signIn(page);
    await page.locator('[data-shell=sidebar]').locator('[data-key$="/alpha"]').click();
    await page.locator('.xterm-rows').waitFor();
    const term = await page.locator('.xterm').elementHandle();
    const state = () => page.evaluate(() => ({
      sidebar: !!document.querySelector('[data-shell=sidebar]'),
      bar: !!document.querySelector('[data-shell=bottom-bar]'),
      mobile: !!document.querySelector('.workspace.mobile'),
    }));
    const at = async (width) => { await page.setViewportSize({ width, height: 800 }); await page.waitForTimeout(250); return { ...(await state()), ...(await sideways(page)) }; };
    const wide = { sidebar: true, bar: false, mobile: false };
    for (const [width, want] of [[1024, wide], [1023, wide], [721, wide], [720, { sidebar: false, bar: true, mobile: true }], [721, wide], [1024, wide]]) {
      const got = await at(width);
      check(got.sidebar === want.sidebar && got.bar === want.bar && got.mobile === want.mobile, `at ${width}px: sidebar ${got.sidebar}, bottom bar ${got.bar}, mobile ${got.mobile}; expected ${JSON.stringify(want)}`);
      check(!got.scrollable, `at ${width}px the page scrolls sideways (${got.px}px, ${got.culprit})`);
    }
    // The session view is rebuilt by the layout switch only if the shell remounts it; a switch must not lose the terminal screen.
    await waitFor(async () => (await page.locator('.xterm-rows').innerText()).includes('READY-alpha'), 'the alpha screen after the breakpoint round trip');
    check(await term.evaluate((e) => e.isConnected) || (await page.locator('.xterm').count()) === 1, 'no xterm after the breakpoint round trip');
    console.log(`ok  [${engine} breakpoints] 721/720 flips the layout, 1024 and 1023 keep the sidebar, and the session stays on screen`);
  } catch (e) {
    check(false, `scenario aborted: ${e instanceof Error ? e.message : e}`);
  } finally {
    await keepIfFailed(ctx, 'breakpoints', failures.length > 0).catch(() => {});
    await ctx.close();
  }
  return failures;
}

let failures = [];
let browser;
try {
  await startServer();
  // Real tmux sessions on the isolated server; the shell prints a marker so the attached screen is recognisable.
  for (const name of ['alpha', 'beta']) {
    tmux('new-session', '-d', '-s', name, '-x', '100', '-y', '30', `bash --norc -c 'PS1="$ "; echo READY-${name}; exec bash --norc -i'`);
  }
  browser = await launch();
  const vp = viewport('1280x800');
  const label = `${vp.width > 720 ? 'desktop' : 'phone'}-${vp.width}x${vp.height}-${DPR}x`;
  failures.push(...(await runContext(browser, label)));
  if (vp.width > 720) failures.push(...(await runBreakpoints(browser)));
} catch (e) {
  failures.push(e instanceof Error ? e.stack : String(e));
  console.error(`FAIL: ${failures.at(-1)}`);
} finally {
  await browser?.close();
  server.kill();
  try { tmux('kill-server'); } catch {}
  appServer.close();
  rmSync(home, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
  console.log(failures.length ? `FAIL: ${failures.length} check(s) failed` : 'PASS');
  process.exit(failures.length ? 1 : 0);
}
