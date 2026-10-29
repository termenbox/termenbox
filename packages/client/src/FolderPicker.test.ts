import { describe, it, expect } from 'vitest';
import { render } from 'svelte/server';
import FolderPicker from './FolderPicker.svelte';

const html = (c: unknown, props: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(c, { props }).body;

// Navigation, filter, hidden, error/Retry, truncated and typed paths run in lib/folderBrowser.test.ts against the same state machine.
describe('FolderPicker', () => {
  const list = async () => ({ ok: true as const, path: '/home/u', home: '/home/u', entries: [], truncated: false });
  it('renders nothing closed, and the path bar, trail, filter, hidden toggle and a disabled Select while loading when open', () => {
    expect(html(FolderPicker, { open: false, list, onselect: () => {} })).not.toContain('folder-picker');
    const out = html(FolderPicker, { open: true, list, onselect: () => {} });
    expect(out).toContain('data-testid="picker-path"');
    expect(out).toContain('data-testid="picker-go"');
    expect(out).toContain('data-testid="picker-filter"');
    expect(out).toContain('data-testid="picker-hidden"');
    expect(out).toMatch(/data-testid="picker-select"[^>]*disabled|disabled[^>]*data-testid="picker-select"/);
    expect(out).toContain('Select this folder');
  });
});
