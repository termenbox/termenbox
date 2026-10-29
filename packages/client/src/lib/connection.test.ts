import { afterEach, describe, it, expect, vi } from 'vitest';
import { backoffDelay, b64ToBytes, textToB64, Connection } from './connection';
import { PROTOCOL_VERSION } from './extensions';
import type { Transport } from './transport';

describe('backoffDelay', () => {
  it('doubles from 1s up to a 30s cap', () => {
    const none = () => 0;
    expect([0, 1, 2, 3, 4, 5, 6, 20].map((n) => backoffDelay(n, none))).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000, 30000]);
  });
  it('adds at most 25% jitter', () => {
    expect(backoffDelay(0, () => 1)).toBe(1250);
  });
});

describe('utf-8 base64', () => {
  it('round-trips non-latin text as bytes', () => {
    const bytes = b64ToBytes(textToB64('héllo ✓'));
    expect(new TextDecoder().decode(bytes)).toBe('héllo ✓');
  });
});

describe('heartbeat', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  function fake() {
    const sent: string[] = [];
    const t: Transport = { kind: 'ws', onmessage: null, onclose: null, send: (x) => void sent.push(x), close: () => {} };
    return { t, sent };
  }

  async function connected() {
    vi.useFakeTimers();
    const doc = Object.assign(new EventTarget(), { visibilityState: 'visible' });
    vi.stubGlobal('document', doc);
    vi.stubGlobal('window', { addEventListener: () => {}, removeEventListener: () => {} });
    const { t, sent } = fake();
    const drops: string[] = [];
    const states: string[] = [];
    const c = new Connection({
      target: async () => ({ wsUrl: 'ws://x' }),
      openers: { rtc: async () => t, ws: async () => t },
      credentials: async () => ({ cert: 'c', sign: async () => 's' }),
      onState: (s) => states.push(s),
      onReady: () => {},
      onFatal: () => {},
      onDrop: (reason) => drops.push(reason),
    });
    c.start();
    await vi.advanceTimersByTimeAsync(10);
    t.onmessage!(JSON.stringify({ type: 'challenge', nonce: 'n', server_id: 's' }));
    await vi.advanceTimersByTimeAsync(10);
    t.onmessage!(JSON.stringify({ type: 'auth_ok', user: 'u', server_name: 's' }));
    return { c, t, sent, drops, states, doc };
  }

  it('keeps pinging while hidden, slowly, so an idle path does not cut the link', async () => {
    const { c, t, sent, drops, doc } = await connected();
    await vi.advanceTimersByTimeAsync(5000);
    t.onmessage!(JSON.stringify({ type: 'pong' }));
    doc.visibilityState = 'hidden'; doc.dispatchEvent(new Event('visibilitychange'));
    const count = sent.length;
    for (let i = 0; i < 6; i++) {
      await vi.advanceTimersByTimeAsync(20000);
      t.onmessage!(JSON.stringify({ type: 'pong' }));
    }
    expect(sent.length - count).toBe(6); // one ping every 20 s, i.e. well inside a minute
    expect(drops).toEqual([]);
    c.close();
  });

  it('on foregrounding, finds a link that died while hidden within seconds, not the full window', async () => {
    const { c, drops, states, doc } = await connected();
    doc.visibilityState = 'hidden'; doc.dispatchEvent(new Event('visibilitychange'));
    doc.visibilityState = 'visible'; doc.dispatchEvent(new Event('visibilitychange'));
    await vi.advanceTimersByTimeAsync(2600);
    expect(states[states.length - 1]).toBe('checking'); // visible to the user before the drop
    await vi.advanceTimersByTimeAsync(600);
    expect(drops).toHaveLength(1);
    expect(drops[0]).toContain('no reply for 3 s');
    c.close();
  });

  it('goes back to connected when a slow reply finally arrives', async () => {
    const { t, states, drops } = await connected();
    await vi.advanceTimersByTimeAsync(5000 + 3000); // ping sent, then 3 s of silence
    expect(states[states.length - 1]).toBe('checking');
    t.onmessage!(JSON.stringify({ type: 'pong' }));
    expect(states[states.length - 1]).toBe('connected');
    expect(drops).toEqual([]);
  });

  it('reports the retry attempt and the wait while reconnecting, and retryNow skips the wait', async () => {
    const { c, t, states } = await connected();
    t.onclose!();
    expect(states[states.length - 1]).toBe('reconnecting');
    expect(c.retry?.attempt).toBe(1);
    expect(c.retry?.inMs).toBeGreaterThan(0);
    c.retryNow();
    await vi.advanceTimersByTimeAsync(10);
    expect(c.retry).toBeNull(); // attempt 0 again: the first retry runs at once
    c.close();
  });

  it('survives a reply that is several seconds late, which is normal on a mobile link', async () => {
    const { t, drops, states } = await connected();
    await vi.advanceTimersByTimeAsync(5000); // a ping goes out
    await vi.advanceTimersByTimeAsync(8000); // 8 s without a reply
    expect(drops).toEqual([]);
    t.onmessage!(JSON.stringify({ type: 'pong' }));
    await vi.advanceTimersByTimeAsync(5000);
    expect(drops).toEqual([]);
    t.onmessage!(JSON.stringify({ type: 'pong' })); // the next ping is answered too
    expect(states[states.length - 1]).toBe('connected');
  });

  it('drops a connection that has been silent for the whole window, and says why', async () => {
    const { c, drops, states } = await connected();
    await vi.advanceTimersByTimeAsync(5000 + 12000 + 100);
    expect(drops).toHaveLength(1);
    expect(drops[0]).toContain('no reply for 12 s');
    expect(c.drops).toBe(1);
    expect(states).toContain('reconnecting');
  });
});

