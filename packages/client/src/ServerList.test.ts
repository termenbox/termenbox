import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import ServerList from './ServerList.svelte';
import type { HubServer } from './lib/hub';

const item = (id: string, name: string) => ({ server_id: id, server_url: '', server_name: name, registered_at: 0 });
const state = (id: string, over: Partial<HubServer>): HubServer => ({ id, server: item(id, id), state: 'connected', reason: '', version: '0.1.51', backends: ['tmux'], acpAgents: [], controlMode: true, tmuxVersion: '3.8', prefix: 'C-b', ...over });
const props = { servers: [item('a', 'alpha'), item('b', 'beta')], states: [state('a', { path: 'Direct, LAN' }), state('b', { state: 'unreachable', reason: 'No route to host' })], counts: { a: 3, b: 0 }, onpause() {}, onresume() {}, onretry() {}, onadd() {}, onremove() {}, onshare() {} };

describe('the servers page', () => {
  const out = render(ServerList, { props }).body;
  it('puts every action behind one menu button per server and no action buttons in the row', () => {
    expect((out.match(/data-testid="server-menu"/g) ?? []).length).toBe(2);
    expect(out).not.toMatch(/>\s*Pause\s*</);
    expect(out).not.toContain('Remove server');
    expect(out).not.toContain('Send to another device');
  });
  it('counts the sessions of a connected server', () => {
    expect(out).toContain('3 sessions');
  });
  it('keeps the connection error out of the row, behind an icon only a failing server has', () => {
    expect((out.match(/data-testid="server-issue"/g) ?? []).length).toBe(1);
    expect(out).toContain('data-severity="error"');
    expect(out).not.toContain('No route to host');
    expect(out).not.toContain('Why can&#39;t I connect');
  });
  it('does not lead back to Sessions or draw an icon before the name', () => {
    expect(out).not.toContain('← Sessions');
    expect(out).not.toContain('linear-gradient');
  });
});
