import { describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import NewWorkspaceSheet from './NewWorkspaceSheet.svelte';
import type { HubServer } from './lib/hub';
import type { WindowMap } from './lib/windows';

const server = (id: string, name: string, backends = ['tmux'], acpAgents: string[] = []): HubServer => ({
  id, state: 'connected', reason: '', version: '0.1.23+dev', backends, tabs: true, acpAgents, controlMode: true, tmuxVersion: '3.6', prefix: 'C-b',
  server: { server_url: '', server_id: id, server_name: name, registered_at: 0, record: { ip: '127.0.0.1', udp_port: 1, fingerprint: '' } } as HubServer['server'],
});
const manyAgents = [server('a', 'alpha', ['tmux', 'acp'], ['claude', 'codex', 'opencode', 'copilot', 'gemini', 'kilo', 'aider', 'goose'])];
const manyDirs = {
  'a/ws-1': { windows: [{ path: '/home/user/project1' }] },
  'a/ws-2': { windows: [{ path: '/home/user/project2' }] },
  'a/ws-3': { windows: [{ path: '/home/user/project3' }] },
  'a/ws-4': { windows: [{ path: '/home/user/project4' }] },
} as unknown as WindowMap; // fixtures carry only the path
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
    expect(out).toContain('data-testid="agents-more"');
    expect(out).toMatch(/data-testid="agents-more"[\s\S]*?<small[^>]*>12</);
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

describe('the agent row', () => {
  const tmuxOnly = [server('a', 'alpha')];
  it('shows three tiles and a More tile that counts every agent', () => {
    const out = html({ servers: manyAgents, initialServerId: 'a', locked: true });
    expect(out.match(/data-testid="agent-tile"/g)).toHaveLength(3);
    expect(out).toMatch(/data-testid="agents-more"[\s\S]*?<small[^>]*>12</);
    expect(out).not.toContain('Show all');
  });
  it('starts on Claude Code and offers claude, codex and shell on a device with no history', () => {
    const out = html({ servers: tmuxOnly });
    expect(out).toMatch(/aria-checked="true"[^>]*data-testid="agent-tile"[\s\S]*?Claude Code/);
    for (const label of ['Claude Code', 'Codex', 'Shell']) expect(out).toContain(label);
  });
  it('preselects the last used agent', () => {
    vi.stubGlobal('localStorage', { getItem: (k: string) => (k === 'termenbox_recent_agents' ? '{"a":["codex"]}' : null), setItem: () => {} });
    try {
      const out = html({ servers: tmuxOnly });
      expect(out).toMatch(/aria-checked="true"[^>]*data-testid="agent-tile"[\s\S]*?Codex/);
    } finally { vi.unstubAllGlobals(); }
  });
});

describe('the folder field', () => {
  const one = [server('a', 'alpha', ['tmux', 'acp'], ['claude'])];
  it('has no free-text path input, only a readonly field and a Choose button', () => {
    const out = html({ servers: one, initialServerId: 'a', locked: true });
    expect(out).not.toContain('placeholder="/home/you/project');
    expect(out).toContain('aria-label="Folder"');
    expect(out).toContain('data-testid="folder-change"');
    expect(out).toContain('Choose a folder');
    expect(out).not.toContain('type="file"');
  });
  it('seeds the chips from the folders of open workspaces and keeps three of them with a More chip', () => {
    const out = html({ servers: one, initialServerId: 'a', locked: true, windows: manyDirs });
    expect(out.match(/data-testid="dir-chip"/g)).toHaveLength(3);
    expect(out).toContain('data-testid="dirs-more"');
  });
  it('saves no recents while the dialog is only open (they are written by create())', () => {
    const st: Record<string, string> = {};
    vi.stubGlobal('localStorage', { getItem: (k: string) => st[k] ?? null, setItem: (k: string, v: string) => { st[k] = v; } });
    try {
      html({ servers: one, initialServerId: 'a', locked: true, windows: manyDirs });
      expect(st.termenbox_recent_agents).toBeUndefined(); // create() is the only writer; its click path is asserted in the browser (5.1)
      expect(st.termenbox_recent_dirs).toBeUndefined();
    } finally { vi.unstubAllGlobals(); }
  });
});

describe('the footer', () => {
  it('summarises the agent and the folder and says so politely', () => {
    const out = html({ servers: [server('a', 'alpha')] });
    expect(out).toMatch(/aria-live="polite"[^>]*data-testid="summary"[^>]*>Claude Code in the default folder</);
  });
  it('hides the Server select for one server and shows it for several', () => {
    expect(html({ servers: [server('a', 'alpha')] })).not.toContain('Server <');
    expect(html({ servers: [server('a', 'alpha'), server('b', 'beta')] })).toContain('Server <');
  });
  it('keeps Create disabled with the reason visible for a server without tmux', () => {
    const out = html({ servers: [server('a', 'alpha', ['acp'], ['claude'])], initialServerId: 'a', locked: true });
    expect(out).toContain('runs no terminals');
    expect(out).toMatch(/<button[^>]*disabled[^>]*>Create<|<button type="submit"[^>]*disabled/);
  });
});
