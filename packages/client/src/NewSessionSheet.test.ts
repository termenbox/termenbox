import { afterEach, describe, it, expect, vi } from 'vitest';
import { render } from 'svelte/server';
import NewSessionSheet from './NewSessionSheet.svelte';
import type { HubServer } from './lib/hub';

const server = (id: string, name: string, backends = ['tmux'], acpAgents: string[] = []): HubServer => ({
  id, state: 'connected', reason: '', version: '0.1.23+dev', backends, acpAgents, controlMode: true, tmuxVersion: '3.6', prefix: 'C-b',
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

describe('the View control', () => {
  afterEach(() => vi.unstubAllGlobals());
  const both = [server('a', 'alpha', ['tmux', 'acp'], ['claude', 'opencode', 'mine'])];
  const control = (out: string) => out.includes('data-testid="view"');
  const on = (out: string, id: string) => new RegExp(`data-testid="view-${id}"[^>]*aria-checked="true"|aria-checked="true"[^>]*data-testid="view-${id}"`).test(out);

  it('shows Terminal and Agent view for an agent with an ACP form, Terminal selected', () => {
    const out = html({ servers: both });
    expect(control(out)).toBe(true);
    expect(out).toContain("agent's own TUI");
    expect(out).toContain('Agent view');
    expect(on(out, 'terminal')).toBe(true);
    expect(on(out, 'acp')).toBe(false);
    expect(out).not.toContain('Manager');
  });
  it('is absent when the server has no ACP agent for the template or only one view', () => {
    expect(control(html({ servers: [server('a', 'alpha', ['tmux', 'acp'], [])] }))).toBe(false);
    expect(control(html({ servers: [server('a', 'alpha')] }))).toBe(false);
  });
  it('is absent on a server with acp and no tmux, whose templates are the ACP agents', () => {
    const out = html({ servers: [server('a', 'alpha', ['acp'], ['opencode', 'claude'])] });
    expect(control(out)).toBe(false);
    expect(out).not.toContain('Manager');
    expect(out).toContain('OpenCode');
    expect(out).not.toContain('Codex');
  });
  it('keeps the Manager dropdown for command beside acp when there is no tmux', () => {
    const out = html({ servers: [server('a', 'alpha', ['command', 'acp'], ['claude'])] });
    expect(out).toContain('Manager');
    expect(out).toContain('Agent session (ACP)');
    expect(control(out)).toBe(false);
  });
  it('keeps the Manager dropdown for other backends beside tmux and acp', () => {
    const out = html({ servers: [server('a', 'alpha', ['tmux', 'acp', 'command'], ['claude'])] });
    expect(out).toContain('Manager');
    expect(out).not.toContain('Agent session (ACP)');
    expect(control(out)).toBe(true);
  });
  it('shows the remembered view', () => {
    const st: Record<string, string> = { termenbox_new_session_view: '{"a/claude":"acp"}' };
    vi.stubGlobal('localStorage', { getItem: (k: string) => st[k] ?? null, setItem: () => {} });
    const out = html({ servers: both });
    expect(on(out, 'acp')).toBe(true);
    expect(on(out, 'terminal')).toBe(false);
  });
  it('falls back to Terminal when the remembered agent is no longer listed', () => {
    const st: Record<string, string> = { termenbox_new_session_view: '{"a/claude":"acp"}' };
    vi.stubGlobal('localStorage', { getItem: (k: string) => st[k] ?? null, setItem: () => {} });
    const out = html({ servers: [server('a', 'alpha', ['tmux', 'acp'], ['opencode'])] });
    expect(control(out)).toBe(false);
  });
  it('still renders, on Terminal, when storage throws', () => {
    vi.stubGlobal('localStorage', { getItem: () => { throw new Error('denied'); }, setItem: () => { throw new Error('denied'); } });
    const out = html({ servers: both });
    expect(control(out)).toBe(true);
    expect(on(out, 'terminal')).toBe(true);
  });
});
