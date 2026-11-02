import { beforeAll, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import WorkspaceActions from './WorkspaceActions.svelte';

// Server-side render, like the other component tests: the markup a phone gets. Typing, Enter, Escape and the soft keyboard are in
// e2e/reorder.mjs, which runs this component in Chrome at 390 px.
// The sheet remembers what had focus when it was made; a server render has no page, so a bare one stands in.
beforeAll(() => { vi.stubGlobal('document', { activeElement: null, body: {}, querySelector: () => null }); });
const workspace = { id: 'a', name: 'alpha', agent: null, serverId: 's', serverName: 'Host', key: 's/a' };
const html = (props: Record<string, unknown> = {}) => render(WorkspaceActions, { props: { workspace, ondetach() {}, oncloseworkspace() {}, onclose() {}, ...props } }).body;
const move = { onmove: () => null };
const disabled = (out: string, id: string) => new RegExp(`<button[^>]*disabled[^>]*data-testid="workspace-move-${id}"`).test(out);

describe('the workspace actions sheet', () => {
  it('names the workspace and its server, and offers Rename only when it can', () => {
    const out = html({ onrename: () => {} });
    expect(out).toContain('alpha');
    expect(out).toContain('Host');
    expect(out).toContain('data-testid="workspace-rename-item"');
    expect(html()).not.toContain('workspace-rename-item');
  });

  it('offers Upload only when it can', () => {
    expect(html({ onupload: () => {} })).toContain('data-testid="workspace-upload"');
    expect(html()).not.toContain('workspace-upload');
  });

  it('offers Move up, Move down, To top and To bottom and says where the workspace stands', () => {
    const out = html({ ...move, placement: { position: 2, count: 4 } });
    for (const id of ['up', 'down', 'top', 'bottom']) expect(out).toContain(`data-testid="workspace-move-${id}"`);
    expect(out).toMatch(/Position\s*2\s*of\s*4\s*on\s*Host/);
    expect(out).toContain('Order, position 2 of 4');
    for (const id of ['up', 'down', 'top', 'bottom']) expect(disabled(out, id)).toBe(false);
  });

  it('disables what cannot move at either end', () => {
    const first = html({ ...move, placement: { position: 1, count: 3 } });
    expect(disabled(first, 'up')).toBe(true);
    expect(disabled(first, 'top')).toBe(true);
    expect(disabled(first, 'down')).toBe(false);
    const last = html({ ...move, placement: { position: 3, count: 3 } });
    expect(disabled(last, 'down')).toBe(true);
    expect(disabled(last, 'bottom')).toBe(true);
    expect(disabled(last, 'up')).toBe(false);
  });

  it('shows no Move buttons for the only workspace of a server, without a position, or without a handler', () => {
    expect(html({ ...move, placement: { position: 1, count: 1 } })).not.toContain('workspace-move-up');
    expect(html({ ...move })).not.toContain('workspace-move-up');
    expect(html({ placement: { position: 2, count: 4 } })).not.toContain('workspace-move-up');
  });

  it('has a live region for the announcement of a move', () => {
    expect(html({ ...move, placement: { position: 2, count: 4 } })).toMatch(/role="status"[^>]*aria-live="polite"|aria-live="polite"[^>]*role="status"/);
  });

  it('opens straight on the question to close, naming the workspace and its server', () => {
    const out = html({ confirmFirst: true });
    expect(out).toContain('Everything running in it ends');
    expect(out).not.toContain('workspace-rename-item');
  });
});
