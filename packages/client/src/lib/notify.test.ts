import { describe, expect, it, vi } from 'vitest';
import { announce, attentionIcon, bannerChoices, bannerText, createAttention, createNotifier, titleFor, waitingList } from './notify';
import type { AgentMap, SessionAgent } from './agents';

const ag = (patch: Partial<SessionAgent>): SessionAgent => ({ state: 'needs_input', seq: 1, stale: false, ts: 1000, ...patch });

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

  it('tells once per session however often it is updated, and again after the session stops needing you', () => {
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

  it('opens the session when the notification is clicked', () => {
    const { update, opened, notes } = setup();
    update([w('a')], true, false);
    notes[0].onclick?.();
    expect(opened).toEqual(['a']);
    expect(notes[0].close).toHaveBeenCalled();
  });
});
