// The ACP sessions this device holds (U07b): the stream of each, so switching sessions or leaving the view keeps its
// place, and what the view remembers about a session on this device (draft, scroll place, opened diffs) across a
// reload. The stream itself lives on the server; a reload asks for it again with `acp_open`.
import { writable } from 'svelte/store';
import { applyView, emptyStream, type StreamState } from './acpStream';
import type { AcpViewMessage } from './protocol';

/** Streams kept at once on this device; the least recently updated is dropped first (it is asked for again on open). */
export const MAX_STREAMS = 8;

export const acpStreams = writable<Record<string, StreamState>>({});

export function applyAcp(map: Record<string, StreamState>, key: string, session: string, msg: AcpViewMessage): Record<string, StreamState> {
  const held = map[key] ?? emptyStream(session);
  const next: Record<string, StreamState> = { ...map };
  delete next[key]; // reinserted last: the most recently updated
  next[key] = applyView(held, msg);
  const keys = Object.keys(next);
  for (const k of keys.slice(0, Math.max(0, keys.length - MAX_STREAMS))) delete next[k];
  return next;
}

/** After a server's session list: drops the streams of that server's sessions that are no longer in it. */
export function forgetAcp(map: Record<string, StreamState>, serverPrefix: string, live: Set<string>): Record<string, StreamState> {
  const gone = Object.keys(map).filter((k) => k.startsWith(serverPrefix) && !live.has(k));
  if (!gone.length) return map;
  const next = { ...map };
  for (const k of gone) delete next[k];
  return next;
}

// ---- What this device remembers per session

/** Where the reader was: the entry at the top of the screen and how far into it, or at the end (following). Entry ids
 *  restart with each epoch (a server restart), so a place belongs to the epoch it was taken in. */
export interface Place { entry: number; offset: number; bottom: boolean; epoch?: number }

/** The place when it can be used with the stream of `epoch`: the end always can, a place in the text only in its own epoch. */
export const placeFor = (place: Place | undefined, epoch: number): Place | undefined =>
  place && (place.bottom || place.epoch === epoch) ? place : undefined;
export interface Saved { draft?: string; place?: Place; opened?: string[] }

const STORE_KEY = 'termenbox_acp_view';
const MAX_KEPT = 30;
const MAX_DRAFT = 16 * 1024;

const store = (): Pick<Storage, 'getItem' | 'setItem'> | undefined => {
  try { return typeof localStorage === 'undefined' ? undefined : localStorage; } catch { return undefined; }
};

function read(storage = store()): Record<string, Saved> {
  try {
    const raw = JSON.parse(storage?.getItem(STORE_KEY) ?? '{}');
    return raw && typeof raw === 'object' && !Array.isArray(raw) ? raw : {};
  } catch { return {}; }
}

export function loadSaved(key: string, storage = store()): Saved {
  const s = read(storage)[key];
  if (!s || typeof s !== 'object') return {};
  const p = s.place;
  return {
    draft: typeof s.draft === 'string' ? s.draft.slice(0, MAX_DRAFT) : undefined,
    place: p && Number.isInteger(p.entry) && Number.isFinite(p.offset) ? { entry: p.entry, offset: p.offset, bottom: !!p.bottom, epoch: Number.isFinite(p.epoch) ? p.epoch : undefined } : undefined,
    opened: Array.isArray(s.opened) ? s.opened.filter((x) => typeof x === 'string').slice(0, 100) : undefined,
  };
}

/** Merges `patch` into the saved state of a session. Best effort: private mode or a full store just forgets. */
export function save(key: string, patch: Partial<Saved>, storage = store()): void {
  const all = read(storage);
  const merged: Saved = { ...all[key], ...patch };
  if (!merged.draft) delete merged.draft;
  delete all[key]; // newest last
  if (merged.draft || merged.place || merged.opened?.length) all[key] = merged;
  const kept = Object.fromEntries(Object.entries(all).slice(-MAX_KEPT));
  try { storage?.setItem(STORE_KEY, JSON.stringify(kept)); } catch { /* nothing to do */ }
}
