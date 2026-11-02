// Soft-keyboard helpers: the key row's sequences, sticky Ctrl/Alt, and pinch-to-zoom font sizing.
import { joinRows, type Row } from './terminal/copyJoin';

export const KEYS = {
  esc: '\x1b',
  tab: '\t',
  up: '\x1b[A',
  down: '\x1b[B',
  right: '\x1b[C',
  left: '\x1b[D',
  home: '\x1b[H',
  end: '\x1b[F',
  ctrlC: '\x03',
  ctrlD: '\x04',
} as const;

export interface Mods {
  ctrl: boolean;
  alt: boolean;
  shift?: boolean;
}

/** Shift with a key-row sequence (xterm): Tab is back-tab, cursor keys carry modifier 2. Null when Shift changes nothing. */
export function shiftSeq(seq: string): string | null {
  if (seq === KEYS.tab) return '\x1b[Z';
  const m = /^\x1b\[([ABCDHF])$/.exec(seq);
  return m ? `\x1b[1;2${m[1]}` : null;
}

/**
 * Applies the sticky modifiers to what the soft keyboard just produced. Only a single typed character is
 * transformed (a paste or an escape sequence passes through); `used` tells the caller to clear the modifiers.
 */
export function applyMods(data: string, mods: Mods): { out: string; used: boolean } {
  if (!(mods.ctrl || mods.alt || mods.shift) || [...data].length !== 1) return { out: data, used: false };
  let ch = mods.shift ? data.toUpperCase() : data;
  if (mods.ctrl) {
    const code = data.toUpperCase().charCodeAt(0);
    if (code >= 0x40 && code <= 0x5f) ch = String.fromCharCode(code & 0x1f); // A-Z @ [ \ ] ^ _
    else if (data === ' ') ch = '\0';
    else if (data === '?') ch = '\x7f';
    else ch = data;
  }
  return { out: mods.alt ? '\x1b' + ch : ch, used: true };
}

export const FONT_MIN = 9;
export const FONT_MAX = 28;
export const FONT_DEFAULT = 14;

export function clampFont(size: number): number {
  return Math.min(FONT_MAX, Math.max(FONT_MIN, Math.round(size)));
}

/** New font size for a pinch that scaled the finger distance by `ratio` (>1 = fingers apart = bigger). */
export function pinchFont(start: number, ratio: number): number {
  return clampFont(start * ratio);
}

/** The Fn panel: navigation and function keys (xterm sequences), then symbols that are awkward on a phone keyboard. */
export const FN_KEYS: [string, string][] = [
  ['PgUp', '\x1b[5~'],
  ['PgDn', '\x1b[6~'],
  ['Ins', '\x1b[2~'],
  ['Del', '\x1b[3~'],
  ['F1', '\x1bOP'],
  ['F2', '\x1bOQ'],
  ['F3', '\x1bOR'],
  ['F4', '\x1bOS'],
  ['F5', '\x1b[15~'],
  ['F6', '\x1b[17~'],
  ['F7', '\x1b[18~'],
  ['F8', '\x1b[19~'],
  ['F9', '\x1b[20~'],
  ['F10', '\x1b[21~'],
  ['F11', '\x1b[23~'],
  ['F12', '\x1b[24~'],
];

export const SYMBOLS = ['{', '}', '[', ']', '(', ')', '<', '>', '\\', '`', '$', '&', '*', ';', '"', "'", '#', '@', '!', '=', '+', '_', '%', '^', ':', '?'];

/** The last `maxLines` lines of a terminal buffer as plain text, rows of one line joined, trailing blank lines dropped (for the Select view). */
export function bufferText(getRow: (i: number) => Row | undefined, length: number, cols: number, maxLines = 1000): string {
  const rows: Row[] = [];
  for (let i = Math.max(0, length - maxLines); i < length; i++) rows.push(getRow(i) ?? { text: '' });
  return joinRows(rows, cols);
}

/** tmux writes its prefix as `C-b`, `M-a`, a bare character, or `None`; this is what the terminal receives for it. */
export function prefixBytes(notation: string): string {
  const p = notation.trim();
  if (!p || p === 'None') return '';
  const named: Record<string, string> = { Space: ' ', Tab: '\t', Enter: '\r', Escape: '\x1b' };
  const key = (k: string) => (named[k] ?? (k.length === 1 ? k : ''));
  if (p.startsWith('C-M-') || p.startsWith('M-C-')) {
    const k = key(p.slice(4));
    return k ? '\x1b' + applyMods(k, { ctrl: true, alt: false }).out : '';
  }
  if (p.startsWith('C-')) {
    const k = key(p.slice(2));
    return k ? applyMods(k, { ctrl: true, alt: false }).out : '';
  }
  if (p.startsWith('M-')) {
    const k = key(p.slice(2));
    return k ? '\x1b' + k : '';
  }
  return key(p);
}

/** The prefix as it is printed on a keycap: `C-b` is `^B`, `M-a` stays `M-a`, `None` and unknown notation are empty. */
export function prefixLabel(notation: string): string {
  const p = notation.trim();
  if (!p || p === 'None') return '';
  const k = (name: string) => (name.length === 1 ? name.toUpperCase() : name);
  if (p.startsWith('C-M-') || p.startsWith('M-C-')) return '^M-' + k(p.slice(4));
  if (p.startsWith('C-')) return '^' + k(p.slice(2));
  return p;
}

/**
 * What the New line key sends. Claude Code takes Alt+Enter (ESC CR); Codex takes Ctrl+J (LF). Anything else gets
 * Alt+Enter, which most terminal UIs treat as "line break, do not submit".
 */
export function newlineKeys(agent?: string): { seq: string; hint: string } {
  if (agent === 'codex') return { seq: '\n', hint: 'Ctrl+J' };
  return { seq: '\x1b\r', hint: 'Alt+Enter' };
}

export type KeyProfile = 'shell' | 'tmux';

/** After the prefix: splits, copy mode and detach. Window moves live in the windows dropdown. */
export const TMUX_COMBOS: [label: string, key: string][] = [
  ['Split ─', '"'],
  ['Split │', '%'],
  ['Copy mode', '['],
  ['Detach', 'd'],
];

// ---- The key catalogue (U07c): every key the row and the Fn panel offer, with its accessible name and what it sends.

export type KeyGroup = 'Row' | 'Control' | 'Navigation' | 'Function' | 'Symbols';
/** `send` writes bytes; `mod` is a sticky modifier or toggle with a pressed state; `action` runs something in the terminal. */
export type KeyKind = 'send' | 'mod' | 'action';
export interface KeyDef {
  id: string;
  /** What is printed on the keycap (some are replaced at run time: the prefix, the dictation state). */
  label: string;
  /** The accessible name. */
  name: string;
  group: KeyGroup;
  kind: KeyKind;
  /** The bytes a `send` key writes. */
  seq?: string;
  /** Smaller type on the keycap (the text-size and dictation-language keys). */
  small?: boolean;
  /** Only offered for this profile (the tmux prefix). */
  profile?: KeyProfile;
  /** Only offered when the browser can do it (dictation). */
  needs?: 'voice';
}

const send = (id: string, label: string, name: string, seq: string, group: KeyGroup): KeyDef => ({ id, label, name, group, kind: 'send', seq });
const SYMBOL_CHARS = [...SYMBOLS, '|', '~', '-'];

export const KEY_CATALOGUE: readonly KeyDef[] = [
  { id: 'prefix', label: 'Prefix', name: 'tmux prefix', group: 'Row', kind: 'mod', profile: 'tmux' },
  send('slash', '/', 'Slash', '/', 'Row'),
  send('ctrlC', '^C', 'Control C', KEYS.ctrlC, 'Control'),
  send('esc', '⎋', 'Escape', KEYS.esc, 'Control'),
  { id: 'newline', label: '↵', name: 'New line without sending', group: 'Row', kind: 'action' },
  { id: 'ctrl', label: '⌃', name: 'Ctrl', group: 'Control', kind: 'mod' },
  { id: 'alt', label: '⌥', name: 'Alt', group: 'Control', kind: 'mod' },
  { id: 'shift', label: '⇧', name: 'Shift', group: 'Control', kind: 'mod' },
  send('tab', '⇥', 'Tab', KEYS.tab, 'Control'),
  { id: 'fn', label: 'Fn', name: 'Fn', group: 'Row', kind: 'mod' },
  send('up', '↑', 'Arrow up', KEYS.up, 'Navigation'),
  send('down', '↓', 'Arrow down', KEYS.down, 'Navigation'),
  { id: 'keyboard', label: '⌨', name: 'Show or hide the keyboard', group: 'Row', kind: 'action' },
  { id: 'voice', label: '🎤', name: 'Dictate', group: 'Row', kind: 'mod', needs: 'voice' },
  { id: 'voicelang', label: '🌐', name: 'Dictation language', group: 'Row', kind: 'action', small: true, needs: 'voice' },
  { id: 'fontDown', label: 'A−', name: 'Smaller text', group: 'Row', kind: 'action', small: true },
  { id: 'fontUp', label: 'A+', name: 'Larger text', group: 'Row', kind: 'action', small: true },
  send('left', '←', 'Arrow left', KEYS.left, 'Navigation'),
  send('right', '→', 'Arrow right', KEYS.right, 'Navigation'),
  send('home', 'Home', 'Home', KEYS.home, 'Navigation'),
  send('end', 'End', 'End', KEYS.end, 'Navigation'),
  send('pgup', 'PgUp', 'Page up', '\x1b[5~', 'Navigation'),
  send('pgdn', 'PgDn', 'Page down', '\x1b[6~', 'Navigation'),
  send('ins', 'Ins', 'Insert', '\x1b[2~', 'Navigation'),
  send('del', 'Del', 'Delete', '\x1b[3~', 'Navigation'),
  send('enter', '⏎', 'Enter', '\r', 'Control'),
  send('backspace', '⌫', 'Backspace', '\x7f', 'Control'),
  send('space', 'space', 'Space', ' ', 'Control'),
  send('ctrlD', '^D', 'Control D', KEYS.ctrlD, 'Control'),
  ...FN_KEYS.filter(([l]) => /^F\d+$/.test(l)).map(([l, seq]) => send(l.toLowerCase(), l, l, seq, 'Function')),
  ...SYMBOL_CHARS.map((c) => send(`sym-${c.codePointAt(0)!.toString(16)}`, c, `Symbol ${c}`, c, 'Symbols')),
];

export const KEY_BY_ID: ReadonlyMap<string, KeyDef> = new Map(KEY_CATALOGUE.map((k) => [k.id, k]));
