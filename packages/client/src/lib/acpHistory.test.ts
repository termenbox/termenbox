import { describe, it, expect } from 'vitest';
import { asEntry, fileChanged, inRing, join, withWindow, MAX_HISTORY_ENTRIES, MAX_WINDOWS } from './acpHistory';
import type { Entry } from './acpStream';
import type { SessionWindowMessage, WindowEntry } from './protocol';

const w = (turn: string, kind: string, text: string): WindowEntry => ({ turn, kind, text });
const win = (entries: WindowEntry[], anchor: number, over: Partial<SessionWindowMessage> = {}): SessionWindowMessage =>
  ({ type: 'session_window', workspace: 'w', tab: '', turn: entries[anchor]?.turn ?? '', anchor, entries, fingerprint: 'f1', ...over });
const live = (id: number, kind: 'user' | 'message', text: string): Entry => (kind === 'user' ? { id, rev: 0, kind, text } : { id, rev: 0, kind, text, truncated: false });

describe('file entries drawn like live ones', () => {
  it('maps each kind and gives negative ids that never collide with a live id', () => {
    expect(asEntry(w('a', 'user', 'hi'), -1)).toMatchObject({ kind: 'user', text: 'hi', id: -1 });
    expect(asEntry(w('b', 'message', 'yo'), -2)).toMatchObject({ kind: 'message', truncated: false });
    expect(asEntry(w('c', 'tool', 'Bash'), -3)).toMatchObject({ kind: 'tool', title: 'Bash', diffs: [] });
    expect(asEntry(w('d', 'plan', 'step'), -4)).toMatchObject({ kind: 'notice', text: 'step' });
    expect(asEntry(w('e', 'weird', 'x'), -5).kind).toBe('notice');
  });
});

describe('the ring wins', () => {
  const ring = [live(7, 'user', 'deploy the thing'), live(8, 'message', 'done')];
  it('shows a turn older than the live list in the history block, anchored', () => {
    const j = join(win([w('t1', 'user', 'fix parser'), w('t2', 'message', 'fixed'), w('t3', 'tool', 'Edit')], 0), ring);
    expect(j.history.map((h) => h.turn)).toEqual(['t1', 't2', 't3']);
    expect(j.anchor?.turn).toBe('t1');
    expect(j.live).toBeUndefined();
  });
  it('stops the file entries at the first one the live list holds and lands a hit inside it on the live entry', () => {
    const j = join(win([w('t1', 'user', 'fix parser'), w('t2', 'user', 'deploy  the thing'), w('t3', 'message', 'done')], 2), ring);
    expect(j.history.map((h) => h.turn)).toEqual(['t1']);
    expect(j.live?.id).toBe(8);
    expect(j.anchor).toBeUndefined();
  });
  it('never matches a tool or an empty text, so only real repeats are dropped', () => {
    expect(inRing(w('x', 'tool', 'deploy the thing'), ring)).toBeUndefined();
    expect(inRing(w('x', 'user', '   '), ring)).toBeUndefined();
    expect(inRing(w('x', 'message', 'done'), ring)?.id).toBe(8);
  });
});

describe('bounded memory', () => {
  it('keeps one window per workspace and the last few workspaces', () => {
    let m = {};
    for (let i = 0; i < 20; i++) m = withWindow(m, `s/w${i % 6}`, win([w(`t${i}`, 'user', 'x')], 0));
    expect(Object.keys(m).length).toBeLessThanOrEqual(MAX_WINDOWS);
    m = withWindow(m, 's/w5', win([w('again', 'user', 'y')], 0));
    expect(Object.keys(m).filter((k) => k === 's/w5')).toHaveLength(1);
  });
  it('caps the entries of one window', () => {
    const many = Array.from({ length: MAX_HISTORY_ENTRIES + 100 }, (_, i) => w(`t${i}`, 'user', `u${i}`));
    expect(Object.values(withWindow({}, 'k', win(many, 0)))[0].entries).toHaveLength(MAX_HISTORY_ENTRIES);
    expect(join(win(many, 0), []).history).toHaveLength(MAX_HISTORY_ENTRIES);
  });
});

describe('a file that changed', () => {
  it('is noticed from the fingerprint, and only between two known ones', () => {
    const a = win([], 0, { fingerprint: 'f1' });
    expect(fileChanged(a, win([], 0, { fingerprint: 'f2' }))).toBe(true);
    expect(fileChanged(a, win([], 0, { fingerprint: 'f1' }))).toBe(false);
    expect(fileChanged(undefined, a)).toBe(false);
    expect(fileChanged(a, win([], 0, { fingerprint: '' }))).toBe(false);
  });
});
