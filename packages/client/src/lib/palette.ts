// The command palette's entries, its search, and the app's keyboard shortcuts (decision 0101, item 8 of U04).
import { writable } from 'svelte/store';
import { DESTINATIONS } from './router';
import type { AgentMap } from './agents';
import type { HubServer, HubSession } from './hub';
import { fleetOf } from './navlist';
import { STATES, type FleetState } from './ds/states';

export type EntryKind = 'destination' | 'action' | 'server' | 'session';
export interface PaletteEntry { id: string; kind: EntryKind; label: string; detail?: string; state?: FleetState; agent?: string }

export interface PaletteInput { servers: HubServer[]; sessions: HubSession[]; agents: AgentMap; wide: boolean; rightOpen: boolean; canCreate: boolean }

/** Ids: `dest:<name>`, `action:<name>[:<server>]`, `server:<id>`, `session:<key>`. */
export function buildEntries(i: PaletteInput): PaletteEntry[] {
  const out: PaletteEntry[] = DESTINATIONS.map((d) => ({ id: `dest:${d.id}`, kind: 'destination', label: d.label, detail: 'Go to' }));
  if (i.canCreate) out.push({ id: 'action:new-session', kind: 'action', label: 'New session', detail: 'Action' });
  for (const s of i.servers) {
    const name = s.server.server_name || 'Server';
    if (s.state === 'paused') out.push({ id: `action:resume:${s.id}`, kind: 'action', label: `Resume server ${name}`, detail: 'Action' });
    else out.push({ id: `action:pause:${s.id}`, kind: 'action', label: `Pause server ${name}`, detail: 'Action' });
  }
  if (i.wide) out.push({ id: 'action:toggle-right', kind: 'action', label: i.rightOpen ? 'Hide the right pane' : 'Show the right pane', detail: 'Action' });
  out.push({ id: 'action:shortcuts', kind: 'action', label: 'Keyboard shortcuts', detail: 'Action' });
  for (const s of i.servers) out.push({ id: `server:${s.id}`, kind: 'server', label: s.server.server_name || 'Server', detail: `Server, ${s.state}` });
  for (const s of i.sessions) {
    const agent = i.agents[s.key];
    const state = fleetOf(agent);
    out.push({ id: `session:${s.key}`, kind: 'session', label: s.name, detail: `${s.serverName || 'Server'} · ${STATES[state].label}`, state, agent: agent?.agent });
  }
  return out;
}

const norm = (s: string) => s.toLowerCase();
const wordStart = (text: string, token: string) => text.split(/[\s\-_/.·]+/).some((w) => w.startsWith(token));
function isSubsequence(text: string, token: string) { let i = 0; for (const c of text) if (c === token[i]) i++; return i === token.length; }

/** 0 = no match. Every word typed must match the label or the detail; a label that starts with it scores highest. */
export function scoreEntry(e: PaletteEntry, query: string): number {
  const tokens = norm(query).split(/\s+/).filter(Boolean);
  if (!tokens.length) return 1;
  const label = norm(e.label), detail = norm(e.detail ?? '');
  let total = 0;
  for (const t of tokens) {
    let s = 0;
    if (label === t) s = 120;
    else if (label.startsWith(t)) s = 100;
    else if (wordStart(label, t)) s = 80;
    else if (label.includes(t)) s = 60;
    else if (wordStart(detail, t)) s = 30;
    else if (detail.includes(t)) s = 20;
    else if (isSubsequence(label, t)) s = 10;
    if (!s) return 0;
    total += s;
  }
  return total;
}

/** Best first; equal scores keep the order the entries were built in. */
export function searchEntries(entries: PaletteEntry[], query: string, limit = 50): PaletteEntry[] {
  return entries
    .map((e, i) => ({ e, i, s: scoreEntry(e, query) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s || a.i - b.i)
    .slice(0, limit)
    .map((x) => x.e);
}

// --- shortcuts ---------------------------------------------------------------------------------------------------
export interface Shortcut { id: 'palette'; mac: string; other: string; label: string; scope: string }
/**
 * Only registered shortcuts are intercepted; every other key goes to the page and the terminal untouched.
 * Ctrl/Cmd+K: on macOS Cmd is never sent to a terminal, so nothing is lost. On Windows and Linux it takes Ctrl+K
 * (readline "kill to end of line") from the terminal, and the page prevents the browser's own use of it (Chrome and
 * Firefox focus their search box). That is why shortcuts can be turned off (Settings).
 */
export const SHORTCUTS: Shortcut[] = [
  { id: 'palette', mac: '⌘K', other: 'Ctrl+K', label: 'Open or close the command palette', scope: 'Everywhere in the app, including while the terminal has focus' },
  { id: 'focus-composer', mac: '⌘⇧O', other: 'Ctrl+Shift+O', label: 'Focus the composer', scope: 'In the decision view, when no field is being edited' },
  { id: 'approve-decision', mac: '⌘⇧A', other: 'Ctrl+Shift+A', label: 'Answer with the first option', scope: 'When a decision card shows options' },
  { id: 'deny-decision', mac: '⌘⇧D', other: 'Ctrl+Shift+D', label: 'Answer with the second option', scope: 'When a decision card shows two or more options' },
  { id: 'next-needs-you', mac: '⌘⇧N', other: 'Ctrl+Shift+N', label: 'Open the next session that needs you', scope: 'Everywhere' },
  { id: 'toggle-panes', mac: '⌘⇧P', other: 'Ctrl+Shift+P', label: 'Show or hide the right pane', scope: 'On the desktop' },
];
export const PALETTE_KEYS: { keys: string; label: string }[] = [
  { keys: '↑ ↓', label: 'Move through the results' },
  { keys: 'Enter', label: 'Open the highlighted result' },
  { keys: 'Esc', label: 'Close and return to where you were' },
];

export interface KeyLike { key: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean; isComposing?: boolean; keyCode?: number; getModifierState?: (k: string) => boolean }

export const isMac = (nav: Pick<Navigator, 'platform' | 'userAgent'> | undefined = typeof navigator === 'undefined' ? undefined : navigator) =>
  /Mac|iPhone|iPad/.test(nav?.platform || nav?.userAgent || '');

export function matchShortcut(e: KeyLike, mac: boolean): Shortcut['id'] | null {
  if (e.isComposing || e.keyCode === 229) return null; // an IME is composing: the key belongs to it
  if (e.getModifierState?.('AltGraph')) return null; // AltGr (Ctrl+Alt on Windows) types characters on many layouts
  if (e.altKey || e.shiftKey) return null;
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  return mod && e.key.toLowerCase() === 'k' ? 'palette' : null;
}

const KEY = 'termenbox_shortcuts';
export function loadShortcutsEnabled(store: Pick<Storage, 'getItem'> | undefined = typeof localStorage === 'undefined' ? undefined : localStorage): boolean {
  try { return store?.getItem(KEY) !== 'off'; } catch { return true; }
}
export function saveShortcutsEnabled(on: boolean, store: Pick<Storage, 'setItem'> | undefined = typeof localStorage === 'undefined' ? undefined : localStorage) {
  try { store?.setItem(KEY, on ? 'on' : 'off'); } catch { /* private mode: kept for this visit */ }
}

/** Shared by the frame (which listens) and Settings (which switches it). */
export const shortcutsOn = writable(loadShortcutsEnabled());
export function setShortcuts(on: boolean) { shortcutsOn.set(on); saveShortcutsEnabled(on); }