describe('terminal lane', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  async function connect(lanes: boolean, channels?: string[]) {
    vi.useFakeTimers();
    vi.stubGlobal('document', Object.assign(new EventTarget(), { visibilityState: 'visible' }));
    vi.stubGlobal('window', { addEventListener: () => {}, removeEventListener: () => {} });
    const sent: string[] = [];
    const term: Uint8Array[] = [];
    let termOpen = true;
    const t: Transport = {
      kind: 'ws',
      onmessage: null,
      onclose: null,
      send: (x) => void sent.push(x),
      close: () => {},
      ...(lanes ? { lanes: true, onterm: null, sendTerm: (b: Uint8Array) => (termOpen ? (term.push(b), true) : false) } : {}),
    };
    const heard: string[] = [];
    const c = new Connection({
      target: async () => ({ wsUrl: 'ws://x' }),
      openers: { rtc: async () => t, ws: async () => t },
      credentials: async () => ({ cert: 'c', sign: async () => 's' }),
      onState: () => {},
      onReady: () => {},
      onFatal: () => {},
    });
    c.subscribe((m) => heard.push(m.type === 'term' ? `term:${new TextDecoder().decode(m.bytes)}` : m.type));
    c.start();
    await vi.advanceTimersByTimeAsync(10);
    t.onmessage!(JSON.stringify({ type: 'challenge', nonce: 'n', server_id: 's' }));
    await vi.advanceTimersByTimeAsync(10);
    t.onmessage!(JSON.stringify({ type: 'auth_ok', user: 'u', server_name: 's', ...(channels ? { channels } : {}) }));
    return { c, t, sent, term, heard, close: () => (termOpen = false) };
  }

  it('asks for the lane only on a carrier that has one', async () => {
    const a = await connect(true, ['term']);
    expect(JSON.parse(a.sent.find((s) => s.includes('"auth"'))!).channels).toBe(true);
    a.c.close();
    const b = await connect(false);
    expect(JSON.parse(b.sent.find((s) => s.includes('"auth"'))!).channels).toBeUndefined();
    b.c.close();
  });

  it('sends keystrokes as raw bytes and hands received bytes to listeners once the server agreed', async () => {
    const { c, t, sent, term, heard } = await connect(true, ['term']);
    c.send({ type: 'input', data: textToB64('ls\r') });
    expect(term.map((b) => new TextDecoder().decode(b))).toEqual(['ls\r']);
    expect(sent.some((s) => s.includes('"input"'))).toBe(false);
    t.onterm!(new TextEncoder().encode('hello'));
    expect(heard).toContain('term:hello');
    c.close();
  });

  it('keeps the JSON pipe when the server did not list the lane (an older server) or the lane is not open', async () => {
    const old = await connect(true);
    old.c.send({ type: 'input', data: textToB64('a') });
    expect(old.term).toEqual([]);
    expect(old.sent.some((s) => s.includes('"input"'))).toBe(true);
    old.c.close();
    const shut = await connect(true, ['term']);
    shut.close();
    shut.c.send({ type: 'input', data: textToB64('b') });
    expect(shut.sent.some((s) => s.includes('"input"'))).toBe(true);
    shut.c.close();
  });
});

