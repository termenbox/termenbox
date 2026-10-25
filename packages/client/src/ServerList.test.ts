import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import ServerList from './ServerList.svelte';
import type { HubServer } from './lib/hub';

const item = (id: string, name: string) => ({ server_id: id, server_url: '', server_name: name, registered_at: 0 });
const state = (id: string, over: Partial<HubServer>): HubServer => ({ id, server: item(id, id), state: 'connected', reason: '', version: '0.1.51', updateAvailable: '', updating: false, backends: ['tmux'], acpAgents: [], controlMode: true, tmuxVersion: '3.8', prefix: 'C-b', ...over });
const props = { servers: [item('a', 'alpha'), item('b', 'beta')], states: [state('a', { path: 'Direct, LAN' }), state('b', { state: 'unreachable', reason: 'No route to host' })], counts: { a: 3, b: 0 }, onpause() {}, onresume() {}, onretry() {}, onadd() {}, onremove() {}, onshare() {} };

describe('the servers page', () => {
  const out = render(ServerList, { props }).body;
  it('puts every action behind one menu button per server and no action buttons in the row', () => {
    expect((out.match(/data-testid="server-menu"/g) ?? []).length).toBe(2);
    expect(out).not.toMatch(/>\s*Pause\s*</);
    expect(out).not.toContain('Remove server');
    expect(out).not.toContain('Send to another device');
  });
  it('counts the workspaces of a connected server', () => {
    expect(out).toContain('3 workspaces');
  });
  it('keeps the connection error out of the row, behind an icon only a failing server has', () => {
    expect((out.match(/data-testid="server-issue"/g) ?? []).length).toBe(1);
    expect(out).toContain('data-severity="error"');
    expect(out).not.toContain('No route to host');
    expect(out).not.toContain('Why can&#39;t I connect');
  });
  it('does not lead back to Workspaces or draw an icon before the name', () => {
    expect(out).not.toContain('← Workspaces');
    expect(out).not.toContain('linear-gradient');
  });
});

describe('a server card', () => {
  const card = (over: Partial<HubServer>) => render(ServerList, { props: { ...props, servers: [item('a', 'alpha')], states: [state('a', { path: 'Direct, internet', version: '0.1.53+abc1234', ...over })], counts: { a: 2 } } }).body;
  it('shows the version next to the name, without the build', () => {
    const out = card({});
    expect(out).toMatch(/<h3[^>]*>alpha<\/h3>(?:\s|<!--.*?-->)*<span class="version[^"]*"[^>]*>v0\.1\.53<\/span>/);
    expect(out).not.toContain('abc1234');
  });
  it('offers the update only when the server is connected and has a newer release', () => {
    expect(card({})).not.toContain('server-update');
    const out = card({ updateAvailable: '0.1.54' });
    expect(out).toContain('data-testid="server-update"');
    expect(out).toContain('Update alpha to v0.1.54');
    expect(card({ updateAvailable: '0.1.54', state: 'unreachable' })).not.toContain('server-update"');
  });
  it('says Updating instead of offering the update again', () => {
    const out = card({ updateAvailable: '0.1.54', updating: true });
    expect(out).toContain('data-testid="server-updating"');
    expect(out).not.toContain('data-testid="server-update"');
  });
  it('writes the state as separate parts, so the separators do not depend on spaces', () => {
    const out = card({});
    expect(out).toContain('Connected</span>');
    expect(out).toMatch(/class="part sep[^"]*">Direct, internet<\/span>/);
    expect(out).toContain('2 workspaces</span>');
    expect(out).not.toContain('Connected·');
  });
});
