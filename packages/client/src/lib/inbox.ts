// The inbox (decision 0095): every Attention item that reached this device, newest first, kept across restarts.
// It reads the same items as the banner, the title count and the badge, so there is one source. Task titles stay
// on this device; they are never part of a push.
import { writable } from 'svelte/store';
import type { Attention, AttentionKind } from './protocol';

export const INBOX_MAX = 200;
const KEY = 'termenbox_inbox';

export interface InboxItem {
  /** Unique across servers and server restarts. */
  id: string;
  /** The workspace key as the hub knows it. */
  key: string;
  name: string;
  kind: AttentionKind;
  at: number;
  title?: string;
  read: boolean;
}

/** Adds the items not seen before; the list stays newest first and bounded. */
export function mergeAttention(list: InboxItem[], serverId: string, items: Attention[], keyOf: (workspace: string) => string, nameOf: (workspace: string) => string): InboxItem[] {
  const have = new Set(list.map((i) => i.id));
  const fresh: InboxItem[] = [];
  for (const it of items) {
    const id = `${serverId}:${it.id}:${it.raised_at}`;
    if (have.has(id) || it.kind === 'unknown') continue;
    have.add(id);
    fresh.push({ id, key: keyOf(it.session), name: nameOf(it.session), kind: it.kind, at: it.raised_at, title: it.task?.title, read: false });
  }
  if (!fresh.length) return list;
  return [...fresh.sort((a, b) => b.at - a.at), ...list].slice(0, INBOX_MAX);
}

export const unreadCount = (list: InboxItem[]) => list.filter((i) => !i.read).length;
export const markAllRead = (list: InboxItem[]) => (unreadCount(list) ? list.map((i) => (i.read ? i : { ...i, read: true })) : list);

const store = () => (typeof localStorage === 'undefined' ? undefined : localStorage);

export function loadInbox(s: Pick<Storage, 'getItem'> | undefined = store()): InboxItem[] {
  try {
    const v = JSON.parse(s?.getItem(KEY) ?? '[]');
    if (!Array.isArray(v)) return [];
    return v
      .filter((i) => i && typeof i.id === 'string' && typeof i.key === 'string' && typeof i.at === 'number')
      .map((i) => ({ id: i.id, key: i.key, name: String(i.name ?? ''), kind: i.kind, at: i.at, title: typeof i.title === 'string' ? i.title : undefined, read: !!i.read }))
      .slice(0, INBOX_MAX);
  } catch {
    return [];
  }
}

export function saveInbox(list: InboxItem[], s: Pick<Storage, 'setItem'> | undefined = store()) {
  try {
    s?.setItem(KEY, JSON.stringify(list));
  } catch {
    /* Private mode: kept in memory. */
  }
}

export const inbox = writable<InboxItem[]>(loadInbox());
inbox.subscribe((l) => saveInbox(l));
