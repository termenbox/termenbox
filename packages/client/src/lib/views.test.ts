import { describe, expect, it } from 'vitest';
import { adaptiveDefault, applyView, loadView, presetView, saveView, withServerStatus } from './views';
import { applyEvents, applySnapshot, type AgentMap, type SessionAgent } from './agents';
import type { HubSession } from './hub';

const session = (serverId: string, id: string): HubSession => ({ id, name: id, agent: null, serverId, serverName: `Host ${serverId}`, key: `${serverId}/${id}` });
const agent = (state: SessionAgent['state'], ts: number, extra: Partial<SessionAgent> = {}): SessionAgent => ({ state, ts, seq: 1, stale: false, ...extra });
const sessions = [session('a', 'newer'), session('b', 'oldest'), session('a', 'error'), session('b', 'working'), session('a', 'idle'), session('b', 'shell')];
const agents: AgentMap = {
  'a/newer': agent('needs_input', 5000, { agent: 'claude', lastResponseAt: 4000 }),
  'b/oldest': agent('needs_input', 1000, { agent: 'codex', lastResponseAt: 2000 }),
  'a/error': agent('error', 3000, { agent: 'claude', lastResponseAt: 6000 }),
  'b/working': agent('working', 8000, { agent: 'codex', lastResponseAt: 7000 }),
  'a/idle': agent('idle', 2000, { agent: 'claude', lastResponseAt: 1000 }),
  'b/shell': agent('idle', 2000, { agent: 'shell' }),
};
const keys = (view: ReturnType<typeof presetView>, map = agents) => applyView(sessions, map, view, 10000).flatMap(g => g.sessions.map(s => s.key));

describe('session view presets', () => {
  it('Needs me includes errors and orders by longest wait across servers', () => {
    expect(keys(presetView('needs'))).toEqual(['b/oldest', 'a/error', 'a/newer']);
  });
  it('keeps the longest wait first through activity and a refreshed snapshot', () => {
    let map = applyEvents({}, [
      { session: 'b/oldest', seq: 1, ts: 1000, source: 'hook', confidence: 'high', kind: 'state', state: 'needs_input' },
      { session: 'a/newer', seq: 1, ts: 2000, source: 'hook', confidence: 'high', kind: 'state', state: 'needs_input' },
      { session: 'b/oldest', seq: 2, ts: 3000, source: 'hook', confidence: 'high', kind: 'message', summary: 'still waiting' },
    ]).map;
    expect(keys(presetView('needs'), map)).toEqual(['b/oldest', 'a/newer']);
    map = applySnapshot(map, [
      { session: 'b/oldest', seq: 2, ts: 4000, state: 'needs_input', stale: false },
      { session: 'a/newer', seq: 1, ts: 2000, state: 'needs_input', stale: false },
    ]);
    expect(keys(presetView('needs'), map)).toEqual(['b/oldest', 'a/newer']);
    const cold = applySnapshot({}, [
      { session: 'b/oldest', seq: 2, ts: 4000, state: 'needs_input', stale: false, waiting_since: 1000 },
      { session: 'a/newer', seq: 1, ts: 2000, state: 'needs_input', stale: false, waiting_since: 2000 },
    ]);
    expect(keys(presetView('needs'), cold)).toEqual(['b/oldest', 'a/newer']);
  });
  it('Working includes only working sessions', () => {
    expect(keys(presetView('working'))).toEqual(['b/working']);
  });
  it('Recent uses last_response_at, with sessions that have never answered last', () => {
    expect(keys(presetView('recent'))).toEqual(['b/working', 'a/error', 'a/newer', 'b/oldest', 'a/idle', 'b/shell']);
  });
  it('Cache expiring includes idle agents, excludes shells and unknown/expired caches, sorts by remaining time', () => {
    const extra = [...sessions, session('b', 'idle'), session('a', 'expired')];
    const map = { ...agents, 'b/idle': agent('done', 0, { agent: 'codex', lastResponseAt: 2000 }), 'a/expired': agent('idle', 0, { agent: 'claude', lastResponseAt: 1 }) };
    const groups = applyView(extra, map, presetView('cache'), 10000, { remaining: (_a, key) => ({ 'a/idle': 200000, 'b/idle': 1000, 'a/expired': 0 }[key] ?? null) });
    expect(groups.flatMap(g => g.sessions.map(s => s.key))).toEqual(['b/idle', 'a/idle']);
  });
  it('the default cache estimate uses the device TTL and excludes agents without a last response', () => {
    expect(keys(presetView('cache'))).toEqual(['a/idle']);
    expect(applyView(sessions, agents, presetView('cache'), 400000)).toEqual([]);
    expect(applyView(sessions, agents, presetView('cache'), 400000, { ttlMs: 3600000 })[0].sessions[0].key).toBe('a/idle');
  });
  it('By server groups every session with a stable server id and readable name', () => {
    const groups = applyView(sessions, agents, presetView('server'), 10000);
    expect(groups.map(g => [g.key, g.label, g.serverId])).toEqual([['server:b', 'Host b', 'b'], ['server:a', 'Host a', 'a']]);
    expect(groups.flatMap(g => g.sessions)).toHaveLength(6);
  });
  it('a server group header carries that server’s state and reason for Retry', () => {
    const groups = applyView(sessions, agents, presetView('server'), 10000);
    const status = { id: 'b', server: { server_id: 'b', server_name: 'Host b', server_url: '', registered_at: 0 }, state: 'unreachable' as const, reason: 'No route', version: '', backends: [], acpAgents: [], controlMode: false, tmuxVersion: '', prefix: '' };
    expect(withServerStatus(groups, [status])[0].server).toEqual(status);
    expect(withServerStatus(groups, [status])[1].server).toBeUndefined();
  });
});