describe('a connect that fails', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('says why WebRTC failed and logs what each transport reported, then retries', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('document', Object.assign(new EventTarget(), { visibilityState: 'visible' }));
    vi.stubGlobal('window', { addEventListener: () => {}, removeEventListener: () => {} });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const states: Array<[string, string | undefined]> = [];
    const c = new Connection({
      target: async () => ({ record: { udp_port: 4443 } as never }),
      openers: { rtc: async () => { throw new Error('ICE failed'); }, ws: async () => { throw new Error('unused'); } },
      credentials: async () => ({ cert: 'c', sign: async () => 's' }),
      onState: (s, detail) => states.push([s, detail]),
      onReady: () => {},
      onFatal: () => {},
      onDrop: () => {},
    });
    c.start();
    await vi.advanceTimersByTimeAsync(10);
    expect(states.at(-1)).toEqual(['unreachable', expect.stringContaining('UDP port 4443 (ICE failed)')]);
    expect(warn).toHaveBeenCalledWith('[termenbox] connect failed:', 'rtc: ICE failed');
    expect(c.retry).not.toBeNull();
    c.close();
  });
});

describe('after a network change (decision 0123)', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  /** A connection whose carrier can be replaced, with a working `window` and `navigator.connection`. */
  async function setup() {
    vi.useFakeTimers();
    const doc = Object.assign(new EventTarget(), { visibilityState: 'visible' });
    const win = new EventTarget();
    const net = new EventTarget();
    vi.stubGlobal('document', doc);
    vi.stubGlobal('window', win);
    vi.stubGlobal('navigator', { connection: net });
    const made: { t: Transport; sent: string[]; closed: boolean }[] = [];
    const open = async () => {
      const rec = { sent: [] as string[], closed: false } as { t: Transport; sent: string[]; closed: boolean };
      rec.t = {
        kind: 'rtc', onmessage: null, onclose: null, send: (x) => void rec.sent.push(x), close: () => { rec.closed = true; },
        path: { key: `lan 10.0.0.${made.length + 2}:4443`, kind: 'lan', label: 'Direct, LAN', addr: '10.0.0.2', port: 4443 },
      };
      made.push(rec);
      return rec.t;
    };
    const paths: (string | null)[] = [];
    const winners: string[] = [];
    const states: string[] = [];
    const c = new Connection({
      target: async () => ({ wsUrl: 'ws://x' }),
      openers: { rtc: open, ws: open },
      credentials: async () => ({ cert: 'c', sign: async () => 's' }),
      onState: (s) => states.push(s),
      onReady: () => {},
      onFatal: () => {},
      onPath: (p) => paths.push(p?.label ?? null),
      onWinner: (k) => winners.push(k),
    });
    const login = async (rec: { t: Transport }) => {
      await vi.advanceTimersByTimeAsync(10);
      rec.t.onmessage!(JSON.stringify({ type: 'challenge', nonce: 'n', server_id: 's' }));
      await vi.advanceTimersByTimeAsync(10);
      rec.t.onmessage!(JSON.stringify({ type: 'auth_ok', user: 'u', server_name: 's' }));
    };
    c.start();
    await login({ t: await vi.waitFor(() => made[0].t) });
    return { c, made, win, net, doc, paths, winners, states, login };
  }

  it('shows the path that carries the workspace and remembers it as the winner once authenticated', async () => {
    const { c, paths, winners } = await setup();
    expect(paths).toEqual(['Direct, LAN']);
    expect(c.pathLabel).toBe('Direct, LAN');
    expect(winners).toEqual(['lan 10.0.0.2:4443']);
    c.close();
  });

  it('re-races at once when the network changed and the link does not answer within 2 s, with no backoff wait', async () => {
    const { c, made, net, login } = await setup();
    net.dispatchEvent(new Event('change')); // Wi-Fi to cellular
    expect(made[0].sent.at(-1)).toContain('"ping"');
    await vi.advanceTimersByTimeAsync(1900);
    expect(made).toHaveLength(1); // still waiting for the answer
    await vi.advanceTimersByTimeAsync(200);
    // No reply: the old carrier is dropped and a new race has started, not after the 1 s backoff but immediately.
    expect(made[0].closed).toBe(true);
    await vi.advanceTimersByTimeAsync(10);
    expect(made).toHaveLength(2);
    await login(made[1]);
    expect(c.pathLabel).toBe('Direct, LAN');
    c.close();
  });

  it('keeps the link when it answers the proof of life after a network change', async () => {
    const { c, made, win } = await setup();
    win.dispatchEvent(new Event('online')); // a foregrounding ping and a proof-of-life ping
    const pings = made[0].sent.filter((x) => x.includes('"ping"')).length;
    for (let i = 0; i < pings; i++) made[0].t.onmessage!(JSON.stringify({ type: 'pong' }));
    await vi.advanceTimersByTimeAsync(5000);
    expect(made).toHaveLength(1);
    expect(made[0].closed).toBe(false);
    c.close();
  });
  it('does not take a pong that was already in flight for the proof of life, and drops a dead link within 2 s', async () => {
    const { c, made, win, doc } = await setup();
    doc.dispatchEvent(new Event('visibilitychange')); // a ping is on its way...
    win.dispatchEvent(new Event('online')); // ...when the network changes (and one more ping asks for proof)
    made[0].t.onmessage!(JSON.stringify({ type: 'pong' })); // the old answer arrives; the new ping is never answered
    await vi.advanceTimersByTimeAsync(2100);
    expect(made[0].closed).toBe(true);
    await vi.advanceTimersByTimeAsync(10);
    expect(made).toHaveLength(2); // re-raced at once, no backoff
    c.close();
  });

  it('keeps the link once every ping sent since the change has been answered', async () => {
    const { c, made, win, doc } = await setup();
    doc.dispatchEvent(new Event('visibilitychange'));
    win.dispatchEvent(new Event('online'));
    const pings = made[0].sent.filter((x) => x.includes('"ping"')).length;
    for (let i = 0; i < pings; i++) made[0].t.onmessage!(JSON.stringify({ type: 'pong' }));
    await vi.advanceTimersByTimeAsync(5000);
    expect(made).toHaveLength(1);
    expect(made[0].closed).toBe(false);
    c.close();
  });

  it('other traffic does not cancel the 2 s deadline of a network change: only the answer to the ping does', async () => {
    const { c, made, net } = await setup();
    net.dispatchEvent(new Event('change'));
    made[0].t.onmessage!(JSON.stringify({ type: 'server_health', health: {} })); // buffered data from the old network
    await vi.advanceTimersByTimeAsync(2100);
    expect(made[0].closed).toBe(true);
    c.close();
  });

  it('a foregrounding right after a network change does not stretch the 2 s deadline to 3 s', async () => {
    const { c, made, net, doc } = await setup();
    net.dispatchEvent(new Event('change'));
    doc.dispatchEvent(new Event('visibilitychange')); // wake asks for a 3 s answer
    await vi.advanceTimersByTimeAsync(2100);
    expect(made[0].closed).toBe(true);
    c.close();
  });

  it('a drop with no network change goes through the backoff, whatever happened to an earlier change', async () => {
    const { c, made, win } = await setup();
    win.dispatchEvent(new Event('online'));
    const pings = made[0].sent.filter((x) => x.includes('"ping"')).length;
    for (let i = 0; i < pings; i++) made[0].t.onmessage!(JSON.stringify({ type: 'pong' }));
    made[0].t.onclose!(); // an unrelated drop
    await vi.advanceTimersByTimeAsync(100);
    expect(made).toHaveLength(1); // waiting out the 1 s backoff
    c.close();
  });
});

