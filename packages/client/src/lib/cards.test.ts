import { describe, expect, it } from 'vitest';
import { ageText, chipFor, FrozenOrder, progress, staleText, toolLine } from './cards';
import type { WorkspaceAgent } from './agents';

const a = (state: WorkspaceAgent['state'], extra: Partial<WorkspaceAgent> = {}): WorkspaceAgent => ({ state, seq: 1, stale: false, ts: 1000, ...extra });

describe('chips', () => {
  it('always carry a word and an icon, for every state', () => {
    for (const s of ['needs_input', 'working', 'done', 'error', 'idle', 'unknown'] as const) {
      const c = chipFor(a(s), 1000);
      expect(c.label.length).toBeGreaterThan(2);
      expect(c.icon.length).toBeGreaterThan(0);
    }
    expect(chipFor(a('needs_input'), 0).label).toBe('Needs you');
  });
  it('names why a working workspace works and shows background work, only with server status', () => {
    expect(chipFor(a('working', { serverStatus: true, subReason: 'tool' }), 1000).label).toBe('Running a tool');
    expect(chipFor(a('working', { subReason: 'tool' }), 1000).label).toBe('Working');
    expect(chipFor(a('idle', { serverStatus: true, bg: 2 }), 1000).bg).toBe('bg: 2');
    expect(chipFor(a('idle', { serverStatus: true, bg: 0 }), 1000).bg).toBe('');
  });
  it('marks a working state without news as stale', () => {
    expect(chipFor(a('working'), 1000 + 30_000).stale).toBe(false);
    expect(chipFor(a('working'), 1000 + 90_000).stale).toBe(true);
    expect(staleText(a('working'), 1000 + 190_000)).toBe('no update for 3 min');
  });
});

describe('card text', () => {
  it('summarises plan progress, age and the tool line', () => {
    expect(progress([{ text: 'a', status: 'done' }, { text: 'b', status: 'todo' }, { text: 'c', status: 'completed' }])).toBe('2/3');
    expect(progress([])).toBeNull();
    expect(progress(undefined)).toBeNull();
    expect(ageText(1000, 1000 + 3000)).toBe('now');
    expect(ageText(1000, 1000 + 42_000)).toBe('42 s');
    expect(ageText(1000, 1000 + 5 * 60_000)).toBe('5 min');
    expect(ageText(1000, 1000 + 3 * 3_600_000)).toBe('3 h');
    expect(toolLine(a('working', { tool: { name: 'Edit', summary: 'src/app.ts', phase: 'started' } }))).toBe('Edit src/app.ts');
    expect(toolLine(a('working', { tool: { name: 'Bash', summary: 'ls', phase: 'finished' } }))).toBe('Last: Bash ls');
    expect(toolLine(a('idle'))).toBe('');
  });
});

describe('order', () => {
  it('keeps the order it is given: a state never moves a card', () => {
    const o = new FrozenOrder();
    expect(o.order(['x', 'y', 'z', 'w'], 0)).toEqual(['x', 'y', 'z', 'w']);
  });

  it('holds still while a finger is down, a sheet is open or the list was just scrolled, then applies the change', () => {
    const o = new FrozenOrder(2000);
    expect(o.order(['x', 'y', 'z'], 0)).toEqual(['x', 'y', 'z']);
    o.touchDown();
    expect(o.order(['z', 'y', 'x'], 100)).toEqual(['x', 'y', 'z']); // the user's order changed but nothing moves
    o.touchUp();
    o.sheet(true);
    expect(o.order(['z', 'y', 'x'], 200)).toEqual(['x', 'y', 'z']);
    o.sheet(false);
    o.scrolled(300);
    expect(o.order(['z', 'y', 'x'], 1000)).toEqual(['x', 'y', 'z']);
    expect(o.order(['z', 'y', 'x'], 2500)).toEqual(['z', 'y', 'x']); // two seconds after the scroll
  });

  it('ignores releases of pointers that did not start here, including duplicate releases', () => {
    const o = new FrozenOrder();
    o.touchDown(1); o.touchDown(2); o.touchDown(2);
    o.touchUp(99); expect(o.frozen(0)).toBe(true);
    o.touchUp(1); o.touchUp(1); expect(o.frozen(0)).toBe(true);
    o.touchUp(2); expect(o.frozen(0)).toBe(false);
  });

  it('while held, neither insertion nor removal moves controls; both apply after release', () => {
    const o = new FrozenOrder();
    o.order(['x', 'y'], 0);
    o.touchDown();
    expect(o.order(['y', 'n', 'z'], 10)).toEqual(['x', 'y']);
    expect(o.order(['n', 'y'], 20)).toEqual(['x', 'y']);
    o.touchUp();
    expect(o.order(['n', 'y'], 21)).toEqual(['n', 'y']);
  });
});
