// The four top-level destinations (decision 0101) and the address bar that mirrors them. The hash carries only the
// destination (`#/workspaces`); claim and server links (`#claim=`, `#server=`) are other hashes and are left alone.
import { writable, get } from 'svelte/store';

export const DESTINATIONS = [
  { id: 'home', label: 'Home' },
  { id: 'workspaces', label: 'Workspaces' },
  { id: 'activity', label: 'Activity' },
  { id: 'settings', label: 'Settings' },
] as const;
export type Destination = (typeof DESTINATIONS)[number]['id'];
/** The app opens on Home (decision 0100); the workspace last used is offered there as the Continue card. */
export const DEFAULT_DESTINATION: Destination = 'home';

export const isDestination = (v: unknown): v is Destination => DESTINATIONS.some((d) => d.id === v);

/** What the address bar said before the workspace rename: a bookmark or a history entry still lands on the right page. */
const OLD_ROUTES: Record<string, Destination> = { sessions: 'workspaces' };

/** `#/home` -> `home`; anything else (a claim link, an empty hash, an unknown name) -> null. */
export function parseRoute(hash: string): Destination | null {
  const m = /^#\/([a-z]+)\/?$/.exec(hash);
  if (!m) return null;
  return isDestination(m[1]) ? m[1] : (OLD_ROUTES[m[1]] ?? null);
}
export const routeHash = (d: Destination) => `#/${d}`;

/** A push's deep link (U11): `#/home?focus=<workspace key>`, the key percent-encoded. The key, or null for any other hash. */
export function parseFocusLink(hash: string): string | null {
  const m = /^#\/home\?focus=([^&]+)(?:&tab=[^&]+)?$/.exec(hash);
  if (!m) return null;
  try {
    const key = decodeURIComponent(m[1]);
    return key.length <= 256 ? key : null;
  } catch { return null; }
}

/** The tab a push deep link names (`&tab=<id>`), or null when it names only the workspace. */
export function parseFocusTab(hash: string): string | null {
  const m = /^#\/home\?focus=[^&]+&tab=([^&]+)$/.exec(hash);
  if (!m) return null;
  try {
    const tab = decodeURIComponent(m[1]);
    return tab.length <= 128 ? tab : null;
  } catch { return null; }
}

export const destination = writable<Destination>(DEFAULT_DESTINATION);

type Win = Pick<Window, 'location' | 'history' | 'addEventListener' | 'removeEventListener'>;

/** Moves to a destination and records it in the address bar, so Back returns to the previous one. */
export function navigate(d: Destination, win: Win | undefined = typeof window === 'undefined' ? undefined : window) {
  if (get(destination) !== d) destination.set(d);
  if (win && win.location.hash !== routeHash(d)) {
    try { win.history.pushState(null, '', `${win.location.pathname}${win.location.search}${routeHash(d)}`); } catch { /* sandboxed frame: the store still moves */ }
  }
}

/** Reads the address bar once, then follows Back/Forward. Returns the stop function. */
export function startRouter(win: Win | undefined = typeof window === 'undefined' ? undefined : window): () => void {
  if (!win) return () => {};
  const read = () => { const d = parseRoute(win.location.hash); if (d && d !== get(destination)) destination.set(d); };
  read();
  win.addEventListener('hashchange', read);
  win.addEventListener('popstate', read);
  return () => { win.removeEventListener('hashchange', read); win.removeEventListener('popstate', read); };
}
