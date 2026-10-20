import { describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { ServerHub, newSessionTargets, serverKey, sessionDestinations, type HubConnection } from './hub';
import type { ConnectionOptions } from './connection';
import type { ClientMessage, ServerListItem, ServerMessage } from './protocol';
import type { Target } from './transport';

const server = (id: string): ServerListItem => ({ server_id: id, server_url: '', server_name: id, registered_at: 0 });
class Fake implements HubConnection {
  started = 0;
  closed = 0;
  sent: ClientMessage[] = [];
  listener?: (m: ServerMessage) => void;
  constructor(readonly opts: ConnectionOptions) {}
  start() { this.started++; this.opts.onState('connecting'); }
  close() { this.closed++; this.opts.onState('closed'); }
  send(m: ClientMessage) { this.sent.push(m); }
  subscribe(fn: (m: ServerMessage) => void) { this.listener = fn; return () => { this.listener = undefined; }; }
  ready(reconnect = false) { this.opts.onState('connected'); this.opts.onReady({ type: 'auth_ok', user: 'u', server_name: 'host', backends: ['tmux'] }, reconnect); }
  emit(m: ServerMessage) { this.listener?.(m); }
}
function setup(target?: (server: ServerListItem, refresh: boolean) => Promise<Target>) {
  const connections: Fake[] = [];
  const ready: boolean[] = [];
  const fatal: string[] = [];
  const hub = new ServerHub({ target, onFatal: (id, reason) => fatal.push(`${id}: ${reason}`), credentials: async () => ({ cert: 'c', sign: async () => 's' }), onReady: (_id, _msg, reconnect) => ready.push(reconnect), factory: (opts) => {
    const c = new Fake(opts); connections.push(c); return c;
  } });
  return { hub, connections, ready, fatal };
}

describe('ServerHub', () => {
  it('starts every saved server once, even if start or add is repeated', () => {
    const { hub, connections } = setup();
    hub.start([server('a'), server('b')]); hub.start([server('a'), server('b')]); hub.add(server('a'));
    expect(connections.map(c => c.started)).toEqual([1, 1]);
    expect(get(hub.servers).map(s => s.state)).toEqual(['connecting', 'connecting']);
  });
  it('replaces a failed claim without dropping peers or overriding Pause', () => {
    const { hub, connections } = setup();
    hub.start([{ ...server('a'), claim: 'wrong' }, server('b')]);
    connections[0].opts.onFatal?.('Wrong claim');
    hub.add({ ...server('a'), claim: 'correct' });
    expect(connections).toHaveLength(3);
    expect(connections[2].opts.claim).toBe('correct');
    expect(connections[1].closed).toBe(0);
    hub.add({ ...server('a'), claim: 'correct' });
    expect(connections).toHaveLength(3);
    hub.pause('a'); hub.add({ ...server('a'), claim: 'new' });
    expect(connections).toHaveLength(3);
    expect(get(hub.servers)[0].state).toBe('paused');
    hub.resume('a'); expect(connections[3].opts.claim).toBe('new');
  });
  it('offers every connected server for a new session and preselects the asked-for one, else the first', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b'), server('c')]);
    connections.forEach(c => c.ready()); hub.pause('a');
    const servers = get(hub.servers);
    expect(newSessionTargets(servers).servers.map(s => s.id)).toEqual(['b', 'c']);
    expect(newSessionTargets(servers).serverId).toBe('b');
    expect(newSessionTargets(servers, 'c').serverId).toBe('c');
    expect(newSessionTargets(servers, 'a').serverId).toBe('b'); // paused: not a target
    connections[1].opts.onState('unreachable'); connections[2].opts.onState('unreachable');
    expect(newSessionTargets(get(hub.servers))).toEqual({ servers: [], serverId: '' });
  });
  it('requires a choice among connected peers when the selected server is unavailable', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b'), server('c')]);
    connections.forEach(c => c.ready()); hub.pause('a');
    const servers = get(hub.servers);
    expect(sessionDestinations(servers, servers[0]).map(s => s.id)).toEqual(['b', 'c']);
    expect(sessionDestinations(servers, servers[1]).map(s => s.id)).toEqual(['b']);
    connections[1].opts.onState('unreachable');
    expect(sessionDestinations(get(hub.servers), servers[0]).map(s => s.id)).toEqual(['c']);
  });
  it('pauses only that server, hides its sessions, and resumes with a fresh connection', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b')]);
    connections[0].emit({ type: 'sessions', sessions: [{ id: 'x', name: 'x', agent: null }] });
    hub.pause('a');
    expect(connections.map(c => c.closed)).toEqual([1, 0]);
    expect(get(hub.servers)[0].state).toBe('paused');
    expect(get(hub.allSessions)).toEqual([]);
    hub.retry('a'); expect(connections).toHaveLength(2); // Retry does not override Pause
    hub.resume('a'); expect(connections).toHaveLength(3);
  });
  it('reports a fatal connection error to the app', () => {
    const { hub, connections, fatal } = setup(); hub.start([server('a')]);
    connections[0].opts.onFatal?.('Not the owner');
    expect(fatal).toEqual(['a: Not the owner']);
    expect(get(hub.servers)[0]).toMatchObject({ state: 'closed', reason: 'Not the owner' });
  });
  it('a drop leaves other connections and the dropped server’s last sessions intact, with a reason and Retry', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b')]); connections.forEach(c => c.ready());
    connections[0].emit({ type: 'sessions', sessions: [{ id: 'x', name: 'x', agent: null }] });
    connections[0].opts.onDrop?.('no reply', 1000); connections[0].opts.onState('reconnecting');
    expect(get(hub.servers)[0]).toMatchObject({ state: 'reconnecting', reason: 'no reply' });
    expect(get(hub.servers)[1].state).toBe('connected'); expect(get(hub.allSessions)).toHaveLength(1);
    hub.retry('a'); expect(connections[1].closed).toBe(0); expect(connections[2].started).toBe(1);
    connections[0].opts.onState('unreachable', 'late callback');
    expect(get(hub.servers)[0].state).toBe('connecting');
  });
  it('warns above six connected servers without dropping any, including a late addition', () => {
    const { hub, connections } = setup(); hub.start(Array.from({ length: 7 }, (_, i) => server(String(i))));
    connections.forEach(c => c.ready());
    expect(get(hub.connectedCount)).toBe(7); expect(get(hub.overLimit)).toBe(true);
    hub.add(server('8')); connections[7].ready();
    expect(get(hub.connectedCount)).toBe(8); expect(connections.every(c => !c.closed)).toBe(true);
    hub.pause('8'); hub.pause('6'); expect(get(hub.overLimit)).toBe(false);
  });
  it('isolates session ids, agents, sync cursors and replies across servers', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b')]); connections.forEach(c => c.ready());
    for (const [i, c] of connections.entries()) {
      c.emit({ type: 'sessions', sessions: [{ id: 'tmux:one', name: 'one', agent: null }] });
      c.emit({ type: 'agent_snapshot', states: [{ session: 'tmux:one', state: i ? 'working' : 'needs_input', seq: i + 1, stale: false, ts: 1000 }] });
    }
    expect(get(hub.allSessions).map(s => s.key)).toEqual(['a/tmux:one', 'b/tmux:one']);
    expect(get(hub.agents)['a/tmux:one'].state).toBe('needs_input');
    expect(get(hub.agents)['b/tmux:one'].state).toBe('working');
    connections[0].ready(true);
    expect(connections[0].sent.at(-1)).toEqual({ type: 'agent_sync', after: { 'tmux:one': 1 } });
    hub.sendToSession('b/tmux:one', { type: 'reply', session: 'b/tmux:one', text: 'continue' });
    expect(connections[1].sent.at(-1)).toEqual({ type: 'reply', session: 'tmux:one', text: 'continue' });
    expect(connections[0].sent.some(m => m.type === 'reply')).toBe(false);
  });
  it('removes only the chosen server and its agent data, and closes every connection on shutdown', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b')]);
    hub.remove('a'); expect(get(hub.servers).map(s => s.id)).toEqual(['b']);
    hub.close(); expect(connections.map(c => c.closed)).toEqual([1, 1]); expect(get(hub.agents)).toEqual({});
  });
  it('uses the URL as a stable identity for manually saved servers', () => {
    expect(serverKey({ ...server('a'), server_id: undefined, server_url: 'wss://host/ws' })).toBe('wss://host/ws');
  });
  it('marks readiness after Retry and Resume as a reconnect so the active session can reattach', () => {
    const { hub, connections, ready } = setup(); hub.start([server('a')]); connections[0].ready();
    hub.retry('a'); connections[1].ready();
    hub.pause('a'); hub.resume('a'); connections[2].ready();
    expect(ready).toEqual([false, true, true]);
  });
  it('tries a saved address before refreshing the directory, then refreshes only after a failure', async () => {
    const refreshes: boolean[] = [];
    const { hub, connections } = setup(async (_s, refresh) => { refreshes.push(refresh); return {}; });
    hub.start([{ ...server('a'), record: { ip: '127.0.0.1', udp_port: 1, fingerprint: 'test' } as ServerListItem['record'] }]);
    await connections[0].opts.target();
    connections[0].opts.onState('unreachable', 'offline'); await connections[0].opts.target();
    connections[0].ready(); await connections[0].opts.target();
    expect(refreshes).toEqual([false, true, false]);
  });
  it('keeps each server\'s window lists under global keys, forgets closed sessions, and reports tmux control mode', () => {
    const { hub, connections } = setup();
    hub.start([server('a'), server('b')]);
    connections[0].opts.onState('connected');
    connections[0].opts.onReady({ type: 'auth_ok', user: 'u', server_name: 'a', backends: ['tmux'], control_mode: false, tmux_version: '3.0a', prefix: 'C-a' }, false);
    connections[1].ready();
    const win = { index: 0, name: 'w', command: 'claude', path: '/srv/app', active: true, agent: 'claude' };
    connections[0].emit({ type: 'sessions', sessions: [{ id: 's', name: 's', agent: null }, { id: 't', name: 't', agent: null }] });
    connections[0].emit({ type: 'window_list', session: 's', windows: [win], representative: 0 });
    connections[1].emit({ type: 'window_list', session: 's', windows: [{ ...win, path: '/other' }], representative: null });
    const windows = get(hub.windows);
    expect(windows['a/s'].windows[0].path).toBe('/srv/app');
    expect(windows['b/s'].windows[0].path).toBe('/other');
    const a = get(hub.servers)[0];
    expect([a.controlMode, a.tmuxVersion, a.prefix]).toEqual([false, '3.0a', 'C-a']);
    connections[0].emit({ type: 'sessions', sessions: [{ id: 't', name: 't', agent: null }] });
    expect(Object.keys(get(hub.windows))).toEqual(['b/s']);
  });
  it('sends peek, windows and close messages with the server-local session id', () => {
    const { hub, connections } = setup();
    hub.start([server('a')]);
    connections[0].ready();
    connections[0].emit({ type: 'sessions', sessions: [{ id: 's', name: 's', agent: null }] });
    hub.sendToSession('a/s', { type: 'peek', session: 'a/s' });
    hub.sendToSession('a/s', { type: 'close_window', session: 'a/s', index: 2 });
    hub.sendToSession('a/s', { type: 'close_session', session: 'a/s' });
    expect(connections[0].sent.slice(-3)).toEqual([
      { type: 'peek', session: 's' }, { type: 'close_window', session: 's', index: 2 }, { type: 'close_session', session: 's' },
    ]);
  });
});

