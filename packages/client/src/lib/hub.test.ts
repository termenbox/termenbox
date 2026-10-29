import { describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { ServerHub, newWorkspaceTargets, serverKey, workspaceDestinations, type HubConnection } from './hub';
import type { ConnectionOptions } from './connection';
import type { ClientMessage, ServerListItem, ServerMessage } from './protocol';
import type { Target } from './transport';
import { moveWorkspace } from './order';

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
  const messages: ServerMessage[] = [];
  const hub = new ServerHub({ target, onMessage: (_id, msg) => messages.push(msg), onFatal: (id, reason) => fatal.push(`${id}: ${reason}`), credentials: async () => ({ cert: 'c', sign: async () => 's' }), onReady: (_id, _msg, reconnect) => ready.push(reconnect), factory: (opts) => {
    const c = new Fake(opts); connections.push(c); return c;
  } });
  return { hub, connections, ready, fatal, messages };
}

describe('ServerHub', () => {
  const auth = (version: string, available = '0.1.54'): Extract<ServerMessage, { type: 'auth_ok' }> => ({ type: 'auth_ok', user: 'u', server_name: 'host', version, update_available: available });

  it('sends one update, ignores unrelated errors and waits across reconnects with the same or an older release', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b')]);
    const c = connections[0]; c.opts.onState('connected'); c.opts.onReady(auth('0.1.53+abc'), false);
    hub.updateServer('a'); hub.updateServer('a'); hub.updateServer('b');
    expect(c.sent.filter(m => m.type === 'update_server')).toHaveLength(1);
    expect(connections[1].sent.filter(m => m.type === 'update_server')).toHaveLength(0);
    c.emit({ type: 'error', message: 'Unknown directory' });
    c.opts.onState('reconnecting');
    c.opts.onState('connected'); c.opts.onReady(auth('0.1.53+different-build'), true);
    expect(get(hub.servers)[0].updating).toBe(true);
    c.opts.onReady(auth('0.1.52'), true);
    expect(get(hub.servers)[0].updating).toBe(true);
    c.emit({ type: 'error', message: 'Server update: The update was rolled back.' });
    expect(get(hub.servers)[0].updating).toBe(false);
    hub.close();
  });

  it.each([
    ['0.1.54-beta.9', '0.1.54-beta.10'],
    ['0.1.54-beta.10', '0.1.54'],
    ['0.1.54-nightly.20261029', '0.1.54-nightly.20261030'],
  ])('recognizes a newer release from %s to %s', (from, to) => {
    const { hub, connections } = setup(); hub.start([server('a')]);
    const c = connections[0]; c.opts.onState('connected'); c.opts.onReady(auth(from, to), false);
    hub.updateServer('a'); c.opts.onReady(auth(to, ''), true);
    expect(get(hub.servers)[0].updating).toBe(false);
    hub.close();
  });

  it('expires an unconfirmed update even while disconnected and never lets its timer end a later attempt', () => {
    vi.useFakeTimers();
    const { hub, connections, messages } = setup();
    try {
      hub.start([server('a')]); const c = connections[0];
      c.opts.onState('connected'); c.opts.onReady(auth('0.1.53'), false);
      hub.updateServer('a'); vi.advanceTimersByTime(590_000);
      c.emit({ type: 'error', message: 'Server update: failed' });
      hub.updateServer('a'); vi.advanceTimersByTime(10_000);
      expect(get(hub.servers)[0].updating).toBe(true);
      c.opts.onState('unreachable'); vi.advanceTimersByTime(590_000);
      expect(get(hub.servers)[0].updating).toBe(false);
      expect(messages).toContainEqual(expect.objectContaining({ type: 'error', message: expect.stringContaining('No newer running version') }));
      c.opts.onState('connected'); hub.updateServer('a'); hub.remove('a');
      const n = messages.length; vi.advanceTimersByTime(600_000);
      expect(messages).toHaveLength(n);
    } finally { hub.close(); vi.useRealTimers(); }
  });

  it('does not update without a notice and cancels an attempt when the user pauses the server', () => {
    const { hub, connections } = setup(); hub.start([server('a')]);
    const c = connections[0]; c.ready(); hub.updateServer('a');
    expect(c.sent.some(m => m.type === 'update_server')).toBe(false);
    c.opts.onReady(auth('0.1.53'), false); hub.updateServer('a'); hub.pause('a');
    expect(get(hub.servers)[0]).toMatchObject({ state: 'paused', updating: false });
    hub.close();
  });
  it('reports a newer release and asks the server to install it, until it comes back', () => {
    const { hub, connections } = setup(); hub.start([server('a')]);
    const c = connections[0];
    c.opts.onState('connected');
    c.opts.onReady({ type: 'auth_ok', user: 'u', server_name: 'host', version: '0.1.53+abc', update_available: '0.1.54' }, false);
    expect(get(hub.servers)[0]).toMatchObject({ version: '0.1.53+abc', updateAvailable: '0.1.54', updating: false });
    hub.updateServer('a');
    expect(c.sent).toContainEqual({ type: 'update_server' });
    expect(get(hub.servers)[0].updating).toBe(true);
    c.opts.onReady({ type: 'auth_ok', user: 'u', server_name: 'host', version: '0.1.54+def' }, true);
    expect(get(hub.servers)[0]).toMatchObject({ version: '0.1.54+def', updateAvailable: '', updating: false });
    hub.updateServer('a'); c.emit({ type: 'error', message: 'Only the owner can update this server' });
    expect(get(hub.servers)[0].updating).toBe(false);
  });
  it('learns of a newly recorded or cleared release without reconnecting', () => {
    const { hub, connections } = setup(); hub.start([server('a')]);
    const c = connections[0];
    c.opts.onState('connected');
    c.opts.onReady({ type: 'auth_ok', user: 'u', server_name: 'host', version: '0.1.53+abc' }, false);
    expect(get(hub.servers)[0].updateAvailable).toBe('');
    c.emit({ type: 'update_status', available: '0.1.54' });
    expect(get(hub.servers)[0]).toMatchObject({ updateAvailable: '0.1.54', version: '0.1.53+abc' });
    c.emit({ type: 'update_status', available: '' });
    expect(get(hub.servers)[0].updateAvailable).toBe('');
  });
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
  it('offers every connected server for a new workspace and preselects the asked-for one, else the first', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b'), server('c')]);
    connections.forEach(c => c.ready()); hub.pause('a');
    const servers = get(hub.servers);
    expect(newWorkspaceTargets(servers).servers.map(s => s.id)).toEqual(['b', 'c']);
    expect(newWorkspaceTargets(servers).serverId).toBe('b');
    expect(newWorkspaceTargets(servers, 'c').serverId).toBe('c');
    expect(newWorkspaceTargets(servers, 'a').serverId).toBe('b'); // paused: not a target
    connections[1].opts.onState('unreachable'); connections[2].opts.onState('unreachable');
    expect(newWorkspaceTargets(get(hub.servers))).toEqual({ servers: [], serverId: '' });
  });
  it('requires a choice among connected peers when the selected server is unavailable', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b'), server('c')]);
    connections.forEach(c => c.ready()); hub.pause('a');
    const servers = get(hub.servers);
    expect(workspaceDestinations(servers, servers[0]).map(s => s.id)).toEqual(['b', 'c']);
    expect(workspaceDestinations(servers, servers[1]).map(s => s.id)).toEqual(['b']);
    connections[1].opts.onState('unreachable');
    expect(workspaceDestinations(get(hub.servers), servers[0]).map(s => s.id)).toEqual(['c']);
  });
  it('pauses only that server, hides its workspaces, and resumes with a fresh connection', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b')]);
    connections[0].emit({ type: 'workspaces', workspaces: [{ id: 'x', name: 'x', agent: null }] });
    hub.pause('a');
    expect(connections.map(c => c.closed)).toEqual([1, 0]);
    expect(get(hub.servers)[0].state).toBe('paused');
    expect(get(hub.allWorkspaces)).toEqual([]);
    hub.retry('a'); expect(connections).toHaveLength(2); // Retry does not override Pause
    hub.resume('a'); expect(connections).toHaveLength(3);
  });
  it('reports a fatal connection error to the app', () => {
    const { hub, connections, fatal } = setup(); hub.start([server('a')]);
    connections[0].opts.onFatal?.('Not the owner');
    expect(fatal).toEqual(['a: Not the owner']);
    expect(get(hub.servers)[0]).toMatchObject({ state: 'closed', reason: 'Not the owner' });
  });
  it('a drop leaves other connections and the dropped server’s last workspaces intact, with a reason and Retry', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b')]); connections.forEach(c => c.ready());
    connections[0].emit({ type: 'workspaces', workspaces: [{ id: 'x', name: 'x', agent: null }] });
    connections[0].opts.onDrop?.('no reply', 1000); connections[0].opts.onState('reconnecting');
    expect(get(hub.servers)[0]).toMatchObject({ state: 'reconnecting', reason: 'no reply' });
    expect(get(hub.servers)[1].state).toBe('connected'); expect(get(hub.allWorkspaces)).toHaveLength(1);
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
  it('isolates workspace ids, agents, sync cursors and replies across servers', () => {
    const { hub, connections } = setup(); hub.start([server('a'), server('b')]); connections.forEach(c => c.ready());
    for (const [i, c] of connections.entries()) {
      c.emit({ type: 'workspaces', workspaces: [{ id: 'tmux:one', name: 'one', agent: null }] });
      c.emit({ type: 'agent_snapshot', states: [{ workspace: 'tmux:one', state: i ? 'working' : 'needs_input', seq: i + 1, stale: false, ts: 1000 }] });
    }
    expect(get(hub.allWorkspaces).map(s => s.key)).toEqual(['a/tmux:one', 'b/tmux:one']);
    expect(get(hub.agents)['a/tmux:one'].state).toBe('needs_input');
    expect(get(hub.agents)['b/tmux:one'].state).toBe('working');
    connections[0].ready(true);
    expect(connections[0].sent.at(-1)).toEqual({ type: 'agent_sync', after: { 'tmux:one': 1 } });
    hub.sendToWorkspace('b/tmux:one', { type: 'reply', workspace: 'b/tmux:one', text: 'continue' });
    expect(connections[1].sent.at(-1)).toEqual({ type: 'reply', workspace: 'tmux:one', text: 'continue' });
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
  it('marks readiness after Retry and Resume as a reconnect so the active workspace can reattach', () => {
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
  it('keeps each server\'s window lists under global keys, forgets closed workspaces, and reports tmux control mode', () => {
    const { hub, connections } = setup();
    hub.start([server('a'), server('b')]);
    connections[0].opts.onState('connected');
    connections[0].opts.onReady({ type: 'auth_ok', user: 'u', server_name: 'a', backends: ['tmux'], control_mode: false, tmux_version: '3.0a', prefix: 'C-a' }, false);
    connections[1].ready();
    const win = { index: 0, name: 'w', command: 'claude', path: '/srv/app', active: true, agent: 'claude' };
    connections[0].emit({ type: 'workspaces', workspaces: [{ id: 's', name: 's', agent: null }, { id: 't', name: 't', agent: null }] });
    connections[0].emit({ type: 'window_list', workspace: 's', windows: [win], representative: 0 });
    connections[1].emit({ type: 'window_list', workspace: 's', windows: [{ ...win, path: '/other' }], representative: null });
    const windows = get(hub.windows);
    expect(windows['a/s'].windows[0].path).toBe('/srv/app');
    expect(windows['b/s'].windows[0].path).toBe('/other');
    const a = get(hub.servers)[0];
    expect([a.controlMode, a.tmuxVersion, a.prefix]).toEqual([false, '3.0a', 'C-a']);
    connections[0].emit({ type: 'workspaces', workspaces: [{ id: 't', name: 't', agent: null }] });
    expect(Object.keys(get(hub.windows))).toEqual(['b/s']);
  });
  it('sends peek, windows and close messages with the server-local workspace id', () => {
    const { hub, connections } = setup();
    hub.start([server('a')]);
    connections[0].ready();
    connections[0].emit({ type: 'workspaces', workspaces: [{ id: 's', name: 's', agent: null }] });
    hub.sendToWorkspace('a/s', { type: 'peek', workspace: 'a/s' });
    hub.sendToWorkspace('a/s', { type: 'close_window', workspace: 'a/s', index: 2 });
    hub.sendToWorkspace('a/s', { type: 'close_workspace', workspace: 'a/s' });
    expect(connections[0].sent.slice(-3)).toEqual([
      { type: 'peek', workspace: 's' }, { type: 'close_window', workspace: 's', index: 2 }, { type: 'close_workspace', workspace: 's' },
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
  it('does not rebuild the workspace, agent and window lists for a health sample', () => {
    const { hub, connections } = setup();
    hub.start([server('a')]);
    connections[0].ready();
    let workspaces = 0;
    let agents = 0;
    const offS = hub.allWorkspaces.subscribe(() => { workspaces++; });
    const offA = hub.agents.subscribe(() => { agents++; });
    workspaces = 0; agents = 0;
    connections[0].emit({ type: 'server_health', ts: 5, healthy: true, unhealthy: [], scope: 'host', ram_percent: 20 });
    expect([workspaces, agents]).toEqual([0, 0]);
    offS(); offA();
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

describe('ServerHub candidate bundles (decision 0123)', () => {
  it('keeps a newer verified bundle and ignores a stale one, one for another key, and one that does not verify', async () => {
    const { newServerKey, bundleOf } = await import('./bundleFixture');
    const key = await newServerKey();
    const other = await newServerKey();
    const lan = [{ kind: 'lan' as const, addr: '10.0.0.2', port: 4443 }];
    const saved: ServerListItem[] = [];
    const connections: Fake[] = [];
    const hub = new ServerHub({ credentials: async () => ({ cert: 'c', sign: async () => 's' }), onSaved: s => saved.push(s), factory: opts => { const c = new Fake(opts); connections.push(c); return c; } });
    hub.start([{ ...server(key.serverId), bundle_seq: 5 }]);
    const c = connections[0];
    const settle = () => new Promise(r => setTimeout(r, 20));

    c.emit({ type: 'candidates', bundle: await key.sign(bundleOf(5, lan)) }); await settle();
    c.emit({ type: 'candidates', bundle: await other.sign(bundleOf(9, lan)) }); await settle();
    const good = await key.sign(bundleOf(6, lan));
    c.emit({ type: 'candidates', bundle: good.slice(0, -3) + 'AAA' }); await settle();
    expect(saved).toHaveLength(0);

    c.emit({ type: 'candidates', bundle: good }); await settle();
    expect(saved).toHaveLength(1);
    expect(saved[0]).toMatchObject({ bundle: good, bundle_seq: 6 });
    c.emit({ type: 'candidates', bundle: good }); await settle();
    expect(saved).toHaveLength(1); // the same seq again is stale
  });

  it('follows the published flag of the newest verified bundle', async () => {
    const { newServerKey, bundleOf } = await import('./bundleFixture');
    const key = await newServerKey();
    const lan = [{ kind: 'lan' as const, addr: '10.0.0.2', port: 4443 }];
    const saved: ServerListItem[] = [];
    const connections: Fake[] = [];
    const hub = new ServerHub({ credentials: async () => ({ cert: 'c', sign: async () => 's' }), onSaved: s => saved.push(s), factory: opts => { const c = new Fake(opts); connections.push(c); return c; } });
    hub.start([server(key.serverId)]);
    const settle = () => new Promise(r => setTimeout(r, 20));
    connections[0].emit({ type: 'candidates', bundle: await key.sign(bundleOf(1, lan, { published: true })) }); await settle();
    expect(saved.at(-1)?.published).toBe(true);
    connections[0].emit({ type: 'candidates', bundle: await key.sign(bundleOf(2, lan, { published: false })) }); await settle();
    expect(saved.at(-1)?.published).toBe(false);
  });

  it('takes the same bundle again, as on every reconnect, as a quiet no-op, and still warns about a real rejection', async () => {
    const { newServerKey, bundleOf } = await import('./bundleFixture');
    const key = await newServerKey();
    const lan = [{ kind: 'lan' as const, addr: '10.0.0.2', port: 4443 }];
    const saved: ServerListItem[] = [];
    const connections: Fake[] = [];
    const hub = new ServerHub({ credentials: async () => ({ cert: 'c', sign: async () => 's' }), onSaved: s => saved.push(s), factory: opts => { const c = new Fake(opts); connections.push(c); return c; } });
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    try {
      hub.start([server(key.serverId)]);
      const c = connections[0];
      const settle = () => new Promise(r => setTimeout(r, 20));
      const same = await key.sign(bundleOf(4, lan));
      c.emit({ type: 'candidates', bundle: same }); await settle();
      c.emit({ type: 'candidates', bundle: same }); await settle();
      c.emit({ type: 'candidates', bundle: same }); await settle();
      expect(saved).toHaveLength(1);
      expect(warn).not.toHaveBeenCalled();
      c.emit({ type: 'candidates', bundle: await key.sign(bundleOf(3, lan)) }); await settle();
      expect(warn).toHaveBeenCalledTimes(1); // an older seq is a real rejection
    } finally { warn.mockRestore(); }
  });

  it('remembers the path that connected and shows the active path and every failure', () => {
    const saved: ServerListItem[] = [];
    const connections: Fake[] = [];
    const hub = new ServerHub({ credentials: async () => ({ cert: 'c', sign: async () => 's' }), onSaved: s => saved.push(s), factory: opts => { const c = new Fake(opts); connections.push(c); return c; } });
    hub.start([server('a')]);
    const failure = { path: { key: 'lan 10.0.0.2:4443', kind: 'lan' as const, label: 'Direct, LAN', addr: '10.0.0.2', port: 4443 }, reason: 'ICE failed', fix: 'Join the same Wi-Fi' };
    connections[0].opts.onFailures?.([failure]);
    expect(get(hub.servers)[0].failures).toEqual([failure]);
    connections[0].opts.onPath?.({ key: 'lan 10.0.0.2:4443', label: 'Direct, LAN' });
    connections[0].opts.onWinner?.('lan 10.0.0.2:4443');
    expect(get(hub.servers)[0].path).toBe('Direct, LAN');
    expect(saved.at(-1)?.winner).toBe('lan 10.0.0.2:4443');
  });
  it('shows a new name at once, sends it with the server-local id, and asks for the list again to settle it', () => {
    vi.useFakeTimers();
    const { hub, connections } = setup();
    hub.start([server('a')]);
    connections[0].ready();
    connections[0].emit({ type: 'workspaces', workspaces: [{ id: 'one', name: 'one', agent: null }] });
    hub.rename(serverKey(server('a')) + '/one', 'renamed');
    expect(get(hub.allWorkspaces).map(s => s.name)).toEqual(['renamed']);
    expect(connections[0].sent).toContainEqual({ type: 'rename_workspace', workspace: 'one', name: 'renamed' });
    vi.advanceTimersByTime(2100);
    expect(connections[0].sent.at(-1)).toEqual({ type: 'list_workspaces' });
    // The server's own list is the truth: a refusal brings the old name back.
    connections[0].emit({ type: 'workspaces', workspaces: [{ id: 'one', name: 'one', agent: null }] });
    expect(get(hub.allWorkspaces).map(s => s.name)).toEqual(['one']);
    vi.useRealTimers();
  });
  it('lists workspaces in the user\'s order, whatever the server sends, and appends a new one', () => {
    const { hub, connections } = setup();
    hub.start([server('ord')]);
    connections[0].ready();
    const ws = (...ids: string[]) => ({ type: 'workspaces' as const, workspaces: ids.map(id => ({ id, name: id, agent: null })) });
    connections[0].emit(ws('one', 'two', 'three'));
    expect(get(hub.allWorkspaces).map(s => s.name)).toEqual(['one', 'two', 'three']);
    expect(moveWorkspace(get(hub.allWorkspaces), serverKey(server('ord')) + '/three', 'top')).toEqual({ position: 1, count: 3 });
    expect(get(hub.allWorkspaces).map(s => s.name)).toEqual(['three', 'one', 'two']); // the store lists again at once
    connections[0].emit(ws('two', 'one', 'new', 'three'));
    expect(get(hub.allWorkspaces).map(s => s.name)).toEqual(['three', 'one', 'two', 'new']);
  });
});
