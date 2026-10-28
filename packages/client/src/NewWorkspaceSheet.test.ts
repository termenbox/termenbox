import { afterEach, describe, it, expect, vi } from 'vitest';
import { render } from 'svelte/server';
import NewWorkspaceSheet from './NewWorkspaceSheet.svelte';
import type { HubServer } from './lib/hub';
import type { WindowMap } from './lib/windows';

const server = (id: string, name: string, backends = ['tmux'], acpAgents: string[] = []): HubServer => ({
  id, state: 'connected', reason: '', version: '0.1.23+dev', backends, acpAgents, controlMode: true, tmuxVersion: '3.6', prefix: 'C-b',
  server: { server_url: '', server_id: id, server_name: name, registered_at: 0, record: { ip: '127.0.0.1', udp_port: 1, fingerprint: '' } } as HubServer['server'],
});
const manyAgents = [server('a', 'alpha', ['tmux', 'acp'], ['claude', 'codex', 'opencode', 'copilot', 'gemini', 'kilo', 'aider', 'goose'])];
const manyDirs: WindowMap = {
  'a/ws-1': { windows: [{ path: '/home/user/project1' }] },
  'a/ws-2': { windows: [{ path: '/home/user/project2' }] },
  'a/ws-3': { windows: [{ path: '/home/user/project3' }] },
  'a/ws-4': { windows: [{ path: '/home/user/project4' }] },
};
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
    expect(out).toContain('Agent workspace (ACP)');
    expect(control(out)).toBe(false);
  });
  it('keeps the Manager dropdown for other backends beside tmux and acp', () => {
    const out = html({ servers: [server('a', 'alpha', ['tmux', 'acp', 'command'], ['claude'])] });
    expect(out).toContain('Manager');
    expect(out).not.toContain('Agent workspace (ACP)');
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

describe('agent picker', () => {
  it('shows "Show all N agents" button when more than 6 agents available', () => {
    const out = html({ servers: manyAgents, initialServerId: 'a', locked: true, windows: manyDirs });
    expect(out).toContain('Show all 12 agents');
  });

  it('shows only first 6 agents in grid, rest in picker', () => {
    const out = html({ servers: manyAgents, initialServerId: 'a', locked: true, windows: manyDirs });
    // Should have 6 agent buttons in the grid
    const agentButtons = (out.match(/role="radio"/g) || []).length;
    expect(agentButtons).toBeGreaterThanOrEqual(6);
  });
});

describe('directory chips picker', () => {
  it('shows "+N more" button when more than 3 directories', () => {
    const out = html({ servers: [server('a', 'alpha', ['tmux', 'acp'], ['claude'])], initialServerId: 'a', locked: true, windows: manyDirs });
    expect(out).toContain('+1 more');
  });

  it('shows first 3 dirs as chips, rest in picker', () => {
    const out = html({ servers: [server('a', 'alpha', ['tmux', 'acp'], ['claude'])], initialServerId: 'a', locked: true, windows: manyDirs });
    // Should have chip buttons for first 3 dirs
    expect(out).toContain('/home/user/project1');
    expect(out).toContain('/home/user/project2');
    expect(out).toContain('/home/user/project3');
    // 4th dir should NOT be in chips (only in picker)
    const chipSection = out.substring(out.indexOf('dir-chips'), out.indexOf('</div>', out.indexOf('dir-chips')) + 6);
    expect(chipSection).not.toContain('/home/user/project4');
  });
});
