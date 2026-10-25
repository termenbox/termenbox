import { describe, expect, it } from 'vitest';
import { loadHidden, openTabs, saveHidden } from './tabs';
import type { AgentMap } from './agents';

const a = (state: 'idle' | 'needs_input'): AgentMap[string] => ({ state, seq: 1, stale: false, ts: 0 });

describe('detached tabs', () => {
  const workspaces = [{ key: 's/a' }, { key: 's/b' }, { key: 's/c' }];
  it('closes the tabs of detached workspaces but brings one back while it needs you', () => {
    const hidden = new Set(['s/a', 's/b']);
    expect(openTabs(workspaces, {}, hidden).map(s => s.key)).toEqual(['s/c']);
    expect(openTabs(workspaces, { 's/b': a('needs_input') }, hidden).map(s => s.key)).toEqual(['s/b', 's/c']);
    expect(openTabs(workspaces, { 's/b': a('idle') }, hidden).map(s => s.key)).toEqual(['s/c']);
  });
  it('is kept on the device and survives bad data', () => {
    const mem: Record<string, string> = {};
    const storage = { getItem: (k: string) => mem[k] ?? null, setItem: (k: string, v: string) => { mem[k] = v; } };
    saveHidden(new Set(['s/a']), storage);
    expect([...loadHidden(storage)]).toEqual(['s/a']);
    expect(loadHidden({ getItem: () => '{"x":1}' }).size).toBe(0);
    expect(loadHidden({ getItem: () => 'not json' }).size).toBe(0);
  });
});
