import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import NavigationBar from './NavigationBar.svelte';
import type { HubServer, HubWorkspace } from './lib/hub';

const server = { id: 's', state: 'connected', backends: ['tmux'], server: { server_name: 'Host' } } as unknown as HubServer;
const workspace: HubWorkspace = {
  id: 'w', name: 'Work', agent: null, serverId: 's', serverName: 'Host', key: 's/w',
};
const html = (extra: Record<string, unknown> = {}) => render(NavigationBar, {
  props: {
    workspaces: [], agents: {}, servers: [server], activeKey: 's/w',
    onopen() {}, onactions() {}, onmenu() {}, onworkspaces() {}, ...extra,
  },
}).body;

describe('the top bar', () => {
  it('shows no connection-quality control for a selected server', () => {
    const out = html();
    expect(out).not.toContain('Connection:');
    expect(out).not.toContain('class="secondary link"');
    expect(out).not.toContain('class="dot');
  });
  it('accepts a workspace with tabs without adding a connection control', () => {
    const out = html({ tabsCapable: true, workspace, tabs: [], wide: false });
    expect(out).not.toContain('Connection:');
    expect(out).not.toContain('class="dot');
  });
});
