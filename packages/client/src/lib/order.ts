// The order workspaces are listed in, everywhere (list, sheet, sidebar, tab strip): the user's own, kept per device and per
// server in localStorage. A workspace's state never moves it; "needs you" is shown by badges and counts instead. A new
// workspace appends to the end of its server, one that is gone is pruned. No protocol message is involved; an order
// stored on the server so that it follows the user across devices would be a follow-up.
import { writable } from 'svelte/store';

const PREFIX = 'termenbox_workspace_order:';
const store = () => typeof localStorage === 'undefined' ? undefined : localStorage;

export function loadOrder(serverId: string, storage: Pick<Storage, 'getItem'> | undefined = store()): string[] {
  try {
    const v = JSON.parse(storage?.getItem(PREFIX + serverId) ?? '[]');
    return Array.isArray(v) ? [...new Set(v.filter((x): x is string => typeof x === 'string'))] : [];
  } catch { return []; }
}
export function saveOrder(serverId: string, keys: string[], storage: Pick<Storage, 'setItem' | 'removeItem'> | undefined = store()) {
  try { if (keys.length) storage?.setItem(PREFIX + serverId, JSON.stringify(keys)); else storage?.removeItem(PREFIX + serverId); } catch { /* Private mode: kept in memory. */ }
}

export type Move = 'up' | 'down' | 'top' | 'bottom';
export type Place = Move | { before: string } | { after: string };

/** `keys` with `key` moved; the same array when nothing changes (not there, already there, or the target is itself). */
export function reorder(keys: string[], key: string, to: Place): string[] {
  const from = keys.indexOf(key);
  if (from < 0) return keys;
  const rest = keys.filter((k) => k !== key);
  let at: number;
  if (to === 'up') at = Math.max(0, from - 1);
  else if (to === 'down') at = Math.min(rest.length, from + 1);
  else if (to === 'top') at = 0;
  else if (to === 'bottom') at = rest.length;
  else {
    const target = rest.indexOf('before' in to ? to.before : to.after);
    if (target < 0) return keys;
    at = 'before' in to ? target : target + 1;
  }
  if (at === from) return keys;
  return [...rest.slice(0, at), key, ...rest.slice(at)];
}

/** Servers in the order they first appear; inside a server, the saved order, then whatever is new in the order given. */
export function applyOrder<T extends { key: string; serverId: string }>(workspaces: T[], saved: (serverId: string) => string[]): T[] {
  const servers = [...new Set(workspaces.map((w) => w.serverId))];
  return servers.flatMap((id) => {
    const mine = workspaces.filter((w) => w.serverId === id);
    const rank = new Map(saved(id).map((k, i) => [k, i]));
    return mine.map((w, i) => ({ w, i })).sort((a, b) => (rank.get(a.w.key) ?? Infinity) - (rank.get(b.w.key) ?? Infinity) || a.i - b.i).map((x) => x.w);
  });
}

// One copy per server in memory, read from the device when first needed.
const cache = new Map<string, string[]>();
const version = writable(0);
/** Changes whenever an order does, so the hub lists again. */
export const orderVersion = { subscribe: version.subscribe };
const saved = (serverId: string) => {
  let v = cache.get(serverId);
  if (!v) cache.set(serverId, v = loadOrder(serverId));
  return v;
};

/** The workspaces in the order shown. The server's own order wins when it sent one (decision 0149); an older server
 *  falls back to the device-local order. A server that lists workspaces is the truth about which exist: the others are
 *  pruned from the local copy. */
export function orderWorkspaces<T extends { key: string; serverId: string; order?: number }>(workspaces: T[]): T[] {
  const out = [...new Set(workspaces.map((w) => w.serverId))].flatMap((id) => {
    const mine = workspaces.filter((w) => w.serverId === id);
    if (mine.length && mine.every((w) => typeof w.order === 'number')) {
      return [...mine].sort((a, b) => (a.order! - b.order!) || a.key.localeCompare(b.key));
    }
    const kept = new Set(mine.map((w) => w.key));
    const local = saved(id).filter((k) => kept.has(k));
    if (local.length !== saved(id).length) { cache.set(id, local); saveOrder(id, local); }
    return applyOrder(mine, saved);
  });
  return out;
}

export interface Placement { position: number; count: number }
/** Where a workspace stands among those of its server (1-based), from the list as shown. */
export function placementOf(workspaces: { key: string; serverId: string }[], key: string): Placement | null {
  const me = workspaces.find((w) => w.key === key);
  if (!me) return null;
  const mine = workspaces.filter((w) => w.serverId === me.serverId);
  return { position: mine.findIndex((w) => w.key === key) + 1, count: mine.length };
}

/**
 * Moves a workspace inside its server (an order is per server) and keeps the result. `workspaces` is the list as shown, so
 * what is saved is the full current order of that server: the new and the removed ones are settled by the same step.
 * Returns the new place, or null when nothing moved.
 */
export function moveWorkspace(workspaces: { key: string; serverId: string }[], key: string, to: Place): Placement | null {
  const me = workspaces.find((w) => w.key === key);
  if (!me) return null;
  const shown = workspaces.filter((w) => w.serverId === me.serverId).map((w) => w.key);
  const next = reorder(shown, key, to);
  if (next === shown) return null;
  cache.set(me.serverId, next);
  saveOrder(me.serverId, next);
  version.update((n) => n + 1);
  return { position: next.indexOf(key) + 1, count: next.length };
}

/** The index `key` takes among its server's workspaces once it is lifted out and dropped beside `target`; null when either is unknown. */
export function dropIndex(workspaces: { key: string; serverId: string }[], key: string, target: { key: string; side: 'before' | 'after' }): number | null {
  const me = workspaces.find((w) => w.key === key);
  const there = workspaces.find((w) => w.key === target.key);
  if (!me || !there) return null;
  const rest = workspaces.filter((w) => w.serverId === there.serverId && w.key !== key).map((w) => w.key);
  const at = rest.indexOf(target.key);
  return at < 0 ? null : at + (target.side === 'after' ? 1 : 0);
}

/** The words for a screen reader. */
export const movedText = (name: string, p: Placement) => `${name} moved to position ${p.position} of ${p.count}`;

/** Test hook: forget what is held in memory. */
export function forgetOrders() { cache.clear(); }