describe('version negotiation and the legacy shim (step 3.2)', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  /** A connection to a server whose `auth_ok` is `authOk`. */
  async function against(authOk: object) {
    vi.useFakeTimers();
    vi.stubGlobal('document', Object.assign(new EventTarget(), { visibilityState: 'visible' }));
    vi.stubGlobal('window', { addEventListener: () => {}, removeEventListener: () => {} });
    const sent: string[] = [];
    const t: Transport = { kind: 'ws', onmessage: null, onclose: null, send: (x) => void sent.push(x), close: () => {} };
    const c = new Connection({
      target: async () => ({ wsUrl: 'ws://x' }),
      openers: { rtc: async () => t, ws: async () => t },
      credentials: async () => ({ cert: 'c', sign: async () => 's' }),
      onState: () => {},
      onReady: () => {},
      onFatal: () => {},
      onDrop: () => {},
    });
    const heard: unknown[] = [];
    c.subscribe((m) => heard.push(m));
    c.start();
    await vi.advanceTimersByTimeAsync(10);
    t.onmessage!(JSON.stringify({ type: 'challenge', nonce: 'n', server_id: 's' }));
    await vi.advanceTimersByTimeAsync(10);
    t.onmessage!(JSON.stringify(authOk));
    return { c, t, sent, heard };
  }

  it('announces the protocol version in its hello', async () => {
    const { c, sent } = await against({ type: 'auth_ok', protocol: 1, user: 'u', server_name: 's' });
    expect(JSON.parse(sent[0])).toEqual({ type: 'hello', protocol: PROTOCOL_VERSION });
    c.close();
  });

  it('a current server is spoken to in the workspace vocabulary, unchanged', async () => {
    const { c, t, sent, heard } = await against({ type: 'auth_ok', protocol: 1, user: 'u', server_name: 's' });
    c.send({ type: 'list_workspaces' });
    expect(JSON.parse(sent.at(-1)!)).toEqual({ type: 'list_workspaces' });
    t.onmessage!(JSON.stringify({ type: 'workspaces', workspaces: [{ id: 'a', name: 'a', agent: null }] }));
    expect(heard.at(-1)).toEqual({ type: 'workspaces', workspaces: [{ id: 'a', name: 'a', agent: null }] });
    c.close();
  });

  // LEGACY SHIM test: deleted with the shim in step 8.1. The hello is the recorded one of a server before step 3.2.
  it('an old server (its auth_ok names no protocol) is translated both ways', async () => {
    const recorded = { type: 'auth_ok', user: 'alice', channels: ['term'], server_name: 'vps', mode: 'container', version: '0.1.52+abc', backends: ['tmux'], control_mode: true, tmux_version: '3.4', prefix: 'C-b' };
    const { c, t, sent, heard } = await against(recorded);
    c.send({ type: 'list_workspaces' });
    c.send({ type: 'close_workspace', workspace: 'a' });
    c.send({ type: 'attach', workspace_id: 'a' });
    expect(sent.slice(-3).map((s) => JSON.parse(s))).toEqual([
      { type: 'list_sessions' },
      { type: 'close_session', session: 'a' },
      { type: 'attach', session_id: 'a' },
    ]);
    t.onmessage!(JSON.stringify({ type: 'sessions', sessions: [{ id: 'a', name: 'a', agent: null }] }));
    t.onmessage!(JSON.stringify({ type: 'session_attached', session_id: 'a' }));
    t.onmessage!(JSON.stringify({ type: 'snapshot', session: 'a', text: 'session', hash: 'h' }));
    expect(heard.slice(-3)).toEqual([
      { type: 'workspaces', workspaces: [{ id: 'a', name: 'a', agent: null }] },
      { type: 'workspace_attached', workspace_id: 'a' },
      { type: 'snapshot', workspace: 'a', text: 'session', hash: 'h' },
    ]);
    c.close();
  });
});

