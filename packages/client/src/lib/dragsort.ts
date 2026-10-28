// Dragging a row by its handle: where it would land, how fast to scroll near the edges, and the pointer-event controller
// that ties them to the DOM. The arithmetic is pure and tested; the controller is covered by the phone e2e probe.

export interface RowBox { key: string; top: number; bottom: number }

/** The row the dragged one is dropped beside: before the first row whose middle is below the pointer, else after the last. */
export function dropTarget(rows: RowBox[], y: number, dragged: string): { key: string; side: 'before' | 'after' } | null {
  const others = rows.filter((r) => r.key !== dragged);
  if (!others.length) return null;
  const below = others.find((r) => y < (r.top + r.bottom) / 2);
  return below ? { key: below.key, side: 'before' } : { key: others[others.length - 1].key, side: 'after' };
}

/** Pixels to scroll per frame: 0 in the middle, growing to `max` at the edge (and past it), signed by direction. */
export function edgeScroll(y: number, top: number, bottom: number, zone = 56, max = 18): number {
  if (y < top + zone) return -Math.round(max * Math.min(1, (top + zone - y) / zone));
  if (y > bottom - zone) return Math.round(max * Math.min(1, (y - (bottom - zone)) / zone));
  return 0;
}

export interface DragView { key: string; dy: number; target: { key: string; side: 'before' | 'after' } | null }
export interface DragOptions {
  /** The scrolling element that holds the rows; each row is `[data-row=key]` with `data-list` naming the list it belongs to and holds its handle as `[data-grip]`. */
  scroller: () => HTMLElement | undefined;
  /** The dragged row's offset and the landing place, or null when the drag ended. */
  onview: (view: DragView | null) => void;
  /** A drop (or an arrow key): put `key` beside `target.key`. */
  ondrop: (key: string, target: { key: string; side: 'before' | 'after' }) => void;
}

/**
 * Dragging a row by its handle with pointer events: touch, pen and mouse alike. The handle has `touch-action: none` (and only
 * it), so a finger anywhere else still scrolls. The row follows the pointer, the list scrolls by itself near its edges, and
 * nothing is reordered until the pointer is released. Escape or a cancelled pointer puts everything back. Arrow keys on the
 * handle move one place for those who cannot drag.
 */
export class RowDragger {
  private s: { key: string; pointer: number; startY: number; startScroll: number; y: number; target: DragView['target'] } | null = null;
  private frame = 0;
  constructor(private o: DragOptions) {}

  get active() { return !!this.s; }

  down(e: PointerEvent, key: string) {
    const scroller = this.o.scroller();
    if (e.button !== 0 || this.s || !scroller) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    this.s = { key, pointer: e.pointerId, startY: e.clientY, startScroll: scroller.scrollTop, y: e.clientY, target: null };
    this.frame = requestAnimationFrame(this.tick);
    this.publish();
  }
  move(e: PointerEvent) {
    if (!this.s || this.s.pointer !== e.pointerId) return;
    this.s.y = e.clientY;
    this.retarget();
  }
  up(e: PointerEvent, cancelled = false) {
    if (!this.s || this.s.pointer !== e.pointerId) return;
    const { key, target } = this.s;
    this.stop();
    if (!cancelled && target) this.o.ondrop(key, target);
  }
  key(e: KeyboardEvent, key: string) {
    if (e.key === 'Escape' && this.s) { this.stop(); return; }
    if (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') return;
    e.preventDefault();
    const keys = this.rows(key).map((r) => r.dataset.row!);
    const up = e.key === 'ArrowUp';
    const neighbour = keys[keys.indexOf(key) + (up ? -1 : 1)];
    if (!neighbour) return;
    this.o.ondrop(key, { key: neighbour, side: up ? 'before' : 'after' });
    // The list re-renders and the row's node moves, which drops focus: give it back to the same handle, so the next key goes on.
    requestAnimationFrame(() => this.o.scroller()?.querySelector<HTMLElement>(`[data-row="${CSS.escape(key)}"] [data-grip]`)?.focus());
  }
  destroy() { this.stop(); }

  private stop() {
    cancelAnimationFrame(this.frame);
    this.s = null;
    this.publish();
  }
  private rows(key: string): HTMLElement[] {
    const scroller = this.o.scroller();
    const row = scroller?.querySelector<HTMLElement>(`[data-row="${CSS.escape(key)}"]`);
    if (!scroller || !row) return [];
    return [...scroller.querySelectorAll<HTMLElement>('[data-row]')].filter((r) => r.dataset.list === row.dataset.list);
  }
  private retarget() {
    if (!this.s) return;
    const rows = this.rows(this.s.key).map((r) => { const b = r.getBoundingClientRect(); return { key: r.dataset.row!, top: b.top, bottom: b.bottom }; });
    this.s.target = dropTarget(rows, this.s.y, this.s.key);
    this.publish();
  }
  // Near the top or bottom edge of the list it scrolls by itself, so a row can be taken further than the screen shows.
  private tick = () => {
    const scroller = this.o.scroller();
    if (!this.s || !scroller) return;
    const box = scroller.getBoundingClientRect();
    const delta = edgeScroll(this.s.y, box.top, box.bottom);
    if (delta) { scroller.scrollTop += delta; this.retarget(); }
    this.frame = requestAnimationFrame(this.tick);
  };
  private publish() {
    const scroller = this.o.scroller();
    this.o.onview(this.s ? { key: this.s.key, dy: this.s.y - this.s.startY + ((scroller?.scrollTop ?? 0) - this.s.startScroll), target: this.s.target } : null);
  }
}
