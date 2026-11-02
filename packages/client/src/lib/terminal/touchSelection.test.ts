import { describe, expect, it } from 'vitest';
import { TouchSelection, type Selection, type TermLike } from './touchSelection';

// A 80 x 20 screen of 10 x 20 px cells at (0, 100); the buffer shows line 1000 at the top.
const rect = { left: 0, top: 100, width: 800, height: 400 };

function setup(lines: Record<number, string> = { 1001: 'git commit -m "x"' }) {
  const calls: string[] = [];
  const changes: (Selection | null)[] = [];
  let timer: (() => void) | null = null;
  const term: TermLike = {
    cols: 80,
    rows: 20,
    buffer: { active: { viewportY: 1000, getLine: (i) => (lines[i] === undefined ? undefined : { isWrapped: false, translateToString: () => lines[i] }) } },
    select: (c, r, l) => calls.push(`select ${c},${r},${l}`),
    clearSelection: () => calls.push('clear'),
    getSelection: () => 'picked',
  };
  const sel = new TouchSelection({
    term: () => term,
    screenRect: () => rect,
    hostRect: () => ({ left: 0, top: 100, width: 800, height: 400 }),
    onChange: (s) => changes.push(s),
    setTimer: (fn) => ((timer = fn), 1),
    clearTimer: () => (timer = null),
  });
  return { sel, calls, changes, fire: () => timer?.(), pending: () => timer !== null };
}

describe('TouchSelection', () => {
  it('selects the word under a long press', () => {
    const t = setup();
    t.sel.pointerDown(65, 125, { touch: true, second: false }); // row 1001, col 6 -> "commit"
    expect(t.pending()).toBe(true);
    t.fire();
    expect(t.calls).toEqual(['select 4,1001,6']);
    expect(t.sel.selection?.dragging).toBe(true);
    expect(t.sel.busy).toBe(true);
  });

  it('extends while the finger drags and keeps the selection after it lifts', () => {
    const t = setup();
    t.sel.pointerDown(65, 125, { touch: true, second: false });
    t.fire();
    t.sel.pointerMove(165, 125); // to col 16
    expect(t.calls.at(-1)).toBe('select 4,1001,13');
    t.sel.pointerUp();
    expect(t.sel.busy).toBe(false);
    expect(t.sel.selection).not.toBeNull();
    expect(t.sel.text()).toBe('picked');
  });

  it('a moving finger is a scroll, not a long press', () => {
    const t = setup();
    t.sel.pointerDown(65, 125, { touch: true, second: false });
    t.sel.pointerMove(65, 160);
    expect(t.pending()).toBe(false);
    expect(t.sel.selection).toBeNull();
  });

  it('ignores mouse input and a second finger (pinch)', () => {
    const t = setup();
    t.sel.pointerDown(65, 125, { touch: false, second: false });
    expect(t.pending()).toBe(false);
    t.sel.pointerDown(65, 125, { touch: true, second: true });
    expect(t.pending()).toBe(false);
  });

  it('adjusts either end by dragging a handle', () => {
    const t = setup();
    t.sel.pointerDown(65, 125, { touch: true, second: false });
    t.fire();
    t.sel.pointerUp();
    t.sel.grab('a');
    t.sel.pointerMove(5, 125); // start moves to col 0
    expect(t.calls.at(-1)).toBe('select 0,1001,10');
    expect(t.sel.busy).toBe(true);
    t.sel.pointerUp();
    t.sel.grab('b');
    t.sel.pointerMove(235, 125); // end to col 23
    expect(t.calls.at(-1)).toBe('select 0,1001,24');
  });

  it('keeps the ends ordered when the end is dragged before the start', () => {
    const t = setup();
    t.sel.pointerDown(65, 125, { touch: true, second: false });
    t.fire();
    t.sel.pointerUp();
    t.sel.grab('b');
    t.sel.pointerMove(5, 125);
    expect(t.calls.at(-1)).toBe('select 0,1001,5');
  });

  it('places handles under the ends and hides ones scrolled out of view', () => {
    const t = setup();
    t.sel.pointerDown(65, 125, { touch: true, second: false });
    t.fire();
    expect(t.sel.handlePosition('a')).toEqual({ x: 40, y: 40 });
    expect(t.sel.handlePosition('b')).toEqual({ x: 100, y: 40 });
  });

  it('clear drops the selection and tells the terminal', () => {
    const t = setup();
    t.sel.pointerDown(65, 125, { touch: true, second: false });
    t.fire();
    t.sel.clear();
    expect(t.sel.selection).toBeNull();
    expect(t.calls.at(-1)).toBe('clear');
    expect(t.changes.at(-1)).toBeNull();
  });
});
