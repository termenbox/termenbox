import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import SideNav from './SideNav.svelte';
import type { NavGroup } from '../lib/navlist';

const groups = [{
  server: { id: 'a', state: 'connected', server: { server_name: 'Box' } },
  sessions: [{ key: 'a:work', name: 'work', serverId: 'a', serverName: 'Box', state: 'idle' }],
}] as unknown as NavGroup[];

const html = (extra: Record<string, unknown>) =>
  (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(SideNav, {
    props: { current: 'sessions', onnavigate: () => {}, groups, onopen: () => {}, onserver: () => {}, onsearch: () => {}, searchHint: 'Ctrl K', ...extra },
  }).body;

describe('the sidebar session rows', () => {
  it('offer rename and close beside the name when the handlers are given', () => {
    const out = html({ onrename: () => {}, onclosesession: () => {} });
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
