// The command palette's entries, its search, and the app's keyboard shortcuts (decision 0101, item 8 of U04).
import { writable } from 'svelte/store';
import { DESTINATIONS } from './router';
import type { AgentMap } from './agents';
import type { HubServer, HubWorkspace } from './hub';
import { fleetOf } from './navlist';
import { STATES, type FleetState } from './ds/states';

export type EntryKind = 'destination' | 'action' | 'server' | 'workspace';
export interface PaletteEntry { id: string; kind: EntryKind; label: string; detail?: string; state?: FleetState; agent?: string }

export interface PaletteInput { servers: HubServer[]; workspaces: HubWorkspace[]; agents: AgentMap; wide: boolean; sidebarOpen: boolean; splitOpen: boolean; canCreate: boolean }

/** Ids: `dest:<name>`, `action:<name>[:<server>]`, `server:<id>`, `workspace:<key>`. */
export function buildEntries(i: PaletteInput): PaletteEntry[] {
  const out: PaletteEntry[] = DESTINATIONS.map((d) => ({ id: `dest:${d.id}`, kind: 'destination', label: d.label, detail: 'Go to' }));
  if (i.canCreate) out.push({ id: 'action:new-workspace', kind: 'action', label: 'New workspace', detail: 'Action' });
  // The same as the Alt keys, for where those keys belong to the terminal or a field (the terminal keeps the focus when a workspace opens).
  out.push({ id: 'action:next-needs-you', kind: 'action', label: 'Open the next workspace that needs you', detail: 'Action' });
  out.push({ id: 'action:approve-decision', kind: 'action', label: 'Answer Yes to the open decision', detail: 'Action' });
  out.push({ id: 'action:deny-decision', kind: 'action', label: 'Answer No to the open decision', detail: 'Action' });
  out.push({ id: 'action:focus-composer', kind: 'action', label: 'Focus the reply composer', detail: 'Action' });
  for (const s of i.servers) {
    const name = s.server.server_name || 'Server';
    if (s.state === 'paused') out.push({ id: `action:resume:${s.id}`, kind: 'action', label: `Resume server ${name}`, detail: 'Action' });
    else out.push({ id: `action:pause:${s.id}`, kind: 'action', label: `Pause server ${name}`, detail: 'Action' });
  }
  if (i.wide) {
    out.push({ id: 'action:toggle-sidebar', kind: 'action', label: i.sidebarOpen ? 'Hide the sidebar' : 'Show the sidebar', detail: 'Action' });
    out.push({ id: 'action:toggle-split', kind: 'action', label: i.splitOpen ? 'Hide the terminal beside the decision' : 'Show the terminal beside the decision', detail: 'Action' });
  }
  out.push({ id: 'action:shortcuts', kind: 'action', label: 'Keyboard shortcuts', detail: 'Action' });
  for (const s of i.servers) out.push({ id: `server:${s.id}`, kind: 'server', label: s.server.server_name || 'Server', detail: `Server, ${s.state}` });
  for (const s of i.workspaces) {
    const agent = i.agents[s.key];
    const state = fleetOf(agent);
    out.push({ id: `workspace:${s.key}`, kind: 'workspace', label: s.name, detail: `${s.serverName || 'Server'} · ${STATES[state].label}`, state, agent: agent?.agent });
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
export type ShortcutId = 'palette' | 'approve-decision' | 'deny-decision' | 'next-needs-you' | 'focus-composer' | 'toggle-panes';
export interface Shortcut { id: ShortcutId; mac: string; other: string; label: string; scope: string }
/**
 * Only registered shortcuts are intercepted; every other key goes to the page and the terminal untouched.
 * Ctrl/Cmd+K: on macOS Cmd is never sent to a terminal, so nothing is lost. On Windows and Linux it takes Ctrl+K
 * (readline "kill to end of line") from the terminal, and the page prevents the browser's own use of it (Chrome and
 * Firefox focus their search box). That is why shortcuts can be turned off (Settings).
 * The rest are Alt (Option) plus a letter that no browser uses (not Ctrl+Shift+N/P/O/A/D, Ctrl+T/W/N or Alt+F/E/D, which
 * the browser keeps). They act only when nothing is being typed into: a text field, the composer and the terminal keep
 * every key, so a reply can never answer a decision by accident and a shell never loses a key.
 */
export const SHORTCUTS: Shortcut[] = [
  { id: 'palette', mac: '⌘K', other: 'Ctrl+K', label: 'Open or close the command palette', scope: 'Everywhere in the app, including while the terminal has focus' },
  { id: 'approve-decision', mac: '⌥Y', other: 'Alt+Y', label: 'Answer Yes to the open decision', scope: 'When the decision is on screen with a plain Yes option. From the terminal or a field: Ctrl+K, then “Answer Yes”' },
  { id: 'deny-decision', mac: '⌥N', other: 'Alt+N', label: 'Answer No to the open decision', scope: 'When the decision is on screen with a plain No option; never “don’t ask again”. From the terminal or a field: Ctrl+K' },
  { id: 'next-needs-you', mac: '⌥J', other: 'Alt+J', label: 'Open the next workspace that needs you', scope: 'When no field or terminal has focus' },
  { id: 'focus-composer', mac: '⌥C', other: 'Alt+C', label: 'Focus the reply composer', scope: 'When no field or terminal has focus' },
  { id: 'toggle-panes', mac: '⌥P', other: 'Alt+P', label: 'Show or hide the terminal beside the decision', scope: 'On the desktop, in a workspace' },
];
export const PALETTE_KEYS: { keys: string; label: string }[] = [
  { keys: '↑ ↓', label: 'Move through the results' },
  { keys: 'Enter', label: 'Open the highlighted result' },
  { keys: 'Esc', label: 'Close and return to where you were' },
];
/** The splitters between panes. */
export const SPLITTER_KEYS: { keys: string; label: string }[] = [
  { keys: '← →', label: 'Move the divider (Shift for a larger step)' },
  { keys: 'Home End', label: 'Smallest or largest width' },
  { keys: 'Enter', label: 'Reset the width (a double click does too)' },
];

export interface KeyLike { key: string; code?: string; ctrlKey: boolean; metaKey: boolean; altKey: boolean; shiftKey: boolean; repeat?: boolean; isComposing?: boolean; keyCode?: number; getModifierState?: (k: string) => boolean }

export const isMac = (nav: Pick<Navigator, 'platform' | 'userAgent'> | undefined = typeof navigator === 'undefined' ? undefined : navigator) =>
  /Mac|iPhone|iPad/.test(nav?.platform || nav?.userAgent || '');

const ALT_KEYS: Record<string, ShortcutId> = { y: 'approve-decision', n: 'deny-decision', j: 'next-needs-you', c: 'focus-composer', p: 'toggle-panes' };

/** The letter a key stands for, whatever its case. With Option held a Mac types another character (⌥Y is ¥), so the physical key decides then. */
function letterOf(e: KeyLike): string {
  if (/^[a-z]$/i.test(e.key)) return e.key.toLowerCase();
  const m = /^Key([A-Z])$/.exec(e.code ?? '');
  return m ? m[1].toLowerCase() : '';
}

export function matchShortcut(e: KeyLike, mac: boolean): ShortcutId | null {
  if (e.isComposing || e.keyCode === 229) return null; // an IME is composing: the key belongs to it
  if (e.getModifierState?.('AltGraph')) return null; // AltGr (Ctrl+Alt on Windows) types characters on many layouts
  if (e.shiftKey) return null;
  const letter = letterOf(e);
  if (!e.altKey) {
    const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
    return mod && letter === 'k' ? 'palette' : null;
  }
  if (e.ctrlKey || e.metaKey || e.repeat) return null;
  return ALT_KEYS[letter] ?? null;
}

/** Where a key is being typed: a text field, the composer, a menu search or the terminal. */
const TYPING = 'input, textarea, select, [contenteditable=""], [contenteditable="true"], [role=textbox], [role=combobox], [role=searchbox], .xterm';
export function isTyping(el: Element | null): boolean {
  return !!el?.closest?.(TYPING);
}
/** The palette works everywhere; the rest only when nothing is being typed into and no dialog is open. */
export function shortcutAllowed(id: ShortcutId, focus: Element | null, doc: Pick<Document, 'querySelector'> = document): boolean {
  if (id === 'palette') return true;
  return !isTyping(focus) && !doc.querySelector('[role=dialog], dialog[open]');
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
