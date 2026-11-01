import { describe, it, expect } from 'vitest';
import { SessionSearch, landingOf, resumeOutcome, cleanQuery, cutText, groupHits, loadGroupMode, saveGroupMode, parts, whenText, MAX_HITS_PER_SERVER, type SearchState } from './sessionSearch';
import type { ClientMessage, SearchHitMessage, ServerMessage } from './protocol';

function rig(servers = [{ id: 'a', name: 'Box' }]) {
  const sent: { to: string; msg: ClientMessage }[] = [];
  const states: SearchState[] = [];
  const s = new SessionSearch({ servers: () => servers, send: (to, msg) => sent.push({ to, msg }), onchange: (st) => states.push(st) });
  return { s, sent, states };
}
const hit = (id: number, over: Partial<SearchHitMessage> = {}): ServerMessage => ({ type: 'search_hit', id, workspace: 'w1', tab: 't1', agent: 'claude', up: true, turn: 'u1', kind: 'user', snippet: 'fix the parser bug', highlight: [8, 14], at: '2026-10-30T10:00:00Z', ...over });
const done = (id: number, over: Record<string, unknown> = {}): ServerMessage => ({ type: 'search_done', id, off: [], ...over }) as ServerMessage;

describe('typing', () => {
  it('asks every connected server and shows searching until each is done', () => {
    const { s, sent } = rig([{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }]);
    s.run('  parser ');
    expect(sent.map((x) => x.to)).toEqual(['a', 'b']);
    expect(sent[0].msg).toMatchObject({ type: 'search', query: 'parser' });
    const id = (sent[0].msg as { id: number }).id;
    s.handle('a', done(id));
    expect(s.current.status).toBe('searching');
    s.handle('b', done(id));
    expect(s.current.status).toBe('done');
  });
  it('goes idle on an empty query without asking anyone', () => {
    const { s, sent } = rig();
    s.run('   ');
    expect(sent).toEqual([]);
    expect(s.current.status).toBe('idle');
  });
  it('drops control characters and caps the length like the server', () => {
    expect(cleanQuery('a\u0000b\nc')).toBe('a b c');
    expect(new TextEncoder().encode(cleanQuery('é'.repeat(400))).length).toBeLessThanOrEqual(512);
  });
  it('is done at once when no server is connected', () => {
    const { s } = rig([]);
    s.run('x');
    expect(s.current.status).toBe('done');
    expect(s.current.hits).toEqual([]);
  });
});

describe('streaming hits', () => {
  it('fills while the scan runs, and keeps the server name', () => {
    const { s, sent } = rig();
    s.run('parser');
    const id = (sent[0].msg as { id: number }).id;
    s.handle('a', hit(id));
    expect(s.current.status).toBe('searching');
    expect(s.current.hits).toHaveLength(1);
    expect(s.current.hits[0]).toMatchObject({ serverName: 'Box', workspace: 'w1', up: true, at: Date.parse('2026-10-30T10:00:00Z') });
    s.handle('a', hit(id, { turn: 'u2' }));
    expect(s.current.hits).toHaveLength(2);
  });
  it('ignores the answers of an older search and of a server not asked', () => {
    const { s, sent } = rig();
    s.run('one');
    const first = (sent[0].msg as { id: number }).id;
    s.run('two');
    s.handle('a', hit(first));
    s.handle('zzz', hit(first + 1));
    expect(s.current.hits).toEqual([]);
    s.handle('a', done(first));
    expect(s.current.status).toBe('searching');
  });
  it('clamps a highlight that points outside the snippet', () => {
    const { s, sent } = rig();
    s.run('x');
    s.handle('a', hit((sent[0].msg as { id: number }).id, { highlight: [5, 999] }));
    expect(s.current.hits[0].highlight).toEqual([5, 18]);
    expect(parts(s.current.hits[0])).toEqual({ before: 'fix t', match: 'he parser bug', after: '' });
  });
  it('stops taking hits from one server at the sheet cap', () => {
    const { s, sent } = rig();
    s.run('x');
    const id = (sent[0].msg as { id: number }).id;
    for (let i = 0; i < MAX_HITS_PER_SERVER + 20; i++) s.handle('a', hit(id, { turn: `u${i}` }));
    expect(s.current.hits).toHaveLength(MAX_HITS_PER_SERVER);
  });
});

