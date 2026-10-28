import { describe, it, expect } from 'vitest';
import { render } from 'svelte/server';
import { createRawSnippet } from 'svelte';
import { filterOptions, listboxIndex, trapTarget } from './layer';
import PickerSheet from './PickerSheet.svelte';
import DialogLayer from './DialogLayer.svelte';

const body = createRawSnippet(() => ({ render: () => '<p>content</p>' }));
const footer = createRawSnippet(() => ({ render: () => '<button>Select</button>' }));
const html = (c: unknown, props: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(c, { props }).body;
const options = [
  { value: 'claude', label: 'Claude Code' },
  { value: 'codex', label: 'Codex', recent: true },
  { value: 'gemini', label: 'Gemini' },
];

describe('DialogLayer', () => {
  it('renders nothing while closed', () => {
    expect(html(DialogLayer, { open: false, title: 'Pick', children: body })).not.toContain('<section');
  });
  it('is a labelled region with a 44 px Back, a body and a footer when open', () => {
    const out = html(DialogLayer, { open: true, title: 'Pick', children: body, footer });
    expect(out).toMatch(/<div[^>]*role="region"[^>]*aria-labelledby="layer-title-/);
    expect(out).toContain('>Pick</h3>');
    expect(out).toContain('aria-label="Back"');
    expect(out).toContain('<p>content</p>');
    expect(out).toContain('<footer');
  });
});

describe('PickerSheet', () => {
  const base = { open: true, title: 'Choose agent', options, selected: 'gemini', onselect: () => {} };
  it('marks the selected row with aria-selected and a check mark', () => {
    const out = html(PickerSheet, base);
    expect(out.match(/aria-selected="true"/g)).toHaveLength(1);
    expect(out).toMatch(/aria-selected="true"[^>]*>[\s\S]*Gemini[\s\S]*picker-check/);
    expect(out.match(/picker-check/g)).toHaveLength(1);
  });
  it('lists the Recent section first', () => {
    const out = html(PickerSheet, base);
    expect(out).toContain('Recent');
    expect(out.indexOf('Codex')).toBeLessThan(out.indexOf('Claude Code'));
  });
  it('has a filter that takes the focus, and rows at least 48 px high', () => {
    const out = html(PickerSheet, { ...base, filterable: true, filterPlaceholder: 'Filter agents…' });
    expect(out).toMatch(/<input[^>]*data-autofocus/);
    expect(out).toContain('min-h-12');
  });
  it('shows the empty message when there are no options', () => {
    expect(html(PickerSheet, { ...base, options: [], emptyMessage: 'No agents match' })).toContain('No agents match');
  });
});

describe('layer helpers', () => {
  it('moves through a listbox with arrows, Home and End, clamping at both ends', () => {
    expect(listboxIndex('ArrowDown', 0, 3)).toBe(1);
    expect(listboxIndex('ArrowDown', 2, 3)).toBe(2);
    expect(listboxIndex('ArrowUp', 0, 3)).toBe(0);
    expect(listboxIndex('ArrowUp', 2, 3)).toBe(1);
    expect(listboxIndex('Home', 2, 3)).toBe(0);
    expect(listboxIndex('End', 0, 3)).toBe(2);
    expect(listboxIndex('a', 0, 3)).toBeNull();
    expect(listboxIndex('ArrowDown', 0, 0)).toBeNull();
  });
  it('wraps Tab inside the layer only at the ends', () => {
    expect(trapTarget(false, 2, 3)).toBe(0);
    expect(trapTarget(false, 0, 3)).toBeNull();
    expect(trapTarget(true, 0, 3)).toBe(2);
    expect(trapTarget(true, 1, 3)).toBeNull();
    expect(trapTarget(false, -1, 3)).toBe(0);
    expect(trapTarget(false, 0, 0)).toBeNull();
  });
  it('filters by label or value, ignoring case', () => {
    expect(filterOptions(options, 'GEM').map(o => o.value)).toEqual(['gemini']);
    expect(filterOptions(options, 'code').map(o => o.value)).toEqual(['claude', 'codex']);
    expect(filterOptions(options, '  ')).toHaveLength(3);
    expect(filterOptions(options, 'zzz')).toEqual([]);
  });
});