describe('the control channel (step 3.3)', () => {
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  async function against(authOk: object, withControl: boolean) {
    vi.useFakeTimers();
    vi.stubGlobal('document', Object.assign(new EventTarget(), { visibilityState: 'visible' }));
    vi.stubGlobal('window', { addEventListener: () => {}, removeEventListener: () => {} });
    const sent: string[] = [];
    const control: string[] = [];
    const t: Transport = {
      kind: 'rtc', onmessage: null, onclose: null, send: (x) => void sent.push(x), close: () => {},
      ...(withControl ? { sendControl: (x: string) => { control.push(x); return true; } } : {}),
    };
    const c = new Connection({
      target: async () => ({ wsUrl: 'ws://x' }),
      openers: { rtc: async () => t, ws: async () => t },
      credentials: async () => ({ cert: 'c', sign: async () => 's' }),
      onState: () => {}, onReady: () => {}, onFatal: () => {}, onDrop: () => {},
    });
    c.start();
    await vi.advanceTimersByTimeAsync(10);
    t.onmessage!(JSON.stringify({ type: 'challenge', nonce: 'n', server_id: 's' }));
    await vi.advanceTimersByTimeAsync(10);
    t.onmessage!(JSON.stringify(authOk));
    return { c, t, sent, control };
  }
  const ok = { type: 'auth_ok', protocol: 1, control_channel: true, user: 'u', server_name: 's' };

  it('control messages use the control channel, everything else the first one', async () => {
    const { c, sent, control } = await against(ok, true);
    c.send({ type: 'acp_interrupt', workspace: 'w' });
    c.send({ type: 'list_workspaces' });
    expect(control.map((x) => JSON.parse(x).type)).toEqual(['acp_interrupt']);
    expect(sent.map((x) => JSON.parse(x).type)).toEqual(['hello', 'auth', 'list_workspaces']);
    c.close();
  });

  it('without a control channel (or on a server that does not offer it) they use the first channel', async () => {
    const a = await against(ok, false);
    a.c.send({ type: 'acp_interrupt', workspace: 'w' });
    expect(a.sent.map((x) => JSON.parse(x).type)).toContain('acp_interrupt');
    a.c.close();
    const b = await against({ type: 'auth_ok', protocol: 1, user: 'u', server_name: 's' }, true);
    b.c.send({ type: 'acp_interrupt', workspace: 'w' });
    expect(b.control).toEqual([]);
    expect(b.sent.map((x) => JSON.parse(x).type)).toContain('acp_interrupt');
    b.c.close();
  });

  it('a request resolves with the ack, carries a req id, and times out without one', async () => {
    const { c, t, control } = await against(ok, true);
    const done = c.request({ type: 'acp_interrupt', workspace: 'w' });
    const req = JSON.parse(control[0]).req;
    expect(typeof req).toBe('number');
    t.onmessage!(JSON.stringify({ type: 'control_ack', req, ok: true }));
    expect(await done).toEqual({ ok: true, acknowledged: true, error: undefined });
    const failed = c.request({ type: 'close_workspace', workspace: 'w' });
    t.onmessage!(JSON.stringify({ type: 'control_ack', req: JSON.parse(control[1]).req, ok: false, error: 'Unknown workspace' }));
    expect(await failed).toMatchObject({ ok: false, acknowledged: true, error: 'Unknown workspace' });
    const silent = c.request({ type: 'detach' }, 1000);
    await vi.advanceTimersByTimeAsync(1001);
    expect(await silent).toMatchObject({ ok: false, acknowledged: false });
    c.close();
  });

  it('a request on a server that does not acknowledge resolves at once, and a closed connection ends waiting ones', async () => {
    const old = await against({ type: 'auth_ok', user: 'u', server_name: 's' }, false);
    expect(await old.c.request({ type: 'detach' })).toEqual({ ok: true, acknowledged: false });
    expect(old.sent.map((x) => JSON.parse(x).type)).toContain('detach');
    old.c.close();
    const { c } = await against(ok, true);
    const pending = c.request({ type: 'detach' });
    c.close();
    expect(await pending).toMatchObject({ ok: false, acknowledged: false });
  });
});
