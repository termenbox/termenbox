// Older turns of an ACP workspace, read from the agent's own file (R78, decision 0157). The live reducer keeps its 400
// entries (`MAX_ENTRIES`) and stays the only source for what it holds; what the file adds sits apart, above the live
// list, and only ever holds one window per workspace, so memory is bounded however many hits are opened. Texts come
// from the server and are drawn as text only.
import type { Entry } from './acpStream';
import type { SessionWindowMessage, WindowEntry } from './protocol';

/** Windows kept at once on this device (one per workspace); the least recently asked is dropped first. */
export const MAX_WINDOWS = 4;
/** The server caps a window at 200 turns before and after; this is the client's own limit on what it draws. */
export const MAX_HISTORY_ENTRIES = 400;

export interface HistoryEntry { entry: Entry; turn: string }

/** The first part of an entry's text that decides whether two entries are the same turn. */
const norm = (s: string) => s.replace(/\s+/g, ' ').trim().slice(0, 200);

/** A file entry shaped like a live one so `AcpEntryView` draws it unchanged. Ids are negative: never a live id. */
export function asEntry(w: WindowEntry, id: number): Entry {
  const base = { id, rev: 0 };
  const text = typeof w.text === 'string' ? w.text : '';
  switch (w.kind) {
    case 'user': return { ...base, kind: 'user', text };
    case 'message': return { ...base, kind: 'message', text, truncated: !!w.truncated };
    case 'thought': return { ...base, kind: 'thought', text, truncated: !!w.truncated };
    case 'tool': return { ...base, kind: 'tool', toolId: `file:${w.turn}`, title: text, toolKind: 'other', status: 'completed', diffs: [], locations: [] };
    default: return { ...base, kind: 'notice', level: 'info', text };
  }
}

/** Whether a file entry is a turn the live list already holds. The live entries carry no agent turn id yet, so this is
 *  the same kind with the same opening text; it errs towards "same", so a turn is never drawn twice. */
export function inRing(w: WindowEntry, live: Entry[]): Entry | undefined {
  if (w.kind !== 'user' && w.kind !== 'message' && w.kind !== 'thought') return undefined;
  const t = norm(w.text ?? '');
  if (!t) return undefined;
  return live.find((e) => e.kind === w.kind && 'text' in e && norm(e.text) === t);
}

export interface Joined {
  /** The file entries older than the live list, in conversation order. */
  history: HistoryEntry[];
  /** The live entry that already holds the asked turn (the ring wins), when it does. */
  live?: Entry;
  /** The asked turn among `history`, when it is older than the live list. */
  anchor?: HistoryEntry;
}

/** The ring wins: the file entries stop at the first one the live list holds, and a hit inside it lands on the live entry. */
export function join(w: SessionWindowMessage, live: Entry[], firstId = -1): Joined {
  const entries = Array.isArray(w.entries) ? w.entries.slice(0, MAX_HISTORY_ENTRIES) : [];
  const cut = entries.findIndex((e) => inRing(e, live));
  const kept = cut < 0 ? entries : entries.slice(0, cut);
  const history = kept.map((e, i) => ({ entry: asEntry(e, firstId - i), turn: e.turn }));
  const asked = entries[w.anchor];
  if (asked && cut >= 0 && w.anchor >= cut) return { history, live: inRing(asked, live) ?? live[0] };
  return { history, anchor: history.find((h) => h.turn === w.turn) };
}

/** One window per workspace, newest asked last. A reply for a workspace replaces its older one. */
export type WindowMap = Record<string, SessionWindowMessage>;
export function withWindow(map: WindowMap, key: string, msg: SessionWindowMessage): WindowMap {
  const next: WindowMap = { ...map };
  delete next[key];
  next[key] = { ...msg, entries: msg.entries.slice(0, MAX_HISTORY_ENTRIES) };
  const keys = Object.keys(next);
  for (const k of keys.slice(0, Math.max(0, keys.length - MAX_WINDOWS))) delete next[k];
  return next;
}
/** True when a reply to the same workspace came from a file that changed since the window held before it: the place a
 *  search found may have moved, and the sheet's hits are older than the file. */
export function fileChanged(held: SessionWindowMessage | undefined, msg: SessionWindowMessage): boolean {
  return !!held && !!held.fingerprint && !!msg.fingerprint && held.fingerprint !== msg.fingerprint;
}
