import { describe, expect, it } from 'vitest';
import { canSplit, DEFAULT_LAYOUT, loadLayout, MIN_CONTENT, MIN_HALF, parseLayout, saveLayout, SIDEBAR, sidebarWidthFor, SPLIT, splitWidthFor } from './panes';

describe('workspace layout', () => {
  it('starts with the sidebar open at its default width and no split', () => {
    expect(parseLayout(null)).toEqual(DEFAULT_LAYOUT);
    expect(parseLayout('')).toEqual(DEFAULT_LAYOUT);
  });
  it('round-trips through storage', () => {
    const mem = new Map<string, string>();
    const store = { getItem: (k: string) => mem.get(k) ?? null, setItem: (k: string, v: string) => void mem.set(k, v) };
    const saved = { sidebarOpen: false, sidebarWidth: 333, splitOpen: true, splitWidth: 555 };
    saveLayout(saved, store);
    expect(loadLayout(store)).toEqual(saved);
  });
  it('repairs anything malformed instead of throwing', () => {
    expect(parseLayout('{nope')).toEqual(DEFAULT_LAYOUT);
    expect(parseLayout('[1,2]')).toEqual(DEFAULT_LAYOUT);
    expect(parseLayout(JSON.stringify({ sidebarWidth: 'wide', splitWidth: NaN, sidebarOpen: 'no' }))).toEqual({ ...DEFAULT_LAYOUT });
    expect(parseLayout(JSON.stringify({ sidebarWidth: 9999, splitWidth: 1 })).sidebarWidth).toBe(SIDEBAR.max);
    expect(parseLayout(JSON.stringify({ sidebarWidth: 9999, splitWidth: 1 })).splitWidth).toBe(SPLIT.min);
  });
  it('survives storage that throws (private mode, blocked site data)', () => {
    const boom = { getItem: () => { throw new Error('denied'); }, setItem: () => { throw new Error('denied'); } };
    expect(loadLayout(boom)).toEqual(DEFAULT_LAYOUT);
    expect(() => saveLayout(DEFAULT_LAYOUT, boom)).not.toThrow();
    expect(loadLayout(undefined)).toEqual(DEFAULT_LAYOUT);
  });
});

describe('clamping keeps the terminal usable', () => {
  it('the sidebar stays in range and leaves the content its minimum', () => {
    expect(sidebarWidthFor(10, 1440)).toBe(SIDEBAR.min);
    expect(sidebarWidthFor(900, 1440)).toBe(SIDEBAR.max);
    expect(sidebarWidthFor(400, 721)).toBe(721 - MIN_CONTENT);
    expect(sidebarWidthFor(260, 721)).toBe(260);
    expect(sidebarWidthFor(480, 1024)).toBe(480);
    expect(sidebarWidthFor(260, 300)).toBe(SIDEBAR.min); // never below its own minimum
  });
  it('the terminal beside the decision leaves each half its minimum', () => {
    expect(splitWidthFor(480, 1200)).toBe(480);
    expect(splitWidthFor(900, 700)).toBe(700 - MIN_HALF);
    expect(splitWidthFor(100, 700)).toBe(SPLIT.min);
    expect(canSplit(MIN_HALF * 2 - 1)).toBe(false);
    expect(canSplit(MIN_HALF * 2)).toBe(true);
  });
});
