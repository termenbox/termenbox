import { describe, it, expect } from 'vitest';
import { render } from 'svelte/server';
import NewSessionSheet from './NewSessionSheet.svelte';
import type { HubServer } from './lib/hub';

const server = (id: string, name: string): HubServer => ({
  id, state: 'connected', reason: '', version: '0.1.23+dev', backends: ['tmux'], acpAgents: [], controlMode: true, tmuxVersion: '3.6', prefix: 'C-b',
  server: { server_url: '', server_id: id, server_name: name, registered_at: 0, record: { ip: '127.0.0.1', udp_port: 1, fingerprint: '' } } as HubServer['server'],
});
const html = (props: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(NewSessionSheet, { props: { sessions: [], windows: {}, oncreate: () => {}, onclose: () => {}, ...props } }).body;

describe('the new session sheet', () => {
  const two = [server('a', 'alpha'), server('b', 'beta')];
  it('asks which server when there are several and none was asked for', () => {
    const out = html({ servers: two });
    expect(out).toContain('<select');
    expect(out).toContain('alpha');
    expect(out).toContain('beta');
    expect(out).toContain('New session</h2>');
  });
  it('starts on the server it was opened for', () => {
    expect(html({ servers: two, initialServerId: 'b' })).toMatch(/<option[^>]*value="b"[^>]*selected/);
  });
  it('is fixed to a server when opened from that server and says so', () => {
    const out = html({ servers: two, initialServerId: 'b', locked: true });
    expect(out).not.toContain('<select');
    expect(out).toContain('New session on beta');
  });
});
