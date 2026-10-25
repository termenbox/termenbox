import { describe, expect, it, vi } from 'vitest';
import type { PlayResult } from './bell';
import { announce, attentionIcon, bannerChoices, bannerText, createAlerter, createAttention, createNotifier, VIBRATE_PATTERN, PENDING_MS, titleFor, waitingList } from './notify';
import type { AgentMap, WorkspaceAgent } from './agents';

const ag = (patch: Partial<WorkspaceAgent>): WorkspaceAgent => ({ state: 'needs_input', seq: 1, stale: false, ts: 1000, ...patch });

describe('needs-you signals', () => {
  it('lists who waits, the longest wait first, and never lists the rest', () => {
    const agents: AgentMap = { a: ag({ waitingSince: 500 }), b: ag({ waitingSince: 100 }), c: ag({ state: 'working' }) };
    const list = waitingList([{ key: 'a', name: 'A' }, { key: 'b', name: 'B' }, { key: 'c', name: 'C' }, { key: 'd', name: 'D' }], agents);
    expect(list.map(w => w.key)).toEqual(['b', 'a']);
  });
  it('words the count and the title', () => {
    expect(titleFor(2)).toBe('(2) Termenbox');
    expect(titleFor(0)).toBe('Termenbox');
    expect(bannerText(1)).toBe('1 needs you');
    expect(bannerText(3)).toBe('3 need you');
  });
  it('offers answers only for a confirmed, fresh, short prompt', () => {
    const prompt = { id: '1', kind: 'yes_no', choices: ['Yes', 'No'], seq: 2, confidence: 'high' };
    expect(bannerChoices(ag({ prompt }), 1000)).toEqual(['Yes', 'No']);
    expect(bannerChoices(ag({ prompt: { ...prompt, confidence: 'low' } }), 1000)).toEqual([]);
    expect(bannerChoices(ag({ prompt: { ...prompt, choices: ['1', '2', '3', '4'] } }), 1000)).toEqual([]);
    expect(bannerChoices(ag({ state: 'working', prompt }), 1000)).toEqual([]);
    expect(bannerChoices(ag({}), 1000)).toEqual([]);
  });
  it('sets the title and the app badge, and clears it', () => {
    const doc = { title: '' };
    const nav = { setAppBadge: vi.fn(async () => {}), clearAppBadge: vi.fn(async () => {}) };
    announce(2, doc, nav);
    expect(doc.title).toBe('(2) Termenbox');
    expect(nav.setAppBadge).toHaveBeenCalledWith(2);
    announce(0, doc, nav);
    expect(nav.clearAppBadge).toHaveBeenCalled();
    announce(1, doc, {}); // a browser without badging
  });
});

describe('attention while the tab is hidden', () => {
  const page = () => ({ title: 'Termenbox', icon: { href: 'https://x.test/favicon.svg?v=3' } });

  it('maps the favicon to its attention twin and leaves other names alone', () => {
    expect(attentionIcon('/favicon.svg')).toBe('/favicon-attention.svg');
    expect(attentionIcon('https://x.test/favicon.svg?v=3')).toBe('https://x.test/favicon-attention.svg?v=3');
    expect(attentionIcon('/other.png')).toBe('/other.png');
  });

  it('flashes only while hidden, and holds still while the page is visible', () => {
    vi.useFakeTimers();
    const p = page();
    const a = createAttention(p, 1000);
    a.update(2, false);
    expect(p.title).toBe('(2) Termenbox');
    vi.advanceTimersByTime(3000);
    expect(p.title).toBe('(2) Termenbox');
    a.update(2, true);
    expect(p.icon.href).toContain('favicon-attention.svg');
    vi.advanceTimersByTime(1000);
    expect(p.title).toBe('Termenbox');
    expect(p.icon.href).toBe('https://x.test/favicon.svg?v=3');
    vi.advanceTimersByTime(1000);
    expect(p.title).toBe('(2) Termenbox');
    a.stop();
    vi.useRealTimers();
  });

  it('restores the page and stops the timer when nothing needs you', () => {
    vi.useFakeTimers();
    const p = page();
    const a = createAttention(p, 1000);
    a.update(1, true);
    a.update(0, true);
    expect(p.title).toBe('Termenbox');
    expect(p.icon.href).toBe('https://x.test/favicon.svg?v=3');
    expect(vi.getTimerCount()).toBe(0);
    vi.useRealTimers();
  });

  it('works on a page without a favicon link', () => {
    const p = { title: 'Termenbox', icon: null };
    const a = createAttention(p);
    a.update(1, false);
    expect(p.title).toBe('(1) Termenbox');
    a.stop();
  });
});