describe('cut, off and lost', () => {
  it('says a search was cut, once per reason, and nothing for a cancel', () => {
    const { s, sent } = rig([{ id: 'a', name: 'A' }, { id: 'b', name: 'B' }]);
    s.run('x');
    const id = (sent[0].msg as { id: number }).id;
    s.handle('a', done(id, { cut: 'time' }));
    s.handle('b', done(id, { cut: 'bytes' }));
    expect(s.current.cuts).toEqual([cutText('time')]);
    expect(cutText('cancelled')).toBeNull();
    expect(cutText('hits')).toMatch(/cut/);
  });
  it('keeps the server’s fixed reason for an agent whose search is off', () => {
    const { s, sent } = rig();
    s.run('x');
    s.handle('a', done((sent[0].msg as { id: number }).id, { off: [{ agent: 'claude', reason: 'unknown file version' }] }));
    expect(s.current.off).toEqual([{ agent: 'claude', reason: 'unknown file version' }]);
  });
  it('finishes when a pending server drops, and counts it', () => {
    const { s } = rig();
    s.run('x');
    s.lost('a');
    expect(s.current).toMatchObject({ status: 'done', lost: 1 });
  });
});

describe('cancel', () => {
  it('a new query cancels the running one on the servers that still work', () => {
    const { s, sent } = rig();
    s.run('one');
    s.run('two');
    expect(sent.map((x) => (x.msg as { cancel?: boolean }).cancel ?? false)).toEqual([false, true, false]);
    expect(sent[1].msg).toMatchObject({ query: '' });
  });
  it('closing cancels, forgets the query and the hits, and ignores a late answer', () => {
    const { s, sent } = rig();
    s.run('secret words');
    const id = (sent[0].msg as { id: number }).id;
    s.handle('a', hit(id));
    s.close();
    expect(sent.at(-1)?.msg).toMatchObject({ cancel: true });
    expect(s.current).toMatchObject({ query: '', hits: [], status: 'idle' });
    s.handle('a', hit(id));
    expect(s.current.hits).toEqual([]);
  });
  it('does not send a cancel for a search that already ended', () => {
    const { s, sent } = rig();
    s.run('x');
    s.handle('a', done((sent[0].msg as { id: number }).id));
    s.stop();
    expect(sent).toHaveLength(1);
  });
});

