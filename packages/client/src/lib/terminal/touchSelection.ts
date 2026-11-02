// Touch text selection for the terminal: long-press a word, drag to extend, drag a handle to adjust an end.
// The controller owns the gesture state machine; the component only draws what `onChange` reports and forwards
// pointer events. Everything the browser provides is injected, so the logic runs under test with a fake
// terminal and fake timers.
import { selectedText, type CopyTerm } from './copyJoin';
import { cellAt, cellBox, ordered, selectionArgs, wordRange, type Cell, type Rect } from '../selection';

export interface TermLike extends CopyTerm {
  cols: number;
  rows: number;
  buffer: { active: { viewportY: number; getLine(i: number): { isWrapped: boolean; translateToString(trim?: boolean): string } | undefined } };
  select(col: number, row: number, length: number): void;
  clearSelection(): void;
}

export interface Selection {
  a: Cell;
  b: Cell;
  /** The finger that started the selection is still down. */
  dragging: boolean;
}

export interface SelectionDeps {
  term: () => TermLike | null;
  /** The terminal's text area on screen. */
  screenRect: () => Rect | null;
  /** The element the handles and bar are positioned against. */
  hostRect: () => Rect | null;
  onChange: (sel: Selection | null) => void;
  vibrate?: () => void;
  setTimer?: (fn: () => void, ms: number) => unknown;
  clearTimer?: (id: unknown) => void;
  holdMs?: number;
  slopPx?: number;
}

type End = 'a' | 'b';

export class TouchSelection {
  private sel: Selection | null = null;
  private dragEnd: End | null = null;
  private timer: unknown = null;
  private origin = { x: 0, y: 0 };
  private readonly holdMs: number;
  private readonly slopPx: number;
  private readonly setTimer: (fn: () => void, ms: number) => unknown;
  private readonly clearTimer: (id: unknown) => void;

  constructor(private deps: SelectionDeps) {
    this.holdMs = deps.holdMs ?? 450;
    this.slopPx = deps.slopPx ?? 10;
    this.setTimer = deps.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
    this.clearTimer = deps.clearTimer ?? ((id) => clearTimeout(id as ReturnType<typeof setTimeout>));
  }

  get selection(): Selection | null {
    return this.sel;
  }

  /** True while the finger is selecting or dragging a handle: the page must not scroll. */
  get busy(): boolean {
    return !!this.sel?.dragging || this.dragEnd !== null;
  }

  /** A touch started. `second` is true when another finger is already down (pinch, not a long press). */
  pointerDown(x: number, y: number, opts: { touch: boolean; second: boolean }): void {
    this.cancelHold();
    if (!opts.touch || opts.second || this.dragEnd) return;
    this.origin = { x, y };
    this.timer = this.setTimer(() => this.begin(), this.holdMs);
  }

  pointerMove(x: number, y: number): void {
    if (this.timer && Math.hypot(x - this.origin.x, y - this.origin.y) > this.slopPx) this.cancelHold(); // a scroll
    if (!this.sel) return;
    const cell = this.cellAt(x, y);
    if (!cell) return;
    if (this.dragEnd) this.sel = { ...this.sel, [this.dragEnd]: cell };
    else if (this.sel.dragging) this.sel = { ...this.sel, b: cell };
    else return;
    this.apply();
  }

  pointerUp(): void {
    this.cancelHold();
    this.dragEnd = null;
    if (this.sel?.dragging) {
      this.sel = { ...this.sel, dragging: false };
      this.deps.onChange(this.sel);
    }
  }

  /** The user grabbed a handle. */
  grab(end: End): void {
    this.dragEnd = end;
  }

  clear(): void {
    this.cancelHold();
    this.dragEnd = null;
    if (!this.sel) return;
    this.sel = null;
    this.deps.term()?.clearSelection();
    this.deps.onChange(null);
  }

  text(): string {
    const term = this.deps.term();
    return term ? selectedText(term) : '';
  }

  /** Where to draw a handle (below-left of the start, below-right of the end), relative to the host element. */
  handlePosition(end: End): { x: number; y: number } | null {
    const term = this.deps.term();
    const screen = this.deps.screenRect();
    const host = this.deps.hostRect();
    if (!this.sel || !term || !screen || !host) return null;
    const [start, last] = ordered(this.sel.a, this.sel.b);
    const cell = end === 'a' ? start : last;
    const box = cellBox(cell, screen, term.cols, term.rows, term.buffer.active.viewportY);
    if (!box) return null;
    return { x: box.x + (screen.left - host.left) + (end === 'b' ? box.w : 0), y: box.y + (screen.top - host.top) + box.h };
  }

  /** The buffer scrolled: handles must be redrawn at their new place. */
  refresh(): void {
    if (this.sel) this.deps.onChange({ ...this.sel });
  }

  private begin(): void {
    this.timer = null;
    const term = this.deps.term();
    const cell = this.cellAt(this.origin.x, this.origin.y);
    if (!term || !cell) return;
    const line = term.buffer.active.getLine(cell.row)?.translateToString(true) ?? '';
    const w = wordRange(line, cell.col);
    this.sel = { a: { col: w.start, row: cell.row }, b: { col: w.end, row: cell.row }, dragging: true };
    this.apply();
    this.deps.vibrate?.();
  }

  private cellAt(x: number, y: number): Cell | null {
    const term = this.deps.term();
    const rect = this.deps.screenRect();
    if (!term || !rect) return null;
    return cellAt(x, y, rect, term.cols, term.rows, term.buffer.active.viewportY);
  }

  private apply(): void {
    const term = this.deps.term();
    if (!this.sel || !term) return;
    const [start, end] = ordered(this.sel.a, this.sel.b);
    const { col, row, length } = selectionArgs(start, end, term.cols);
    term.select(col, row, length);
    this.deps.onChange(this.sel);
  }

  private cancelHold(): void {
    if (this.timer !== null) this.clearTimer(this.timer);
    this.timer = null;
  }
}
