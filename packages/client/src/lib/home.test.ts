import { describe, expect, it } from 'vitest';
import { continueTarget, fleetCounts, heroChoices, queueOf, rowOf, sessionGroups, Settle, splitQueue } from './home';
import type { AgentMap, SessionAgent } from './agents';
import type { HubSession } from './hub';
import type { AttentionKind } from './protocol';

const sess = (name: string, serverId = 's1'): HubSession => ({ id: name, name, agent: null, serverId, key: `${serverId}/${name}`, serverName: serverId.toUpperCase() });
const withAtt = (kind: AttentionKind, at: number, extra: Partial<SessionAgent> = {}): SessionAgent =>
  ({ state: 'unknown', seq: 1, stale: false, ts: at, serverStatus: true, attention: { id: `a${at}`, session: 'x', kind, raised_at: at, task: { title: `task ${at}`, started_at: 0, source: 'hook' } }, ...extra });
const quiet = (activity: SessionAgent['activity'] = 'idle'): SessionAgent => ({ state: activity === 'working' ? 'working' : 'idle', seq: 1, stale: false, ts: 1, serverStatus: true, activity });

describe('queue order', () => {
  it('puts needs_input, then failed, then finished_unseen, oldest wait first inside a kind', () => {
    const ss = ['f-new', 'n-new', 'fin', 'f-old', 'n-old', 'calm'].map((n) => sess(n));
    const agents: AgentMap = {
      's1/f-new': withAtt('failed', 500), 's1/n-new': withAtt('needs_input', 400), 's1/fin': withAtt('finished_unseen', 10),
      's1/f-old': withAtt('failed', 100), 's1/n-old': withAtt('needs_input', 200), 's1/calm': quiet(),
    };
    expect(queueOf(ss, agents).map((r) => r.name)).toEqual(['n-old', 'n-new', 'f-old', 'f-new', 'fin']);
  });
  it('spans servers and ignores sessions with no open item or an unknown kind', () => {
    const ss = [sess('a', 's1'), sess('b', 's2'), sess('c', 's2')];
    const agents: AgentMap = { 's1/a': withAtt('needs_input', 300), 's2/b': withAtt('needs_input', 100), 's2/c': withAtt('unknown', 1) };
    expect(queueOf(ss, agents).map((r) => `${r.serverName}:${r.name}`)).toEqual(['S2:b', 'S1:a']);
  });
  it('reads an older server (no server status) from the raw state', () => {
    const ss = [sess('a'), sess('b'), sess('c')];
    const agents: AgentMap = {
      's1/a': { state: 'error', seq: 1, stale: false, ts: 5 }, 's1/b': { state: 'needs_input', seq: 1, stale: false, ts: 9, waitingSince: 7 },
      's1/c': { state: 'done', seq: 1, stale: false, ts: 1 },
    };
    expect(queueOf(ss, agents).map((r) => [r.name, r.kind])).toEqual([['b', 'needs_input'], ['a', 'failed']]);
  });
  it('makes the oldest needs_input the hero, and no hero when only failed or finished items wait', () => {
    const ss = [sess('a'), sess('b'), sess('c')];
    const q = splitQueue(queueOf(ss, { 's1/a': withAtt('needs_input', 9), 's1/b': withAtt('needs_input', 3), 's1/c': withAtt('failed', 1) }));
    expect(q.hero?.name).toBe('b');
    expect(q.rest.map((r) => r.name)).toEqual(['a', 'c']);
    const none = splitQueue(queueOf(ss, { 's1/c': withAtt('failed', 1) }));
    expect(none.hero).toBeUndefined();
    expect(none.rest).toHaveLength(1);
  });
  it('carries what a row shows: task title, server, wait start and the prompt it was read from', () => {
    const prompt = { id: 'p1', kind: 'choice', choices: ['Yes', 'No'], seq: 4, confidence: 'high' };
    const r = rowOf(sess('a'), withAtt('needs_input', 42, { prompt, agent: 'claude' }));
    expect(r).toMatchObject({ name: 'a', serverName: 'S1', title: 'task 42', since: 42, agent: 'claude', state: 'needs-you', prompt });
  });
});

describe('fleet counts', () => {
  it('counts the four states from the same items as the queue; idle sessions are not counted', () => {
    const ss = Array.from({ length: 20 }, (_, i) => sess(`s${i}`));
    const agents: AgentMap = {};
    ss.forEach((s, i) => { agents[s.key] = i < 3 ? withAtt('needs_input', i) : i < 5 ? withAtt('failed', i) : i < 9 ? withAtt('finished_unseen', i) : i < 14 ? quiet('working') : quiet(); });
    expect(fleetCounts(ss, agents)).toEqual({ 'needs-you': 3, failed: 2, finished: 4, working: 5 });
    expect(queueOf(ss, agents)).toHaveLength(9);
  });
});

