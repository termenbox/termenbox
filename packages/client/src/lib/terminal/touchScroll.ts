// One-finger gestures on the terminal. Every Termenbox workspace runs in the alternate screen (tmux), so there
// is no native scrollback to pan, and the browser's tap is a press and release in the same millisecond. This turns
// a vertical drag into wheel events (xterm then scrolls its buffer, reports the wheel to a mouse-aware app, or sends
// arrow keys) and a tap into a press, a short pause and a release that a mouse-aware app registers as a click.
// Everything the browser provides is injected, so the logic runs under test with a fake clock.

export interface ScrollDeps {
  /** Pixels per terminal row: a drag of this length is one wheel notch. */
  rowHeight: () => number;
  /** A wheel gesture of `dy` pixels at the finger (positive scrolls towards newer text). */
  wheel: (dy: number, x: number, y: number) => void;
  /** A tap at the finger. */
  tap: (x: number, y: number) => void;
  now?: () => number;
  /** Frame callback for the fling after the finger lifts. */
  raf?: (fn: () => void) => unknown;
  cancelRaf?: (id: unknown) => void;
  slopPx?: number;
  tapMs?: number;
}

export class TouchScroll {
  private id: number | null = null;
  private start = { x: 0, y: 0, t: 0 };
  private last = { x: 0, y: 0 };
  private samples: { y: number; t: number }[] = [];
  private scrolling = false;
  private flingId: unknown = null;
  private readonly slop: number;
  private readonly tapMs: number;
  private readonly now: () => number;
  private readonly raf: (fn: () => void) => unknown;
  private readonly cancelRaf: (id: unknown) => void;

  constructor(private deps: ScrollDeps) {
    this.slop = deps.slopPx ?? 8;
    this.tapMs = deps.tapMs ?? 450;
    this.now = deps.now ?? (() => performance.now());
    this.raf = deps.raf ?? ((fn) => requestAnimationFrame(fn));
    this.cancelRaf = deps.cancelRaf ?? ((id) => cancelAnimationFrame(id as number));
  }

  /** True while a finger is dragging the text: the page must not also scroll. */
  get active(): boolean {
    return this.scrolling;
  }

  /** A finger touched the screen. `ignore` is true during a selection or a pinch: this gesture stays out of it. */
  down(id: number, x: number, y: number, ignore: boolean): void {
    this.stopFling();
    if (this.id !== null || ignore) {
      this.id = null;
      this.scrolling = false;
      return;
    }
    this.id = id;
    this.start = { x, y, t: this.now() };
    this.last = { x, y };
    this.samples = [{ y, t: this.start.t }];
    this.scrolling = false;
  }

  /** The finger moved. Returns true when this is a scroll (the caller keeps the event from doing anything else). */
  move(id: number, x: number, y: number): boolean {
    if (id !== this.id) return false;
    if (!this.scrolling) {
      const dx = x - this.start.x;
      const dy = y - this.start.y;
      if (Math.hypot(dx, dy) < this.slop) return false;
      this.scrolling = true;
      this.last = { x: this.start.x, y: this.start.y }; // the slop distance counts too
    }
    const dy = this.last.y - y; // finger up = newer text
    this.last = { x, y };
    const t = this.now();
    this.samples.push({ y, t });
    this.samples = this.samples.filter((s) => t - s.t <= 120);
    if (dy !== 0) this.deps.wheel(dy, x, y);
    return true;
  }

  /** The finger lifted (or the gesture was cancelled). */
  up(id: number, cancelled = false): void {
    if (id !== this.id) return;
    this.id = null;
    if (this.scrolling) {
      this.scrolling = false;
      if (!cancelled) this.fling();
      return;
    }
    if (!cancelled && this.now() - this.start.t <= this.tapMs) this.deps.tap(this.last.x, this.last.y);
  }

  stopFling(): void {
    if (this.flingId !== null) this.cancelRaf(this.flingId);
    this.flingId = null;
  }

  /** Keeps the text moving after a flick, slowing down by a fixed factor per frame. */
  private fling(): void {
    const first = this.samples[0];
    const lastS = this.samples[this.samples.length - 1];
    if (!first || !lastS || lastS.t - first.t < 16) return;
    let v = ((first.y - lastS.y) / (lastS.t - first.t)) * 16; // px per frame, finger up positive
    if (Math.abs(v) < 2) return;
    const x = this.last.x;
    const y = this.last.y;
    const step = () => {
      v *= 0.94;
      if (Math.abs(v) < 0.5) {
        this.flingId = null;
        return;
      }
      this.deps.wheel(v, x, y);
      this.flingId = this.raf(step);
    };
    this.flingId = this.raf(step);
  }
}
