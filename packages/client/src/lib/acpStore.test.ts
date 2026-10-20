import { describe, expect, it } from 'vitest';
import { MAX_STREAMS, applyAcp, forgetAcp, loadSaved, placeFor, save } from './acpStore';
import type { AcpViewMessage } from './protocol';

const view = (over: Partial<AcpViewMessage> = {}): AcpViewMessage => ({ type: 'acp_view', session: 's', epoch: 1, rev: 1, reset: true, first: 1, entries: [{ id: 1, rev: 1, kind: 'user', text: 'hi' }], ...over });
const memory = () => { const m = new Map<string, string>(); return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) }; };

describe('streams held on this device', () => {
  it('keeps one stream per session and drops the least recently updated beyond the cap', () => {
    let map = {};
    for (let i = 0; i < MAX_STREAMS + 3; i++) map = applyAcp(map, `k${i}`, `s${i}`, view());
    expect(Object.keys(map)).toHaveLength(MAX_STREAMS);
    expect(map).not.toHaveProperty('k0');
    map = applyAcp(map, 'k4', 's4', view({ reset: false, rev: 2, entries: [] }));
    expect(Object.keys(map).at(-1)).toBe('k4');
  });
  it('forgets the streams of that server’s sessions that no longer exist, and no other server’s', () => {
    const map = applyAcp(applyAcp(applyAcp({}, 'one/a', 's', view()), 'one/b', 's', view()), 'two/a', 's', view());
    expect(Object.keys(forgetAcp(map, 'one/', new Set(['one/b'])))).toEqual(['one/b', 'two/a']);
    expect(forgetAcp(map, 'one/', new Set(['one/a', 'one/b']))).toBe(map);
  });
});

describe('what the view remembers across a reload', () => {
  it('round-trips the draft, the place and the opened diffs, and merges patches', () => {
    const st = memory();
    save('k', { draft: 'fix the' }, st);
    save('k', { place: { entry: 7, offset: 12, bottom: false } }, st);
    expect(loadSaved('k', st)).toEqual({ draft: 'fix the', place: { entry: 7, offset: 12, bottom: false }, opened: undefined });
    save('k', { draft: '' }, st);
    expect(loadSaved('k', st).draft).toBeUndefined();
  });
  it('survives garbage and a store that throws', () => {
    const bad = { getItem: () => '{{{', setItem: () => { throw new Error('full'); } };
    expect(loadSaved('k', bad)).toEqual({});
    expect(() => save('k', { draft: 'x' }, bad)).not.toThrow();
    const odd = { getItem: () => JSON.stringify({ k: { draft: 5, place: { entry: 'x' }, opened: [1, 'a'] } }), setItem: () => {} };
    expect(loadSaved('k', odd)).toEqual({ draft: undefined, place: undefined, opened: ['a'] });
  });
  it('keeps only the newest sessions', () => {
    const st = memory();
    for (let i = 0; i < 40; i++) save(`k${i}`, { draft: String(i) }, st);
    expect(loadSaved('k0', st).draft).toBeUndefined();
    expect(loadSaved('k39', st).draft).toBe('39');
  });
});

describe('the reader’s place and the epoch', () => {
  it('a place in the text belongs to the epoch it was taken in; the end is good in any', () => {
    const mid = { entry: 7, offset: 12, bottom: false, epoch: 3 };
    expect(placeFor(mid, 3)).toBe(mid);
    expect(placeFor(mid, 4)).toBeUndefined();
    expect(placeFor({ entry: 7, offset: 12, bottom: false }, 3)).toBeUndefined();
    const end = { entry: 0, offset: 0, bottom: true, epoch: 3 };
    expect(placeFor(end, 4)).toBe(end);
    expect(placeFor(undefined, 4)).toBeUndefined();
  });
  it('is saved with its epoch and read back with it', () => {
    const m = memory();
    save('k', { place: { entry: 2, offset: 5, bottom: false, epoch: 99 } }, m);
    expect(loadSaved('k', m).place).toEqual({ entry: 2, offset: 5, bottom: false, epoch: 99 });
  });
});
