import { describe, it, expect } from 'vitest';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { backdropClose } from './backdropClose';

// No DOM here: a bare target stands in for the <dialog>, events are dispatched by hand.
function setup() {
  const listeners = new Map<string, (e: never) => void>();
  const node = {
    addEventListener: (t: string, f: (e: never) => void) => listeners.set(t, f),
    removeEventListener: (t: string) => listeners.delete(t),
  } as unknown as HTMLDialogElement;
  let closed = 0;
  const action = backdropClose(node, () => { closed++; });
  const fire = (type: string, target: unknown) => listeners.get(type)?.({ target } as never);
  const field = {};
  return { node, action, fire, field, closes: () => closed, listeners };
}

describe('backdropClose', () => {
  it('closes on a press and release on the backdrop', () => {
    const s = setup();
    s.fire('pointerdown', s.node);
    s.fire('click', s.node);
    expect(s.closes()).toBe(1);
  });

  it('stays open when a press starts in a field and the release lands on the backdrop', () => {
    const s = setup();
    s.fire('pointerdown', s.field);
    s.fire('click', s.node); // the browser retargets the click to the common ancestor: the dialog
    expect(s.closes()).toBe(0);
  });

  it('stays open on a click inside the content', () => {
    const s = setup();
    s.fire('pointerdown', s.field);
    s.fire('click', s.field);
    expect(s.closes()).toBe(0);
  });

  it('does not reuse a stale press for a later click', () => {
    const s = setup();
    s.fire('pointerdown', s.node);
    s.fire('click', s.field);
    s.fire('click', s.node); // e.g. a keyboard-synthesised click with no pointerdown
    expect(s.closes()).toBe(0);
  });

  it('follows an updated callback and detaches on destroy', () => {
    const s = setup();
    let other = 0;
    s.action.update(() => { other++; });
    s.fire('pointerdown', s.node);
    s.fire('click', s.node);
    expect([s.closes(), other]).toEqual([0, 1]);
    s.action.destroy();
    expect(s.listeners.size).toBe(0);
  });
});

describe('dialogs', () => {
  it('none close on a bare click on the dialog (a drag out of a field would dismiss it)', () => {
    const dir = join(__dirname, '..');
    const bad = readdirSync(dir)
      .filter(f => f.endsWith('.svelte'))
      .filter(f => /(on:click|onclick)=\{\(e\) => \{ if \(e\.target === dialog/.test(readFileSync(join(dir, f), 'utf8')));
    expect(bad).toEqual([]);
  });
});
