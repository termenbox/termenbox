import { describe, expect, it } from 'vitest';
import { applyWindowList, forgetWindows, representativeWindow, shortPath, tabTexts, windowToOpen } from './windows';
import type { WindowInfo } from './protocol';

const w = (index: number, path: string, extra: Partial<WindowInfo> = {}): WindowInfo => ({ index, name: 'n', command: 'bash', path, active: false, ...extra });

describe('window state', () => {
  it('stores a window list per workspace and forgets the workspaces that are gone', () => {
    let map = applyWindowList({}, { type: 'window_list', workspace: 'a', windows: [w(0, '/x')], representative: 0 });
    map = applyWindowList(map, { type: 'window_list', workspace: 'b', windows: [w(0, '/y')] });
    expect(map.b.representative).toBeNull();
    expect(Object.keys(forgetWindows(map, ['b']))).toEqual(['b']);
  });
  it('speaks for the representative window, else the active one, else the first', () => {
    expect(representativeWindow({ windows: [w(0, '/a'), w(2, '/b')], representative: 2 })?.path).toBe('/b');
    expect(representativeWindow({ windows: [w(0, '/a'), w(1, '/c', { active: true })], representative: null })?.path).toBe('/c');
    expect(representativeWindow({ windows: [w(5, '/a')], representative: 9 })?.path).toBe('/a');
    expect(representativeWindow(undefined)).toBeUndefined();
  });
});

describe('short paths', () => {
  it('shows the last two segments, and ~ for a home directory', () => {
    expect(shortPath('/home/u/projects/termenbox')).toBe('~/projects/termenbox');
    expect(shortPath('/home/u/a/b/c')).toBe('…/b/c');
    expect(shortPath('/home/u')).toBe('~');
    expect(shortPath('/root')).toBe('~');
    expect(shortPath('/srv/app')).toBe('/srv/app');
    expect(shortPath('/')).toBe('/');
    expect(shortPath('')).toBe('');
  });
});

describe('tab text', () => {
  const workspaces = [{ key: 's/a', name: 'a' }, { key: 's/b', name: 'b' }, { key: 's/c', name: 'c' }, { key: 's/d', name: 'd' }];
  const windows = {
    's/a': { windows: [w(0, '/srv/app')], representative: 0 },
    's/b': { windows: [w(0, '/srv/app')], representative: 0 },
    's/c': { windows: [w(0, '/srv/other')], representative: 0 },
  };
  it('tells tabs that read the same apart by workspace name, and falls back to the name without a path', () => {
    const t = tabTexts(workspaces, windows);
    expect(t['s/a'].label).toBe('/srv/app · a');
    expect(t['s/b'].label).toBe('/srv/app · b');
    expect(t['s/c'].label).toBe('/srv/other');
    expect(t['s/c'].hint).toBe('/srv/other');
    expect(t['s/d'].label).toBe('d');
  });
});

describe('opening a workspace that needs you', () => {
  it('goes to the agent window when another one is current, and leaves the current one alone otherwise', () => {
    const list = [w(0, '/a', { active: true }), w(2, '/b', { agent: 'claude' })];
    expect(windowToOpen({ windows: list, representative: 2 })).toBe(2);
    expect(windowToOpen({ windows: list, representative: 0 })).toBeNull();
    expect(windowToOpen({ windows: list, representative: null })).toBeNull();
    expect(windowToOpen(undefined)).toBeNull();
  });
});
