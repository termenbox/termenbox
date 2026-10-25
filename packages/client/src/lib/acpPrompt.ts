// The prompt line of an ACP workspace (U07b): slash-command completion from the agent's own list, prompt history, and what
// each key of the key row does here. Pure, so the rules are tested without a browser.
import type { Info } from './acpStream';

export type Command = Info['commands'][number];

export interface Completion { query: string; items: Command[] }

/** The commands to offer while the cursor is in the first word of a draft that starts with `/`; none otherwise. Names
 *  that start with what was typed come first, then names that contain it. */
export function completions(draft: string, cursor: number, commands: Command[], limit = 50): Completion | null {
  if (!draft.startsWith('/')) return null;
  const word = /^\/(\S*)/.exec(draft)!;
  if (cursor > word[0].length) return null;
  const q = word[1].toLowerCase();
  const starts = commands.filter((c) => c.name.toLowerCase().startsWith(q));
  const has = commands.filter((c) => !c.name.toLowerCase().startsWith(q) && c.name.toLowerCase().includes(q));
  const items = [...starts, ...has].slice(0, limit);
  return items.length ? { query: word[1], items } : null;
}

/** The draft after choosing `c`: `/name ` followed by whatever argument text was already there. The cursor goes to the end. */
export function complete(draft: string, c: Command): { text: string; cursor: number } {
  const rest = draft.replace(/^\/\S*/, '').replace(/^\s+/, '');
  const text = `/${c.name} ${rest}`;
  return { text, cursor: c.name.length + 2 };
}

/** Whether Enter should choose the highlighted command instead of sending: the draft is a half-typed name. */
export function enterCompletes(draft: string, item: Command | undefined): boolean {
  return !!item && /^\/\S*$/.test(draft) && draft !== `/${item.name}`;
}

/** Earlier prompts, newest last, without repeats of the last one; `↑` and `↓` walk them like a shell. */
export class History {
  private i = -1;
  private stash = '';
  constructor(private items: string[]) {}
  /** Starts again from the end (a new draft was typed or sent). */
  reset(items?: string[]) { if (items) this.items = items; this.i = -1; }
  previous(current: string): string | null {
    if (!this.items.length || this.i === 0) return null;
    if (this.i === -1) { this.stash = current; this.i = this.items.length; }
    this.i--;
    return this.items[this.i];
  }
  next(): string | null {
    if (this.i === -1) return null;
    this.i++;
    if (this.i >= this.items.length) { this.i = -1; return this.stash; }
    return this.items[this.i];
  }
  get browsing() { return this.i !== -1; }
}

export const promptsOf = (entries: { kind: string; text?: string }[], max = 50): string[] =>
  entries.filter((e) => e.kind === 'user' && e.text).map((e) => e.text as string).filter((t, i, a) => t !== a[i - 1]).slice(-max);

export type KeyEffect =
  | { do: 'interrupt' }
  | { do: 'close-popup' }
  | { do: 'accept' }
  | { do: 'move'; by: 1 | -1 }
  | { do: 'history'; by: 1 | -1 }
  | { do: 'clear' }
  | { do: 'insert'; text: string }
  | { do: 'cursor'; to: 'left' | 'right' | 'home' | 'end' }
  | { do: 'none' };

export interface KeyState { working: boolean; popup: boolean; draft: string; cursor: number }

/** What a key-row key (the byte sequences the terminal's row sends) does at the prompt. Esc and ^C interrupt a turn
 *  that is running; with a popup open Esc closes it first. */
export function keyEffect(seq: string, st: KeyState): KeyEffect {
  switch (seq) {
    case '\x1b': return st.popup ? { do: 'close-popup' } : st.working ? { do: 'interrupt' } : { do: 'none' };
    case '\x03': return st.working ? { do: 'interrupt' } : st.draft ? { do: 'clear' } : { do: 'none' };
    case '\t': return st.popup ? { do: 'accept' } : { do: 'none' };
    case '\x1b[A': return st.popup ? { do: 'move', by: -1 } : { do: 'history', by: -1 };
    case '\x1b[B': return st.popup ? { do: 'move', by: 1 } : { do: 'history', by: 1 };
    case '\x1b[D': return { do: 'cursor', to: 'left' };
    case '\x1b[C': return { do: 'cursor', to: 'right' };
    case '\x1b[H': return { do: 'cursor', to: 'home' };
    case '\x1b[F': return { do: 'cursor', to: 'end' };
    default: return seq.length > 0 && !/[\u0000-\u001f\u007f]/.test(seq) ? { do: 'insert', text: seq } : { do: 'none' };
  }
}

/** The keys of the row that mean something at the prompt: few enough that each is wide enough to hit on a 360 px phone. */
export const PROMPT_KEYS: readonly string[] = ['esc', 'ctrlC', 'tab', 'slash', 'up', 'down', 'newline', 'voice', 'voicelang'];

/** Inserts `text` at the cursor. */
export function insertAt(draft: string, cursor: number, text: string): { text: string; cursor: number } {
  const c = Math.max(0, Math.min(cursor, draft.length));
  return { text: draft.slice(0, c) + text + draft.slice(c), cursor: c + text.length };
}
