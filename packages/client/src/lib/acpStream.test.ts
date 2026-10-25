import { describe, expect, it } from 'vitest';
import { MAX_ENTRIES, OPEN_WAIT_MS, TRUNCATED_NOTE, applyView, isNewEntry, openingText, shellName, cleanBlock, contextPercent, copyText, diffRows, diffStat, emptyStream, entryFrom, infoFrom, openMessage, permissionOutcome, planProgress, search, toolStatus, toolsAsking, transcriptText, type Entry, type StreamState } from './acpStream';
import type { AcpEntryWire, AcpViewMessage } from './protocol';

const view = (over: Partial<AcpViewMessage> = {}): AcpViewMessage => ({ type: 'acp_view', session: 's', epoch: 7, rev: 1, reset: true, first: 1, entries: [], ...over });
const user = (id: number, text: string, rev = id): AcpEntryWire => ({ id, rev, kind: 'user', text });
const msg = (id: number, text: string, rev = id): AcpEntryWire => ({ id, rev, kind: 'message', text });
const texts = (s: StreamState) => s.entries.map((e) => ('text' in e ? e.text : e.kind));

describe('the stream reducer', () => {
  it('starts from a reset and applies deltas: a grown reply replaces itself, new entries append', () => {
    let s = applyView(emptyStream('s'), view({ rev: 2, entries: [user(1, 'go'), msg(2, 'Hel')] }));
    expect(texts(s)).toEqual(['go', 'Hel']);
    s = applyView(s, view({ reset: false, rev: 3, entries: [msg(2, 'Hello', 3)] }));
    expect(texts(s)).toEqual(['go', 'Hello']);
    s = applyView(s, view({ reset: false, rev: 4, entries: [msg(3, 'Next', 4)] }));
    expect(texts(s)).toEqual(['go', 'Hello', 'Next']);
    expect(s.rev).toBe(4);
  });

  it('a reset, or another epoch, drops what was held', () => {
    let s = applyView(emptyStream('s'), view({ entries: [user(1, 'old')] }));
    s = applyView(s, view({ reset: true, epoch: 8, rev: 1, entries: [user(1, 'new')] }));
    expect(texts(s)).toEqual(['new']);
    s = applyView(s, view({ reset: false, epoch: 9, rev: 1, entries: [user(5, 'again')], first: 5 }));
    expect(texts(s)).toEqual(['again']);
  });

  it('drops what the server no longer keeps and holds at most MAX_ENTRIES', () => {
    const many = Array.from({ length: MAX_ENTRIES + 50 }, (_, i) => user(i + 1, `p${i}`));
    let s = applyView(emptyStream('s'), view({ entries: many, first: 1 }));
    expect(s.entries).toHaveLength(MAX_ENTRIES);
    expect(s.entries[0].id).toBe(51);
    s = applyView(s, view({ reset: false, first: 100, entries: [] }));
    expect(s.entries[0].id).toBe(100);
  });

  it('keeps the info it has when a delta carries none, and replaces it when one arrives', () => {
    const info = { agent: 'claude', dir: '/w', status: 'running', working: false, mode: 'ask', modes: [{ id: 'ask', name: 'Ask' }, { id: 'plan', name: 'Plan' }] };
    let s = applyView(emptyStream('s'), view({ info }));
    s = applyView(s, view({ reset: false, rev: 2, entries: [] }));
    expect(s.info?.modes.map((m) => m.id)).toEqual(['ask', 'plan']);
    s = applyView(s, view({ reset: false, rev: 3, info: { ...info, working: true } }));
    expect(s.info?.working).toBe(true);
  });

  it('asks for only what it lacks when it opens a workspace again', () => {
    expect(openMessage('s')).toEqual({ type: 'acp_open', session: 's' });
    const s = applyView(emptyStream('s'), view({ rev: 9, entries: [user(1, 'x')] }));
    expect(openMessage('s', s)).toEqual({ type: 'acp_open', session: 's', after: 9, epoch: 7 });
  });
});

