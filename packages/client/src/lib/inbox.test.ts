import { describe, expect, it } from 'vitest';
import { INBOX_MAX, loadInbox, markAllRead, mergeAttention, saveInbox, unreadCount, type InboxItem } from './inbox';
import type { Attention } from './protocol';

const att = (id: string, at: number, kind: Attention['kind'] = 'finished_unseen', workspace = 's'): Attention => ({ id, workspace: workspace, kind, raised_at: at, task: { title: 'fix login', started_at: 1, source: 'hook' } });
const merge = (list: InboxItem[], items: Attention[], server = 'srv') => mergeAttention(list, server, items, (s) => `${server}/${s}`, (s) => s.toUpperCase());

describe('inbox', () => {
  it('adds each item once, newest first, unread', () => {
    let l = merge([], [att('a1', 100), att('a2', 200, 'needs_input')]);
    expect(l.map((i) => i.at)).toEqual([200, 100]);
    expect(l[0]).toMatchObject({ key: 'srv/s', name: 'S', kind: 'needs_input', read: false, title: 'fix login' });
    expect(merge(l, [att('a1', 100)])).toBe(l);
    expect(unreadCount(l)).toBe(2);
  });

  it('tells a server restart from a repeat: the same id at another time is a new item', () => {
    const l = merge(merge([], [att('a1', 100)]), [att('a1', 900)]);
    expect(l).toHaveLength(2);
  });

  it('is bounded to 200 items and drops the oldest', () => {
    let l: InboxItem[] = [];
    for (let i = 0; i < INBOX_MAX + 20; i++) l = merge(l, [att(`a${i}`, i + 1)]);
    expect(l).toHaveLength(INBOX_MAX);
    expect(l[0].at).toBe(INBOX_MAX + 20);
  });

  it('marks everything read and ignores kinds it does not know', () => {
    const l = markAllRead(merge([], [att('a1', 1), att('a2', 2, 'unknown' as never)]));
    expect(l).toHaveLength(1);
    expect(unreadCount(l)).toBe(0);
  });

  it('survives an app restart through storage, and a damaged copy', () => {
    const mem: Record<string, string> = {};
    const s = { getItem: (k: string) => mem[k] ?? null, setItem: (k: string, v: string) => void (mem[k] = v) };
    const l = merge([], [att('a1', 5)]);
    saveInbox(l, s);
    expect(loadInbox(s)).toEqual(l);
    mem.termenbox_inbox = '{"not":"a list"}';
    expect(loadInbox(s)).toEqual([]);
    mem.termenbox_inbox = 'garbage';
    expect(loadInbox(s)).toEqual([]);
  });
});
