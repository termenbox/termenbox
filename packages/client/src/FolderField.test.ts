import { describe, it, expect } from 'vitest';
import { render } from 'svelte/server';
import FolderField from './FolderField.svelte';

const html = (c: unknown, props: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(c, { props }).body;
const field = (p: Record<string, unknown>) => html(FolderField, { value: '', home: '/home/u', recents: [], onchange: () => {}, onbrowse: () => {}, ...p });
const recents = ['/home/u/code/termenbox', '/home/u/code/app', '/home/u/notes', '/srv/work', '/home/u/a/b'];

describe('FolderField', () => {
  it('is a readonly line, not a text input, with the path in the title and ~ shortening', () => {
    const out = field({ value: '/home/u/code/termenbox' });
    expect(out).toContain('aria-label="Folder"');
    expect(out).toContain('title="/home/u/code/termenbox"');
    expect(out).toContain('~/code/termenbox');
    expect(out).not.toMatch(/<input/);
    expect(out).toContain('>Change<');
  });
  it('says Choose a folder in the empty state', () => {
    const out = field({});
    expect(out).toContain('Choose a folder');
    expect(out).toContain('>Choose<');
    expect(out).not.toContain('dir-chips');
  });
  it('shows up to three recent chips (last two segments, full path in title) and a More chip for the rest', () => {
    const out = field({ recents });
    expect(out.match(/data-testid="dir-chip"/g)).toHaveLength(3);
    expect(out).toContain('code/termenbox');
    expect(out).toContain('title="/home/u/code/app"');
    expect(out).toContain('data-testid="dirs-more"');
  });
  it('leaves the current folder out of the chips and has no More when all fit', () => {
    const out = field({ value: '/home/u/notes', recents: recents.slice(0, 3) });
    expect(out.match(/data-testid="dir-chip"/g)).toHaveLength(2);
    expect(out).not.toContain('dirs-more');
  });
  it('lists the limited recent folders in a layer with a filter', () => {
    const out = field({ recents, moreOpen: true, value: '/srv/work' });
    expect(out).toContain('data-testid="dirs-layer"');
    expect(out).toContain('Recent folders');
    expect(out).toContain('Filter folders…');
    expect(out).toContain('aria-selected="true"');
  });
});
