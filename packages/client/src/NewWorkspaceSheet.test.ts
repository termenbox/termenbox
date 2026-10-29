import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import NewWorkspaceSheet from './NewWorkspaceSheet.svelte';
import type { HubServer } from './lib/hub';

const server = (id: string, name: string, backends = ['tmux']): HubServer => ({
  id, state: 'connected', reason: '', version: '0.1.23+dev', backends, tabs: true, acpAgents: [], controlMode: true, tmuxVersion: '3.6', prefix: 'C-b',
  server: { server_url: '', server_id: id, server_name: name, registered_at: 0, record: { ip: '127.0.0.1', udp_port: 1, fingerprint: '' } } as HubServer['server'],
});
const html = (props: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(NewWorkspaceSheet, { props: { workspaces: [], windows: {}, oncreate: () => {}, onclose: () => {}, ...props } }).body;

describe('the new workspace sheet', () => {
  const two = [server('a', 'alpha'), server('b', 'beta')];
  it('asks which server when there are several and none was asked for', () => {
    const out = html({ servers: two });
    expect(out).toContain('<select');
    expect(out).toContain('alpha');
    expect(out).toContain('beta');
    expect(out).toContain('New workspace</h2>');
  });
  it('starts on the server it was opened for', () => {
    expect(html({ servers: two, initialServerId: 'b' })).toMatch(/<option[^>]*value="b"[^>]*selected/);
  });
  it('is fixed to a server when opened from that server and says so', () => {
    const out = html({ servers: two, initialServerId: 'b', locked: true });
    expect(out).not.toContain('<select');
    expect(out).toContain('New workspace on beta');
  });
});

describe('the first tab', () => {
  it('offers every terminal template and no Manager or View', () => {
    const out = html({ servers: [server('a', 'alpha', ['tmux', 'acp'])] });
    expect(out).toContain('Shell');
    expect(out).toContain('Claude Code');
    expect(out).toContain('Cursor');
    expect(out).not.toContain('Manager');
    expect(out).not.toContain('Agent view');
    expect(out).not.toContain('data-testid="view"');
  });
  it('marks the default template as the first tab', () => {
    const out = html({ servers: [server('a', 'alpha')] });
    expect(out).toMatch(/aria-checked="true"[^>]*>[\s\S]*?Claude Code|Claude Code[\s\S]*?aria-checked="true"/);
  });
  it('says a server without tmux cannot start a terminal workspace', () => {
    const out = html({ servers: [server('a', 'alpha', ['acp'])] });
    expect(out).toContain('runs no terminals');
    expect(out).toMatch(/<button[^>]*type="submit"[^>]*disabled/);
  });
});
