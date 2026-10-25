import { describe, it, expect } from 'vitest';
import { loadPaneModes, withPaneMode, PANE_KEY } from './paneMode';

const memory = () => { const m = new Map<string, string>(); return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) }; };

describe('pane mode per workspace', () => {
  it('is the terminal unless a workspace was switched to Decision, and is kept on this device', () => {
    const st = memory();
    expect(loadPaneModes(st)).toEqual({});
    const a = withPaneMode({}, 'srv/a', 'decision', st);
    expect(a).toEqual({ 'srv/a': 'decision' });
    expect(loadPaneModes(st)).toEqual({ 'srv/a': 'decision' });
    expect(withPaneMode(a, 'srv/a', 'terminal', st)).toEqual({});
    expect(loadPaneModes(st)).toEqual({});
  });
  it('survives broken or missing storage', () => {
    const bad = { getItem: () => '{nope', setItem: () => { throw new Error('full'); } };
    expect(loadPaneModes(bad)).toEqual({});
    expect(withPaneMode({}, 'k', 'decision', bad)).toEqual({ k: 'decision' });
    expect(loadPaneModes(memory())).toEqual({});
    const st = memory(); st.setItem(PANE_KEY, '{"a":"junk","b":"decision"}');
    expect(loadPaneModes(st)).toEqual({ b: 'decision' });
  });
});
