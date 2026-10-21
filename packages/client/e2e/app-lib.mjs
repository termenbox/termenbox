// The real App.svelte (built into ../dist) signed in with a forged local certificate and talking to a scripted server
// over a WebSocket that Playwright answers (page.routeWebSocket), so the real hub, connection, terminal and session view
// run end to end without a server binary. What this does not prove: a real tmux, the real server's resize handling and
// the real agent hooks (e2e/run.mjs covers the server; the decision text here is what the protocol carries, not captured).
import { chromium } from 'playwright-core';
import { createServer } from 'node:http';
import { readFileSync, existsSync, statSync } from 'node:fs';
import { join, extname } from 'node:path';

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

/** One scripted server with sessions s1 (waiting on a decision), s2 (also waiting) and s3 (a plain shell). */
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
  view(session, extra) {
    return { type: 'session_view', session, tier: 't1', agent: 'claude', tail: [], hash: `h${Math.random().toString(36).slice(2)}`, task: 'Fix the login test', ...extra };
  }
  sessions() { return Object.keys(this.names).map((id) => ({ id, name: this.names[id], agent: null })); }
  snapshot() {
    return { type: 'agent_snapshot', states: Object.keys(this.names).map((session) => ({ session, state: this.states[session], seq: 1, ts: Date.now(), stale: false, agent: 'claude', waiting_since: this.states[session] === 'needs_input' ? Date.now() - (session === 's2' ? 90000 : 30000) : undefined })) };
  }
  send(ws, m) { ws.send(JSON.stringify(m)); }
  broadcast(m) { for (const ws of this.sockets) this.send(ws, m); }
  /** Terminal output reaches every connection, as a live session's would. */
  output(text) { this.broadcast({ type: 'output', data: b64text(text) }); }
  setView(session, extra) { this.views.set(session, this.view(session, extra)); this.broadcast(this.views.get(session)); }
  handle(ws, m) {
    this.received.push(m);
    switch (m.type) {
      case 'hello': this.send(ws, { type: 'challenge', nonce: 'nonce', server_id: 'one' }); break;
      case 'auth':
        this.send(ws, { type: 'auth_ok', user: 'tester', server_name: 'one', version: '0.1.31', backends: ['tmux'], control_mode: true, prefix: 'C-b' });
        this.send(ws, { type: 'sessions', sessions: this.sessions() });
        this.send(ws, this.snapshot());
        break;
      case 'list_sessions': this.send(ws, { type: 'sessions', sessions: this.sessions() }); break;
      case 'ping': this.send(ws, { type: 'pong' }); break;
      case 'attach':
        this.send(ws, { type: 'session_attached', session_id: m.session_id });
        this.send(ws, { type: 'output', data: b64text(`attached to ${m.session_id}\r\n$ `) });
        break;
      case 'session_view': {
        const v = this.views.get(m.session);
        if (v) this.send(ws, m.hash === v.hash ? { type: 'session_view', session: m.session, tier: v.tier, tail: [], hash: v.hash, unchanged: true } : v);
        break;
      }
      case 'session_answer':
        // The agent takes the answer: the prompt clears and the session goes back to work.
        this.states[m.session] = 'working';
        this.setView(m.session, { prompt: undefined, tail: ['answered option ' + m.option] });
        this.broadcast(this.snapshot());
        break;
      default: break;
    }
  }
  of(type) { return this.received.filter((m) => m.type === type); }
  clear() { this.received.length = 0; }
}

/** One static server and one Chrome for every scenario; each scenario gets its own context (own storage). */
export async function startShared() {
  const { srv, origin } = await serveDist();
  const browser = await chromium.launch({ ...(process.env.CHROME_PATH ? { executablePath: process.env.CHROME_PATH } : { channel: 'chrome' }), args: ['--no-sandbox'] });
  return { origin, browser, stop: async () => { await browser.close(); srv.close(); } };
}

export async function startApp(shared, { viewport, storage = {}, blockedStorage = false, clipboard = false, server = new ScriptedServer() } = {}) {
  const { origin, browser } = shared;
  const ctx = await browser.newContext({ viewport, isMobile: false, hasTouch: false, ...(clipboard ? { permissions: ['clipboard-read', 'clipboard-write'] } : {}) });
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
  const page = await ctx.newPage();
  const errors = [];
  page.on('pageerror', (e) => errors.push(String(e)));
  page.on('console', (m) => { if (m.type() === 'error' && !/WebSocket|favicon/.test(m.text())) errors.push(m.text()); });
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
      await page.getByRole('navigation', { name: 'Main' }).first().waitFor();
      await page.waitForFunction(() => document.querySelector('[data-shell=frame]'));
    },
    async reload() { await page.reload(); await page.getByRole('navigation', { name: 'Main' }).first().waitFor(); },
    overflow: () => page.evaluate(() => document.documentElement.scrollWidth - innerWidth),
    /** Opens session `id` from the sidebar and waits for the attach. The terminal takes focus when the session attaches (so
     *  typing works at once); `leaveTerminal` (default) hands it back to the page, as a click on empty space would. */
    async openSession(id, { leaveTerminal = true } = {}) {
      await page.locator(`[data-shell=sidebar] [data-key="one/${id}"]`).click();
      await page.locator('.xterm').first().waitFor();
      await page.waitForFunction((n) => [...document.querySelectorAll('.xterm-rows > div')].some((d) => d.textContent.includes(`attached to ${n}`)), id);
      await page.waitForFunction(() => document.activeElement?.classList.contains('xterm-helper-textarea'));
      await page.waitForTimeout(500); // a second attach (the app sends one more when the tab settles) refocuses the terminal
      if (leaveTerminal) await t.leaveTerminal(id);
    },
    /** Focus goes to the session's row in the sidebar, as a click there does. (Blurring to nothing does not work: the terminal takes the focus straight back so a hidden phone keyboard leaves it active.) */
    async leaveTerminal(id = 's1') { await page.locator(`[data-shell=sidebar] [data-key="one/${id}"]`).focus(); },
    async waitResize(sinceCount) {
      await page.waitForFunction(() => true);
      const deadline = Date.now() + 5000;
      while (Date.now() < deadline) { if (server.of('resize').length > sinceCount) return server.of('resize').at(-1); await page.waitForTimeout(50); }
      return null;
    },
    box: async (sel) => (await page.locator(sel).first().boundingBox()),
    /** The text xterm's DOM renderer shows. */
    screen: () => page.evaluate(() => [...document.querySelectorAll('.xterm-rows > div')].map((d) => d.textContent).join('\n')),
    /** Ends the scenario; true when it passed. */
    async end() {
      t.check(errors.length === 0, `console/page errors: ${errors.slice(0, 3).join(' | ')}`);
      await ctx.close();
      return !failed;
    },
  };
  return t;
}