describe('hero options', () => {
  const prompt = (confidence: string, n = 2) => ({ id: 'p', kind: 'choice', choices: ['Yes', 'No', 'Maybe', 'x'].slice(0, n), seq: 1, confidence });
  it('offers the confirmed choices only', () => {
    const a = withAtt('needs_input', 1, { prompt: prompt('high') });
    expect(heroChoices(rowOf(sess('a'), a), a, 10)).toEqual(['Yes', 'No']);
    const low = withAtt('needs_input', 1, { prompt: prompt('low') });
    expect(heroChoices(rowOf(sess('a'), low), low, 10)).toEqual([]);
    const many = withAtt('needs_input', 1, { prompt: prompt('high', 4) });
    expect(heroChoices(rowOf(sess('a'), many), many, 10)).toEqual([]);
  });
});

describe('the Sessions groups', () => {
  it('groups every session by state in a fixed order and drops empty groups', () => {
    const ss = ['i', 'w', 'n2', 'n1', 'f'].map((n) => sess(n));
    const agents: AgentMap = { 's1/i': quiet(), 's1/w': quiet('working'), 's1/n2': withAtt('needs_input', 9), 's1/n1': withAtt('needs_input', 2), 's1/f': withAtt('failed', 3) };
    const g = sessionGroups(ss, agents);
    expect(g.map((x) => x.title)).toEqual(['Needs you', 'Failed', 'Working', 'Idle']);
    expect(g[0].rows.map((r) => r.name)).toEqual(['n1', 'n2']);
  });
});

describe('Continue card', () => {
  const ss = [sess('a'), sess('b', 's2')];
  const connected = new Set(['s1']);
  it('shows the last session when nothing needs a decision', () => {
    expect(continueTarget('s1/a', ss, connected, [])?.name).toBe('a');
  });
  it('is hidden while a decision waits, though failed or finished items do not hide it', () => {
    const agents: AgentMap = { 's2/b': withAtt('needs_input', 1) };
    expect(continueTarget('s1/a', ss, connected, queueOf(ss, agents))).toBeNull();
    expect(continueTarget('s1/a', ss, connected, queueOf(ss, { 's2/b': withAtt('failed', 1) }))?.name).toBe('a');
  });
  it('is hidden without a last session, when it is gone, or when its server is not connected', () => {
    expect(continueTarget(null, ss, connected, [])).toBeNull();
    expect(continueTarget('s1/gone', ss, connected, [])).toBeNull();
    expect(continueTarget('s2/b', ss, connected, [])).toBeNull();
  });
});

describe('the settle rule', () => {
  it('takes the first value at once and follows the data when nothing is touched', () => {
    const s = new Settle<string[]>();
    expect(s.view(['a'], 0)).toEqual(['a']);
    expect(s.view(['b', 'a'], 1)).toEqual(['b', 'a']);
  });
  it('holds the queue while a touch is down, and applies the change on release', () => {
    const s = new Settle<string[]>();
    s.view(['a'], 0);
    s.touchDown(1);
    expect(s.view(['b', 'a'], 5000)).toEqual(['a']);
    s.touchUp(1);
    expect(s.view(['b', 'a'], 5001)).toEqual(['b', 'a']);
  });
  it('holds for 2 s after a scroll, not 1.9 and not 2.0', () => {
    const s = new Settle<string[]>();
    s.view(['a'], 0);
    s.scrolled(10_000);
    expect(s.view(['b'], 11_900)).toEqual(['a']);
    expect(s.view(['b'], 12_000)).toEqual(['b']);
  });
  it('a second scroll restarts the 2 s and two fingers need both lifted', () => {
    const s = new Settle<string[]>();
    s.view(['a'], 0);
    s.scrolled(1000); s.scrolled(2500);
    expect(s.view(['b'], 4000)).toEqual(['a']);
    s.touchDown(1); s.touchDown(2); s.touchUp(1);
    expect(s.view(['b'], 9000)).toEqual(['a']);
    s.touchUp(2);
    expect(s.view(['b'], 9000)).toEqual(['b']);
  });
});

describe('server health on rows (U14)', () => {
  it('flags the rows of servers whose own verdict is unhealthy, and no others', () => {
    const ss = [sess('a', 's1'), sess('b', 's2')];
    const agents: AgentMap = { 's1/a': withAtt('failed', 1), 's2/b': withAtt('failed', 2) };
    const q = queueOf(ss, agents, new Set(['s2']));
    expect(q.map((r) => [r.name, r.unhealthy])).toEqual([['a', undefined], ['b', true]]);
    expect(sessionGroups(ss, agents, new Set(['s1'])).flatMap((g) => g.rows).map((r) => [r.name, r.unhealthy])).toEqual([['a', true], ['b', undefined]]);
  });
  it('works for callers that know nothing of health', () => {
    const ss = [sess('a')];
    const agents: AgentMap = { 's1/a': withAtt('failed', 1) };
    expect(queueOf(ss, agents)[0].unhealthy).toBeUndefined();
    expect(sessionGroups(ss, agents)[0].rows[0].unhealthy).toBeUndefined();
    expect(rowOf(ss[0], agents['s1/a']).unhealthy).toBeUndefined();
  });
});