describe('agent text is untrusted', () => {
  it('is cleaned: escapes, controls and bidi out, lines and indentation kept', () => {
    expect(cleanBlock('a\u001b[31mred\u001b[0m\n  b\u0007‮ c\r\nd\te')).toBe('ared\n  b c\nd  e');
    expect(cleanBlock(5)).toBe('');
    expect(cleanBlock('x'.repeat(20_000)).length).toBe(16_000);
  });

  it('one-time options only, ids exact, unknown entry kinds skipped', () => {
    const e = entryFrom({ id: 1, rev: 1, kind: 'permission', prompt_id: 'acp-1', tool_id: 't', title: 'Write', tool_kind: 'edit', state: 'open',
      options: [{ id: 'a', kind: 'allow_once', name: 'Yes' }, { id: 'b', kind: 'allow_always', name: 'Always' }, { id: 'c', kind: 'reject_always', name: 'Never' }, { id: 'd', kind: 'reject_once', name: 'No' }] } as AcpEntryWire);
    expect(e && e.kind === 'permission' && e.options.map((o) => o.id)).toEqual(['a', 'd']);
    expect(entryFrom({ id: 2, rev: 2, kind: 'hologram' })).toBeNull();
    expect(entryFrom({ id: 'x' } as never)).toBeNull();
    expect(infoFrom({ agent: 'x', dir: '/', status: 'levitating', working: false })?.status).toBe('running');
  });
});

describe('what the view derives', () => {
  const asking = entryFrom({ id: 3, rev: 3, kind: 'permission', prompt_id: 'p', tool_id: 'toolA', title: 'Write a.txt', tool_kind: 'edit', options: [], state: 'open' } as AcpEntryWire) as Entry;
  it('shows a diff once: tools with an open permission hide theirs', () => {
    expect([...toolsAsking([asking])]).toEqual(['toolA']);
    expect([...toolsAsking([{ ...(asking as object), state: 'answered' } as Entry])]).toEqual([]);
  });
  it('counts plan progress and context use', () => {
    expect(planProgress([{ status: 'completed' }, { status: 'pending' }, { status: 'completed' }])).toEqual({ done: 2, total: 3 });
    expect(contextPercent({ used: 500, size: 1000 })).toBe(50);
    expect(contextPercent({ used: 5000, size: 1000 })).toBe(100);
    expect(contextPercent(undefined)).toBeNull();
  });
  it('says tool status in words as well as glyphs', () => {
    expect(toolStatus('in_progress')).toEqual({ glyph: '◐', word: 'running' });
    expect(toolStatus('mystery').word).toBe('waiting');
  });
  it('words what the owner chose in the log', () => {
    const e = (chosen?: string, state = 'answered') => ({ ...(asking as object), state, chosen, options: [{ id: 'y', kind: 'allow_once', name: 'Yes' }, { id: 'n', kind: 'reject_once', name: 'No' }] }) as Extract<Entry, { kind: 'permission' }>;
    expect(permissionOutcome(e('y'))).toBe('Allowed once · Yes');
    expect(permissionOutcome(e('n'))).toBe('Rejected · No');
    expect(permissionOutcome(e(undefined, 'cancelled'))).toBe('Cancelled');
  });
});

describe('diffs', () => {
  it('a new file is all additions', () => {
    expect(diffRows({ newText: 'a\nb' })).toEqual([{ t: '+', text: 'a' }, { t: '+', text: 'b' }]);
  });
  it('shows changes with two lines of context and folds the rest', () => {
    const old = Array.from({ length: 20 }, (_, i) => `line ${i}`).join('\n');
    const next = old.replace('line 10', 'LINE TEN');
    const rows = diffRows({ oldText: old, newText: next });
    expect(rows.filter((r) => r.t === '-')).toEqual([{ t: '-', text: 'line 10' }]);
    expect(rows.filter((r) => r.t === '+')).toEqual([{ t: '+', text: 'LINE TEN' }]);
    expect(rows.filter((r) => r.t === 'gap')).toEqual([{ t: 'gap', count: 8 }, { t: 'gap', count: 7 }]);
    expect(diffStat({ oldText: old, newText: next })).toEqual({ added: 1, removed: 1 });
  });
  it('is bounded for a hostile size', () => {
    const big = Array.from({ length: 5000 }, (_, i) => `l${i}`).join('\n');
    const t = Date.now();
    diffRows({ oldText: big, newText: big.replace('l0', 'x') });
    expect(Date.now() - t).toBeLessThan(2000);
  });
});