describe('browser notifications', () => {
  const w = (key: string, name = key) => ({ key, name, since: 1, agent: ag({}) });
  const setup = (permission = 'granted') => {
    const shown: { title: string; options: NotificationOptions }[] = [];
    const opened: string[] = [];
    const notes: { onclick: (() => void) | null; close: () => void }[] = [];
    const update = createNotifier((title, options) => {
      shown.push({ title, options });
      const n = { onclick: null as (() => void) | null, close: vi.fn() };
      notes.push(n);
      return n;
    }, () => permission, (k) => opened.push(k));
    return { update, shown, opened, notes };
  };

  it('stays quiet while the page is visible, without permission, or when push already notifies', () => {
    const { update, shown } = setup();
    update([w('a')], false, false);
    update([w('a')], true, true);
    setup('default').update([w('a')], true, false);
    expect(shown).toHaveLength(0);
  });

  it('tells once per workspace however often it is updated, and again after the workspace stops needing you', () => {
    const { update, shown } = setup();
    update([w('a', 'Alpha')], true, false);
    update([w('a', 'Alpha')], true, false);
    expect(shown).toHaveLength(1);
    expect(shown[0].options).toMatchObject({ body: 'Alpha needs you', tag: 'termenbox-a' });
    update([w('a'), w('b')], true, false);
    expect(shown).toHaveLength(2);
    update([], true, false);
    update([w('a')], true, false);
    expect(shown).toHaveLength(3);
  });

  it('opens the workspace when the notification is clicked', () => {
    const { update, opened, notes } = setup();
    update([w('a')], true, false);
    notes[0].onclick?.();
    expect(opened).toEqual(['a']);
    expect(notes[0].close).toHaveBeenCalled();
  });
});

