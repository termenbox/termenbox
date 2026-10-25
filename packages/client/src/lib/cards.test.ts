import { describe, expect, it } from 'vitest';
import { ageText, chipFor, FrozenOrder, progress, sortIds, staleText, toolLine } from './cards';
import type { AgentMap, WorkspaceAgent } from './agents';

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
  const agents: AgentMap = { x: a('idle'), y: a('needs_input'), z: a('working') };
  it('puts needs-you first, then error, working, done, idle, and workspaces without events last', () => {
    expect(sortIds(['x', 'y', 'z', 'w'], agents)).toEqual(['y', 'z', 'x', 'w']);
  });

  it('holds still while a finger is down, a sheet is open or the list was just scrolled, then applies the change', () => {
    const o = new FrozenOrder(2000);
    expect(o.order(['x', 'y', 'z'], agents, 0)).toEqual(['y', 'z', 'x']);
    const later: AgentMap = { ...agents, x: a('needs_input') };
    o.touchDown();
    expect(o.order(['x', 'y', 'z'], later, 100)).toEqual(['y', 'z', 'x']); // x needs you now but nothing moves
    o.touchUp();
    o.sheet(true);
    expect(o.order(['x', 'y', 'z'], later, 200)).toEqual(['y', 'z', 'x']);
    o.sheet(false);
    o.scrolled(300);
    expect(o.order(['x', 'y', 'z'], later, 1000)).toEqual(['y', 'z', 'x']);
    expect(o.order(['x', 'y', 'z'], later, 2500)).toEqual(['x', 'y', 'z']); // two seconds after the scroll
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
    o.order(['x', 'y'], agents, 0);
    o.touchDown();
    expect(o.order(['y', 'n', 'z'], agents, 10)).toEqual(['y', 'x']);
    expect(o.order(['n', 'y'], agents, 20)).toEqual(['y', 'x']);
    o.touchUp();
    expect(o.order(['n', 'y'], agents, 21)).toEqual(['y', 'n']);
  });
});
