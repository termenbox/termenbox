// The real App.svelte (built into ../dist) signed in with a forged local certificate and talking to a scripted server
// over a WebSocket that Playwright answers (page.routeWebSocket), so the real hub, connection, terminal and workspace view
// run end to end without a server binary. What this does not prove: a real tmux, the real server's resize handling and
// the real agent hooks (e2e/run.mjs covers the server; the decision text here is what the protocol carries, not captured).
import { launch, contextOptions, trace, keepIfFailed, ENGINE } from './browser.mjs';
import { createServer } from 'node:http';
import { readFileSync, existsSync, statSync } from 'node:fs';
import { join, extname, basename } from 'node:path';

const dist = join(import.meta.dirname, '../dist');
const mime = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.json': 'application/json', '.png': 'image/png', '.woff2': 'font/woff2' };
const b64 = (o) => Buffer.from(JSON.stringify(o)).toString('base64').replace(/\+/g, '-').replace(/\//g, '_');
const cert = `${b64({ exp: Math.floor(Date.now() / 1000) + 86400, sub: 1 })}.sig`;
const b64text = (s) => Buffer.from(s).toString('base64');

/** What Claude Code asks for a tool: its second option grants standing permission, the third is the explicit No. */
export const CLAUDE_PROMPT = {
  id: 'p1', question: 'Do you want to run npm test?', kind: 'choice',
  options: [{ label: 'Yes' }, { label: 'Yes, and don’t ask again for npm test commands' }, { label: 'No, and tell Claude what to do differently' }],
};

export async function serveDist() {
  const srv = createServer((req, res) => {
    let f = join(dist, req.url.split('?')[0].split('#')[0]);
    if (!f.startsWith(dist) || !existsSync(f) || statSync(f).isDirectory()) f = join(dist, 'index.html');
    res.writeHead(200, { 'Content-Type': mime[extname(f)] ?? 'application/octet-stream' }).end(readFileSync(f));
  });
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  return { srv, origin: `http://127.0.0.1:${srv.address().port}/` };
}

/** One scripted server with workspaces s1 (waiting on a decision), s2 (also waiting) and s3 (a plain shell). */
export class ScriptedServer {
  received = [];
  sockets = new Set();
  views = new Map();
  states = { s1: 'needs_input', s2: 'needs_input', s3: 'idle' };
  names = { s1: 'build', s2: 'deploy', s3: 'shell' };
  constructor() {
    this.views.set('s1', this.view('s1', { prompt: CLAUDE_PROMPT, tail: ['$ npm test', 'waiting for permission'] }));
    this.views.set('s2', this.view('s2', { prompt: { ...CLAUDE_PROMPT, id: 'p2' }, tail: ['deploying'] }));
    this.views.set('s3', this.view('s3', { tail: ['$'] }));
  }
  view(workspace, extra) {
    return { type: 'workspace_view', workspace: workspace, tier: 't1', agent: 'claude', tail: [], hash: `h${Math.random().toString(36).slice(2)}`, task: 'Fix the login test', ...extra };
  }
  workspaces() { return Object.keys(this.names).map((id) => ({ id, name: this.names[id], agent: null })); }
  snapshot() {
    return { type: 'agent_snapshot', states: Object.keys(this.names).map((workspace) => ({ workspace: workspace, state: this.states[workspace], seq: 1, ts: Date.now(), stale: false, agent: 'claude', waiting_since: this.states[workspace] === 'needs_input' ? Date.now() - (workspace === 's2' ? 90000 : 30000) : undefined })) };
  }
  send(ws, m) { ws.send(JSON.stringify(m)); }
  broadcast(m) { for (const ws of this.sockets) this.send(ws, m); }
  /** Terminal output reaches every connection, as a live workspace's would. */
  output(text) { this.broadcast({ type: 'output', data: b64text(text) }); }
  setView(workspace, extra) { this.views.set(workspace, this.view(workspace, extra)); this.broadcast(this.views.get(workspace)); }
  handle(ws, m) {
    this.received.push(m);
    switch (m.type) {
      case 'hello': this.send(ws, { type: 'challenge', nonce: 'nonce', server_id: 'one' }); break;
      case 'auth':
        this.send(ws, { type: 'auth_ok', user: 'tester', server_name: 'one', version: '0.1.31', backends: ['tmux'], control_mode: true, prefix: 'C-b' });
        this.send(ws, { type: 'workspaces', workspaces: this.workspaces() });
        this.send(ws, this.snapshot());
        break;
      case 'list_workspaces': this.send(ws, { type: 'workspaces', workspaces: this.workspaces() }); break;
      case 'ping': this.send(ws, { type: 'pong' }); break;
      case 'attach':
        this.send(ws, { type: 'workspace_attached', workspace_id: m.workspace_id });
        this.send(ws, { type: 'output', data: b64text(`attached to ${m.workspace_id}\r\n$ `) });
        break;
      case 'workspace_view': {
        const v = this.views.get(m.workspace);
        if (v) this.send(ws, m.hash === v.hash ? { type: 'workspace_view', workspace: m.workspace, tier: v.tier, tail: [], hash: v.hash, unchanged: true } : v);
        break;
      }
      case 'workspace_answer':
        // The agent takes the answer: the prompt clears and the workspace goes back to work.
        this.states[m.workspace] = 'working';
        this.setView(m.workspace, { prompt: undefined, tail: ['answered option ' + m.option] });
        this.broadcast(this.snapshot());
        break;
      default: break;
    }
  }
  of(type) { return this.received.filter((m) => m.type === type); }
  clear() { this.received.length = 0; }
}

/** One static server and one browser (browser.mjs) for every scenario; each scenario gets its own context (own storage). */
export async function startShared() {
  const { srv, origin } = await serveDist();
  const browser = await launch();
  return { origin, browser, stop: async () => { await browser.close(); srv.close(); } };
}

let scenarios = 0;

export async function startApp(shared, { viewport, deviceScaleFactor, storage = {}, blockedStorage = false, clipboard = false, server = new ScriptedServer() } = {}) {
  const { origin, browser } = shared;
  const ctx = await browser.newContext(contextOptions({ viewport, ...(deviceScaleFactor ? { deviceScaleFactor } : {}), isMobile: false, hasTouch: false, ...(clipboard && ENGINE === 'chromium' ? { permissions: ['clipboard-read', 'clipboard-write'] } : {}) }));
  const label = `${basename(process.argv[1], '.mjs')}-${++scenarios}`;
  await trace(ctx, label);
  const seed = { termenbox_auth: JSON.stringify({ cert, login: 'tester' }), termenbox_ios_hint: '1',
    termenbox_servers: JSON.stringify([{ server_id: 'one', server_name: 'one', server_url: 'ws://127.0.0.1:9/ws', registered_at: 0 }]),
    termenbox_settings: JSON.stringify({ renderer: 'dom' }), ...storage };
  await ctx.addInitScript(([items, blocked]) => {
    // Seed once per tab, not on every reload, so a saved layout survives the reload the tests do.
    if (blocked) {
      const boom = () => { throw new DOMException('denied', 'SecurityError'); };
      Object.defineProperty(window, 'localStorage', { get: boom });
      return;
    }
    if (!sessionStorage.getItem('__seeded')) {
      sessionStorage.setItem('__seeded', '1');
      for (const [k, v] of Object.entries(items)) if (localStorage.getItem(k) === null) localStorage.setItem(k, v);
    }
    window.__writes = [];
    const set = Storage.prototype.setItem;
    Storage.prototype.setItem = function (k, v) { window.__writes.push(k); return set.call(this, k, v); };
  }, [seed, blockedStorage]);
  // Only Chromium lets a test grant and read the real clipboard. Elsewhere the page's writes are recorded as they are made,
  // which proves what the app hands the clipboard API but not that the browser accepts it.
  if (clipboard && ENGINE !== 'chromium') {
    await ctx.addInitScript(() => {
      window.__clip = [];
      Object.defineProperty(navigator, 'clipboard', { value: { writeText: async (x) => { window.__clip.push(String(x)); } }, configurable: true });
    });
  }
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => { if (m.type() === 'error' && !/WebSocket|favicon|Navigated away from page|Viewport argument key "interactive-widget"/.test(m.text())) errors.push(m.text()); });
  await page.routeWebSocket(/127\.0\.0\.1:9\/ws/, (ws) => {
    server.sockets.add(ws);
    ws.onMessage((raw) => { try { server.handle(ws, JSON.parse(typeof raw === 'string' ? raw : raw.toString())); } catch { /* binary lane frames are not used here */ } });
    ws.onClose(() => server.sockets.delete(ws));
  });
  let failed = false;
  const t = {
    page, server, errors, ctx,
    fail: (m) => { failed = true; console.error(`FAIL: ${m}`); },
    ok: (m) => console.log(`ok  ${m}`),
    check(cond, m) { if (!cond) t.fail(m); return !!cond; },
    async open() {
      await page.goto(origin);
      try {
        await page.getByRole('navigation', { name: 'Main' }).first().waitFor({ timeout: 15000 });
        await page.waitForFunction(() => document.querySelector('[data-shell=frame]'));
      } catch (e) {
        throw new Error(`${e.message}\npage errors: ${errors.join(' | ') || 'none'}\npage text: ${(await page.locator('body').innerText({ timeout: 2000 }).catch(() => '')).slice(0, 300)}`, { cause: e });
      }
    },
    async reload() { await page.reload(); await page.getByRole('navigation', { name: 'Main' }).first().waitFor(); },
    // The body, not the root: xterm parks a 50000 px wide measuring div above the page, which Firefox counts in the root's scroll width.
    overflow: () => page.evaluate(() => document.body.scrollWidth - innerWidth),
    /** Opens workspace `id` from the sidebar and waits for the attach. The terminal takes focus when the workspace attaches (so
     *  typing works at once); `leaveTerminal` (default) hands it back to the page, as a click on empty space would. */
    async openWorkspace(id, { leaveTerminal = true } = {}) {
      await page.locator(`[data-shell=sidebar] [data-key="one/${id}"]`).click();
      await page.locator('.xterm').first().waitFor();
      await page.waitForFunction((n) => [...document.querySelectorAll('.xterm-rows > div')].some((d) => d.textContent.includes(`attached to ${n}`)), id);
      await page.waitForFunction(() => document.activeElement?.classList.contains('xterm-helper-textarea'));
      await page.waitForTimeout(500); // a second attach (the app sends one more when the tab settles) refocuses the terminal
      if (leaveTerminal) await t.leaveTerminal(id);
    },
    /** Focus goes to the workspace's row in the sidebar, as a click there does. (Blurring to nothing does not work: the terminal takes the focus straight back so a hidden phone keyboard leaves it active.) */
    async leaveTerminal(id = 's1') { await page.locator(`[data-shell=sidebar] [data-key="one/${id}"]`).focus(); },
    async waitResize(sinceCount) {
      await page.waitForFunction(() => true);
      const deadline = Date.now() + 5000;
      while (Date.now() < deadline) { if (server.of('resize').length > sinceCount) return server.of('resize').at(-1); await page.waitForTimeout(50); }
      return null;
    },
    /** The clipboard text: the real one on Chromium, the last write the page made elsewhere. */
    clipboard: () => ENGINE === 'chromium' ? page.evaluate(() => navigator.clipboard.readText()) : page.evaluate(() => window.__clip.at(-1) ?? ''),
    setClipboard: (text) => ENGINE === 'chromium' ? page.evaluate((x) => navigator.clipboard.writeText(x), text) : page.evaluate((x) => { window.__clip.push(x); }, text),
    waitClipboard: (text) => page.waitForFunction(([e, want]) => (e === 'chromium' ? navigator.clipboard.readText() : Promise.resolve(window.__clip.at(-1) ?? '')).then((x) => x.includes(want)), [ENGINE, text], { timeout: 3000 }).catch(() => {}),
    box: async (sel) => (await page.locator(sel).first().boundingBox()),
    /** The text xterm's DOM renderer shows. */
    screen: () => page.evaluate(() => [...document.querySelectorAll('.xterm-rows > div')].map((d) => d.textContent).join('\n')),
    /** Ends the scenario; true when it passed. */
    async end() {
      t.check(errors.length === 0, `console/page errors: ${errors.slice(0, 3).join(' | ')}`);
      await keepIfFailed(ctx, label, failed);
      await ctx.close();
      return !failed;
    },
  };
  return t;
}