describe('search and copy', () => {
  const entries = [
    entryFrom(user(1, 'Fix the parser')),
    entryFrom(msg(2, 'The **parser** has a bug.\n\n```\nparser.rs\n```')),
    entryFrom({ id: 3, rev: 3, kind: 'tool', tool_id: 't', title: 'Edit parser.rs', tool_kind: 'edit', status: 'completed', locations: [{ path: '/w/src/parser.rs', line: 3 }] } as AcpEntryWire),
  ] as Entry[];
  it('finds every hit in drawing order with the entry it is in', () => {
    const m = search(entries, 'PARSER');
    expect(m.map((x) => `${x.entry}:${x.index}`)).toEqual(['1:0', '2:0', '2:1', '3:0', '3:1']);
    expect(search(entries, '  ')).toEqual([]);
    expect(search(entries, 'nothing')).toEqual([]);
  });
  it('copies one entry or the whole conversation as text', () => {
    expect(copyText(entries[1])).toContain('parser.rs');
    expect(transcriptText(entries).split('\n\n')[0]).toBe('> Fix the parser');
  });
});

describe('a reply that was cut', () => {
  const cutMsg = (truncated: boolean): AcpEntryWire => ({ id: 1, rev: 1, kind: 'message', text: 'The first part', truncated } as AcpEntryWire);
  it('keeps the server’s flag, and a reply without one is whole', () => {
    expect((entryFrom(cutMsg(true)) as Extract<Entry, { kind: 'message' }>).truncated).toBe(true);
    expect((entryFrom({ id: 1, rev: 1, kind: 'thought', text: 'x' } as AcpEntryWire) as Extract<Entry, { kind: 'thought' }>).truncated).toBe(false);
    expect((entryFrom({ id: 1, rev: 1, kind: 'message', text: 'x', truncated: 'yes' } as unknown as AcpEntryWire) as Extract<Entry, { kind: 'message' }>).truncated).toBe(false);
  });
  it('Copy does not hand out the cut text as if it were the whole reply', () => {
    const cut = entryFrom(cutMsg(true))!;
    expect(copyText(cut)).toBe(`The first part\n[${TRUNCATED_NOTE}]`);
    expect(transcriptText([cut])).toContain(TRUNCATED_NOTE);
    expect(copyText(entryFrom(cutMsg(false))!)).toBe('The first part');
  });
});

describe('opening a workspace on a server that does not answer', () => {
  it('says what it waits for, and after a while that the server may be too old', () => {
    expect(OPEN_WAIT_MS).toBeGreaterThan(2000);
    expect(openingText(true, false)).toBe('Opening the workspace…');
    expect(openingText(true, true)).toMatch(/older version, update it/);
    expect(openingText(false, true)).toBe('Waiting for the connection…');
  });
});

describe('what is new to the reader', () => {
  it('nothing is new until the first full picture, then what is newer than it', () => {
    expect(isNewEntry({ rev: 5 }, undefined)).toBe(false);
    expect(isNewEntry({ rev: 5 }, 5)).toBe(false);
    expect(isNewEntry({ rev: 6 }, 5)).toBe(true);
  });
});

describe('the directory and the companion shell', () => {
  it('hands on the directory exactly, and not at all when cleaning would change it', () => {
    expect(infoFrom({ agent: 'a', dir: '/home/me/my  project', status: 'running', working: false })?.cwd).toBe('/home/me/my  project');
    expect(infoFrom({ agent: 'a', dir: '/w\tx', status: 'running', working: false })?.cwd).toBeUndefined();
    expect(infoFrom({ agent: 'a', dir: '/w'.repeat(300), status: 'running', working: false })?.cwd).toBeUndefined();
    expect(infoFrom({ agent: 'a', dir: '', status: 'running', working: false })?.cwd).toBeUndefined();
  });
  it('names the shell after the workspace, and keeps long names that share a start apart', () => {
    expect(shellName('work')).toBe('work-shell');
    const a = 'a'.repeat(60) + '1', b = 'a'.repeat(60) + '2';
    expect(shellName(a)).not.toBe(shellName(b));
    expect(shellName(a)).toBe(shellName(a));
    expect(shellName(a).length).toBeLessThanOrEqual(64);
  });
  it('a diff is cut at the 200 lines the server keeps', () => {
    const lines = (n: number, p: string) => Array.from({ length: n }, (_, i) => `${p}${i}`).join('\n');
    const rows = diffRows({ oldText: lines(300, 'a'), newText: lines(300, 'b') });
    expect(rows.filter((r) => r.t === '-').length).toBe(200);
  });
});
