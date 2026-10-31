import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import SideNav from './SideNav.svelte';
import type { NavGroup } from '../lib/navlist';

const groups = [{
  server: { id: 'a', state: 'connected', server: { server_name: 'Box' } },
  workspaces: [{ key: 'a:work', name: 'work', serverId: 'a', serverName: 'Box', state: 'idle' }],
}] as unknown as NavGroup[];

const html = (extra: Record<string, unknown>) =>
  (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(SideNav, {
    props: { current: 'workspaces', onnavigate: () => {}, groups, onopen: () => {}, onserver: () => {}, onsearch: () => {}, searchHint: 'Ctrl K', ...extra },
  }).body;

describe('the sidebar workspace rows', () => {
  it('offer rename and close beside the name when the handlers are given', () => {
    const out = html({ onrename: () => {}, oncloseworkspace: () => {} });
    expect(out).toContain('aria-label="Rename work"');
    expect(out).toContain('aria-label="Close work"');
    expect(out).toContain('data-key="a:work"');
  });

  it('show neither without the handlers', () => {
    const out = html({});
    expect(out).not.toContain('Rename work');
    expect(out).not.toContain('Close work');
    expect(out).toContain('data-key="a:work"');
  });
});

describe('the sidebar workspace handles', () => {
  const two = [{
    server: { id: 'a', state: 'connected', server: { server_name: 'Box' } },
    workspaces: [
      { key: 'a:one', name: 'one', serverId: 'a', serverName: 'Box', state: 'idle' },
      { key: 'a:two', name: 'two', serverId: 'a', serverName: 'Box', state: 'idle' },
    ],
  }] as unknown as NavGroup[];
  const count = (out: string, needle: string) => out.split(needle).length - 1;

  it('give each row a handle, tagged with its list, when a move handler is given', () => {
    const out = html({ groups: two, onmove: () => {} });
    expect(count(out, 'data-testid="workspace-grip"')).toBe(2);
    expect(out).toContain('Reorder one');
    expect(out).toContain('data-row="a:two"');
    expect(out).toContain('data-list="a"');
    expect(out).toMatch(/role="status"/);
  });

  it('show none without a move handler, or for a server\'s only workspace', () => {
    expect(html({ groups: two })).not.toContain('workspace-grip');
    expect(html({ onmove: () => {} })).not.toContain('workspace-grip');
  });
});
