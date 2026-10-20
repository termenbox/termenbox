<script lang="ts">
  import { LARGE_STEP } from '../lib/panes';

  /**
   * A draggable, focusable divider between two panes (the ARIA window splitter). The line is 2 px but the target is
   * 44 px wide, centred on it. Pointer: drag. Keyboard: arrows move it by `step` (Shift: a larger step), Home and End go
   * to the smallest and largest width, Enter or a double click resets it. `value` is the width of the pane it sizes;
   * `grow` is +1 when dragging right makes that pane wider (a pane on the left) and -1 when it makes it narrower.
   * `onchange` fires while moving; `oncommit` once the pointer is up or the key is released, which is when it is saved.
   */
  let { label, value, min, max, def, step = 16, grow = 1, controls, onchange, oncommit }: {
    label: string; value: number; min: number; max: number; def: number; step?: number; grow?: 1 | -1; controls: string;
    onchange: (width: number) => void; oncommit: () => void;
  } = $props();

  let drag: { id: number; x: number; from: number } | null = null;
  let dragging = $state(false);
  let dirty = false;
  const set = (n: number) => { const next = Math.min(max, Math.max(min, Math.round(n))); if (next !== value) { dirty = true; onchange(next); } };

  function down(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { id: e.pointerId, x: e.clientX, from: value };
    dragging = true;
  }
  function move(e: PointerEvent) {
    if (drag?.id === e.pointerId) set(drag.from + grow * (e.clientX - drag.x));
  }
  function up(e: PointerEvent) {
    if (drag?.id !== e.pointerId) return;
    drag = null;
    dragging = false;
    if (dirty) { dirty = false; oncommit(); }
  }
  function key(e: KeyboardEvent) {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    const by = e.shiftKey ? LARGE_STEP : step;
    if (e.key === 'ArrowRight') set(value + grow * by);
    else if (e.key === 'ArrowLeft') set(value - grow * by);
    else if (e.key === 'Home') set(min);
    else if (e.key === 'End') set(max);
    else if (e.key === 'Enter') set(def);
    else return;
    e.preventDefault();
  }
  function keyup(e: KeyboardEvent) {
    if (dirty && ['ArrowRight', 'ArrowLeft', 'Home', 'End', 'Enter'].includes(e.key)) { dirty = false; oncommit(); }
  }
  function reset() { set(def); if (dirty) { dirty = false; oncommit(); } }
</script>

<!-- A zero-width slot in the row; the 44 px target overlaps the panes either side of the line. -->
<div class="relative z-20 w-0 shrink-0 self-stretch">
  <div role="separator" tabindex="0" aria-orientation="vertical" aria-label={label} aria-controls={controls}
    aria-valuenow={value} aria-valuemin={min} aria-valuemax={max} aria-valuetext="{value} pixels"
    data-resizer={controls} data-dragging={dragging}
    onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} onlostpointercapture={up}
    onkeydown={key} onkeyup={keyup} ondblclick={reset}
    class="resizer absolute inset-y-0 -left-[22px] w-11 cursor-col-resize touch-none select-none"></div>
</div>

<style>
  .resizer::after { content: ''; position: absolute; inset-block: 0; left: 50%; width: 2px; margin-left: -1px; background: var(--line, currentColor); transition: background-color 120ms; }
  .resizer:hover::after, .resizer[data-dragging='true']::after { background: var(--accent); width: 4px; margin-left: -2px; }
  .resizer:focus-visible { outline: none; }
  .resizer:focus-visible::after { background: var(--accent); width: 4px; margin-left: -2px; }
</style>