describe('grouping', () => {
  it('groups by workspace, newest group first, newest hit first', () => {
    const { s, sent } = rig();
    s.run('x');
    const id = (sent[0].msg as { id: number }).id;
    s.handle('a', hit(id, { workspace: 'old', turn: '1', at: '2026-10-01T00:00:00Z' }));
    s.handle('a', hit(id, { workspace: 'new', turn: '2', at: '2026-10-29T00:00:00Z' }));
    s.handle('a', hit(id, { workspace: 'new', turn: '3', at: '2026-10-30T00:00:00Z' }));
    const g = groupHits(s.current.hits, 'workspace', (_sid, w) => (w === 'new' ? 'Fresh' : undefined));
    expect(g.map((x) => x.name)).toEqual(['Fresh', 'old']);
    expect(g[0].hits.map((h) => h.turn)).toEqual(['3', '2']);
  });
  it('groups by agent, or not at all, newest first', () => {
    const { s, sent } = rig();
    s.run('x');
    const id = (sent[0].msg as { id: number }).id;
    s.handle('a', hit(id, { workspace: 'w1', agent: 'claude', turn: '1', at: '2026-10-01T00:00:00Z' }));
    s.handle('a', hit(id, { workspace: 'w2', agent: 'codex', turn: '2', at: '2026-10-29T00:00:00Z' }));
    s.handle('a', hit(id, { workspace: 'w2', agent: 'claude', turn: '3', at: '2026-10-30T00:00:00Z' }));
    const by = groupHits(s.current.hits, 'agent', () => undefined);
    expect(by.map((x) => x.name)).toEqual(['claude', 'codex']);
    expect(by[0].hits.map((h) => h.turn)).toEqual(['3', '1']);
    const flat = groupHits(s.current.hits, 'none', () => undefined);
    expect(flat).toHaveLength(1);
    expect(flat[0].hits.map((h) => h.turn)).toEqual(['3', '2', '1']);
  });
  it('remembers the grouping per viewer and survives missing or broken storage', () => {
    const m = new Map<string, string>();
    const store = { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) };
    expect(loadGroupMode(store)).toBe('workspace');
    saveGroupMode('agent', store);
    expect(loadGroupMode(store)).toBe('agent');
    m.set([...m.keys()][0], 'bogus');
    expect(loadGroupMode(store)).toBe('workspace');
    const broken = { getItem: () => { throw new Error('denied'); }, setItem: () => { throw new Error('denied'); } };
    expect(loadGroupMode(broken)).toBe('workspace');
    expect(() => saveGroupMode('none', broken)).not.toThrow();
    expect(loadGroupMode(undefined)).toBe('workspace');
  });
  it('formats a time as the clock today and the day otherwise', () => {
    const now = Date.parse('2026-10-31T12:00:00Z');
    expect(whenText(undefined, now)).toBe('');
    expect(whenText(now - 86_400_000 * 1.2, now)).toBe('Yesterday');
  });
});

describe('debounced typing', () => {
  it('searches once, for the last text, after a pause; Enter does not wait', async () => {
    const { vi } = await import('vitest');
    vi.useFakeTimers();
    const { debounced } = await import('./sessionSearch');
    const got: string[] = [];
    const d = debounced((t) => got.push(t), 250);
    d.push('p'); d.push('pa'); d.push('par');
    vi.advanceTimersByTime(249);
    expect(got).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(got).toEqual(['par']);
    d.push('pars'); d.flush();
    expect(got).toEqual(['par', 'pars']);
    d.push('x'); d.cancel(); vi.advanceTimersByTime(1000);
    expect(got).toHaveLength(2);
    vi.useRealTimers();
  });
});

describe('what a tap does', () => {
  it('switches to an open terminal tab, jumps in an open ACP workspace, and asks before resuming a dormant one', () => {
    expect(landingOf({ up: true, tab: 't1' })).toBe('switch');
    expect(landingOf({ up: true, tab: '' })).toBe('jump');
    expect(landingOf({ up: false, tab: 't1' })).toBe('resume');
    expect(landingOf({ up: false, tab: '' })).toBe('resume');
  });
  it('after a resume: a resumed tab only switches, an ACP workspace jumps, a shell fallback says so', () => {
    expect(resumeOutcome({ tab: 't1', agent: 'claude' }, [{ tab: 't1', outcome: 'resumed' }])).toEqual({ message: 'Resuming claude in this tab.', jump: false, ok: true });
    expect(resumeOutcome({ tab: '', agent: 'claude' }, undefined)).toMatchObject({ jump: true, ok: true });
    const shell = resumeOutcome({ tab: 't1', agent: 'codex' }, [{ tab: 't1', outcome: 'shell', reason: 'no conversation was recorded for this tab' }]);
    expect(shell).toMatchObject({ jump: false, ok: false });
    expect(shell.message).toContain('The tab is a shell again');
    expect(shell.message).toContain('no conversation was recorded');
    expect(shell.message).not.toMatch(/shown|found/);
  });
  it('does not claim a resume from a server that did not say', () => {
    expect(resumeOutcome({ tab: 't1', agent: 'codex' }, undefined)).toMatchObject({ jump: false, ok: false });
  });
});