describe('sound and vibration when a workspace starts needing you', () => {
  const w = (key: string, since: number, clockOffset?: number) => ({ key, name: key, since, agent: ag({ clockOffset }) });
  const both = { sound: true, vibrate: true };
  const setup = (now = 100_000) => {
    const play = vi.fn((_kind: string): PlayResult => 'played');
    const vibrate = vi.fn(() => true);
    const log = vi.fn();
    const clock = { now };
    return { play, vibrate, log, clock, update: createAlerter(play, vibrate, () => clock.now, log) };
  };

  it('alerts once for a fresh item, with the pattern, and not again while it waits', () => {
    const { play, vibrate, update } = setup();
    update([w('a', 95_000)], both);
    update([w('a', 95_000)], both);
    expect(play).toHaveBeenCalledTimes(1);
    expect(vibrate).toHaveBeenCalledWith(VIBRATE_PATTERN);
    expect(vibrate).toHaveBeenCalledTimes(1);
  });

  it('stays quiet for a queue that was already waiting when the page opened', () => {
    const { play, vibrate, update } = setup();
    update([w('a', 10_000)], both);
    expect(play).not.toHaveBeenCalled();
    expect(vibrate).not.toHaveBeenCalled();
  });

  it('alerts again when a workspace needs you again after being answered', () => {
    const { play, update } = setup();
    update([w('a', 99_000)], both);
    update([], both);
    update([w('a', 99_500)], both);
    expect(play).toHaveBeenCalledTimes(2);
  });

  it('rings once for several items landing together', () => {
    const { play, update } = setup();
    update([w('a', 99_000), w('b', 99_100)], both);
    expect(play).toHaveBeenCalledTimes(1);
  });

  it('honours each setting on its own', () => {
    const a = setup();
    a.update([w('a', 99_000)], { sound: false, vibrate: true });
    expect(a.play).not.toHaveBeenCalled();
    expect(a.vibrate).toHaveBeenCalledTimes(1);
    const b = setup();
    b.update([w('a', 99_000)], { sound: true, vibrate: false });
    expect(b.play).toHaveBeenCalledTimes(1);
    expect(b.vibrate).not.toHaveBeenCalled();
  });

  it('reads the item on this device clock, using the offset of the server clock', () => {
    // the server's clock is 60 s ahead: its 160 000 is 100 000 here, just now
    const { play, update } = setup(100_000);
    update([w('a', 160_000, 60_000)], both);
    expect(play).toHaveBeenCalledTimes(1);
  });
  it('sounds for a question that landed while the page slept, and not for a queue waiting before it opened', () => {
    const { play, update, clock } = setup(100_000);
    update([], both);
    clock.now = 300_000; // the phone woke 200 s later; the server clock matches (offset 0)
    update([w('a', 150_000, 0)], both);
    expect(play).toHaveBeenCalledTimes(1);
    const opened = setup(100_000);
    opened.update([w('b', 50_000, 0)], both); // raised before the page was created
    expect(opened.play).not.toHaveBeenCalled();
  });

  it('does not use the last-check rule without a known clock offset', () => {
    const { play, update, clock } = setup(100_000);
    update([], both);
    clock.now = 300_000;
    update([w('a', 150_000)], both);
    expect(play).not.toHaveBeenCalled();
  });

  it('plays the asking form for a question and the done form for a finished reply', () => {
    const asked = setup();
    asked.update([w('a', 99_000)], both);
    expect(asked.play).toHaveBeenCalledWith('ask');
    const finished = setup();
    finished.update([{ ...w('a', 99_000), agent: ag({ attention: { id: '1', session: 'a', kind: 'finished_unseen', raised_at: 99_000 } }) }], both);
    expect(finished.play).toHaveBeenCalledWith('done');
  });

  it('keeps a sound that found audio locked and plays it on the next tap while the workspace still waits', () => {
    const { play, update, clock } = setup();
    play.mockReturnValueOnce('locked');
    update([w('a', 99_000)], both);
    expect(play).toHaveBeenCalledTimes(1);
    clock.now += 10_000;
    update.replay();
    expect(play).toHaveBeenCalledTimes(2);
    update.replay(); // once only
    expect(play).toHaveBeenCalledTimes(2);
  });

  it('drops a kept sound that is too old, or whose workspace no longer waits', () => {
    const old = setup();
    old.play.mockReturnValueOnce('locked');
    old.update([w('a', 99_000)], both);
    old.clock.now += PENDING_MS + 1;
    old.update.replay();
    expect(old.play).toHaveBeenCalledTimes(1);
    const gone = setup();
    gone.play.mockReturnValueOnce('locked');
    gone.update([w('a', 99_000)], both);
    gone.update([], both);
    gone.update.replay();
    expect(gone.play).toHaveBeenCalledTimes(1);
  });

  it('keeps a kept sound while audio is still locked', () => {
    const { play, update } = setup();
    play.mockReturnValue('locked');
    update([w('a', 99_000)], both);
    update.replay();
    play.mockReturnValue('played');
    update.replay();
    expect(play).toHaveBeenCalledTimes(3);
  });

  it('says why in one log line per update with new items, without a workspace name', () => {
    const { log, update } = setup();
    update([w('secret-workspace', 10_000)], both);
    update([w('secret-workspace', 10_000)], both);
    expect(log).toHaveBeenCalledTimes(1);
    expect(log.mock.calls[0][0]).toMatch(/1 new, 0 fresh \(ages 90 s.*stale/);
    expect(log.mock.calls[0][0]).not.toContain('secret-workspace');
    const off = setup();
    off.update([w('a', 99_000)], { sound: false, vibrate: true });
    expect(off.log.mock.calls[0][0]).toMatch(/sound off, vibrated/);
    const locked = setup();
    locked.play.mockReturnValue('locked');
    locked.update([w('a', 99_000)], both);
    expect(locked.log.mock.calls[0][0]).toMatch(/sound locked/);
  });
});