describe('free controls and adaptive default', () => {
  it('groups by agent, status or none, without changing session identities', () => {
    const view = presetView('server');
    const byAgent = applyView(sessions, agents, { ...view, group: 'agent', sort: 'name' }, 10000);
    expect(byAgent.map(g => g.label)).toEqual(['claude', 'codex', 'shell']);
    const byStatus = applyView(sessions, agents, { ...view, group: 'status', sort: 'name' }, 10000);
    expect(byStatus.map(g => g.label)).toEqual(['Error', 'Idle', 'Needs you', 'Working']);
    expect(applyView(sessions, agents, { ...view, group: 'none' }, 10000)).toHaveLength(1);
  });
  it('combines status, agent, server and text filters', () => {
    const view = { ...presetView('server'), filter: { statuses: ['needs_input' as const], agents: ['claude'], servers: ['a'], query: 'new' } };
    expect(keys(view)).toEqual(['a/newer']);
    expect(keys({ ...view, filter: { ...view.filter, servers: ['b'] } })).toEqual([]);
  });
  it('sorts by name with a deterministic global-key tie break', () => {
    const same = [session('b', 'same'), session('a', 'same')];
    const sorted = applyView(same, {}, { ...presetView('recent'), sort: 'name' }, 0)[0].sessions;
    expect(sorted.map(s => s.key)).toEqual(['a/same', 'b/same']);
  });
  it('sorts sessions without a response time or cache by name instead of failing on infinities', () => {
    const none = [session('a', 'b'), session('a', 'a'), session('a', 'c')];
    for (const sort of ['response', 'cache'] as const) {
      expect(applyView(none, {}, { ...presetView('recent'), sort }, 0)[0].sessions.map(s => s.id)).toEqual(['a', 'b', 'c']);
    }
  });
  it('opens Needs me only for sessions that need a human, otherwise By server', () => {
    expect(adaptiveDefault(agents).preset).toBe('needs');
    expect(adaptiveDefault({ 'a': agent('working', 0) }).preset).toBe('server');
    expect(adaptiveDefault({ 'a': agent('error', 0) }).preset).toBe('needs');
    expect(adaptiveDefault({}).preset).toBe('server');
  });
  it('never mutates source sessions, agent maps or view settings', () => {
    const view = presetView('server'); const before = JSON.stringify({ sessions, agents, view });
    applyView(sessions, agents, view, 10000);
    expect(JSON.stringify({ sessions, agents, view })).toBe(before);
  });
});

describe('device view persistence', () => {
  const storage = () => {
    const values = new Map<string, string>();
    return { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v); } };
  };
  it('remembers preset and customized grouping, sorting and filters on this device', () => {
    const store = storage();
    const view = { ...presetView('recent'), group: 'agent' as const, filter: { statuses: [], agents: ['claude'], servers: ['a'], query: 'test' } };
    saveView(view, store); expect(loadView(store)).toEqual(view);
    expect(loadView(storage())).toBeNull();
  });
  it('defaults adaptively after corrupt or unsupported persisted data', () => {
    expect(loadView({ getItem: () => '{' })).toBeNull();
    expect(loadView({ getItem: () => '{"group":"bogus","sort":"name"}' })).toBeNull();
    expect(loadView({ getItem: () => { throw new Error('blocked'); } })).toBeNull();
    expect(() => saveView(presetView('needs'), { setItem: () => { throw new Error('blocked'); } })).not.toThrow();
  });
});