describe('server health (U14)', () => {
  it('keeps the verdict the server sent, publishes it once, and forgets it when the link drops', () => {
    const { hub, connections } = setup();
    hub.start([server('a')]);
    connections[0].ready();
    let published = 0;
    const off = hub.servers.subscribe(() => { published++; });
    published = 0;
    connections[0].emit({ type: 'server_health', ts: 5, healthy: false, unhealthy: ['ram'], scope: 'container', ram_percent: 93 });
    expect(published).toBe(1);
    expect(get(hub.servers)[0].health).toEqual({ ts: 5, healthy: false, unhealthy: ['ram'], scope: 'container', ram_percent: 93 });
    connections[0].opts.onState('reconnecting');
    expect(get(hub.servers)[0].health).toBeUndefined();
    off();
  });
  it('shows no health for a server that sends none (an older server)', () => {
    const { hub, connections } = setup();
    hub.start([server('a')]);
    connections[0].ready();
    expect(get(hub.servers)[0].health).toBeUndefined();
  });
  it('reports the round trip of the connection, or null before one', () => {
    const { hub, connections } = setup();
    hub.start([server('a')]);
    expect(hub.rtt('a')).toBeNull();
    (connections[0] as unknown as { rtt: number }).rtt = 37;
    expect(hub.rtt('a')).toBe(37);
    expect(hub.rtt('missing')).toBeNull();
  });
});
