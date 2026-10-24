// Where the user's attention is in an agent's terminal, and what Ctrl+A and Ctrl+X mean there.
//
// An agent's prompt box is drawn by the agent, so the page cannot see where it is. What it can see is where the user
// last acted: typing goes to the prompt, and a click on the output above it is a step out of the prompt. In the prompt
// the agent's own keys are the user's (Ctrl+A and Ctrl+X pass through, and so does the wheel); out of it Ctrl+A selects
// the whole terminal, history included, and Ctrl+X copies a selection.

/** A prompt is a box of a few rows around the cursor; a click this many rows above the cursor, or higher, is on the output. */
export const PROMPT_ROWS = 8;

export type Target = 'prompt' | 'output';

/** Where a click on `row` puts the user's attention, with the cursor on `cursorRow` (both rows of the screen). */
export function clickTarget(row: number, cursorRow: number): Target {
  return row < cursorRow - PROMPT_ROWS ? 'output' : 'prompt';
}

export interface KeyInfo { key: string; code: string; ctrl: boolean; meta: boolean; shift: boolean; alt: boolean }
export type KeyAction = 'select-all' | 'cut' | 'pass';

/**
 * What the terminal does with Ctrl+A / Cmd+A and Ctrl+X / Cmd+X; `pass` leaves the key to the program. The physical key
 * counts too: on a non-Latin layout Ctrl+A arrives as another letter and only `code` says KeyA.
 */
export function keyAction(k: KeyInfo, target: Target, hasSelection: boolean): KeyAction {
  if (k.alt) return 'pass';
  const key = k.key.toLowerCase();
  const isA = k.code === 'KeyA' || key === 'a';
  const isX = k.code === 'KeyX' || key === 'x';
  if (isA) {
    // Cmd+A and Ctrl+Shift+A always mean the terminal; plain Ctrl+A only once the user has stepped out of the prompt.
    if (k.meta || (k.ctrl && k.shift)) return 'select-all';
    if (k.ctrl) return target === 'output' ? 'select-all' : 'pass';
  }
  if (isX && (k.ctrl || k.meta) && !k.shift) return target === 'output' && hasSelection ? 'cut' : 'pass';
  return 'pass';
}

/** Joins the parts of a `scrollback` answer in order; returns the whole text when the last one has arrived. */
export class ScrollbackParts {
  private parts: string[] = [];
  add(part: number, last: boolean, text: string): string | null {
    if (part === 0) this.parts = [];
    if (part !== this.parts.length) { this.parts = []; return null; } // a part went missing: start again from the next answer
    this.parts.push(text);
    if (!last) return null;
    const all = this.parts.join('');
    this.parts = [];
    return all;
  }
}
