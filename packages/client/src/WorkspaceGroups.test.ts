import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import WorkspaceGroups from './WorkspaceGroups.svelte';
import type { HubWorkspace } from './lib/hub';
import type { AgentMap, WorkspaceAgent } from './lib/agents';

// Server-side render of the Workspaces list; dragging, the arrow keys and the touch targets are measured in e2e/reorder.mjs.
const ws = (serverId: string, id: string, tabs = 0): HubWorkspace => ({ id, name: id, agent: null, serverId, serverName: `Host ${serverId}`, key: `${serverId}/${id}`, tabs: Array.from({ length: tabs }, (_, i) => ({ id: `t${i + 1}`, label: `t${i + 1}`, kind: 'shell', dir: '', order: i })) });
const agent = (state: WorkspaceAgent['state']): WorkspaceAgent => ({ state, seq: 1, ts: 0, stale: false });
const html = (workspaces: HubWorkspace[], agents: AgentMap = {}, extra: Record<string, unknown> = {}) => render(WorkspaceGroups, { props: { workspaces, agents, now: 1000, onopen() {}, onnew() {}, ...extra } }).body;
const names = (out: string) => [...out.matchAll(/<b[^>]*>([^<]*)<\/b>/g)].map((m) => m[1]);
const count = (out: string, needle: string) => out.split(needle).length - 1;

describe('the Workspaces list', () => {
  it('lists workspaces in the order given: one that needs you does not jump to the top, but still says so', () => {
    const out = html([ws('s', 'calm'), ws('s', 'asks'), ws('s', 'busy')], { 's/asks': agent('needs_input'), 's/busy': agent('working') });
    expect(names(out)).toEqual(['calm', 'asks', 'busy']);
    expect(out).toContain('Needs you');
    expect(out).toContain('Working');
  });

  it('follows a reordered input exactly', () => {
    expect(names(html([ws('s', 'c'), ws('s', 'a'), ws('s', 'b')]))).toEqual(['c', 'a', 'b']);
  });

  it('gives each row a handle and an actions button when several share a server', () => {
    const out = html([ws('s', 'one'), ws('s', 'two')], {}, { onactions() {} });
    expect(count(out, 'data-testid="workspace-grip"')).toBe(2);
    expect(count(out, 'data-testid="workspace-actions"')).toBe(2);
    expect(out).toContain('Reorder one');
    expect(out).toContain('Actions for two');
  });

  it('offers a pencil only when a rename handler is given, and counts the tabs', () => {
    const out = html([ws('s', 'two', 2)], {}, { onrename() {} });
    expect(count(out, 'data-testid="workspace-rename"')).toBe(1);
    expect(out).toContain('Rename two');
    expect(out).toContain('2 tabs');
    expect(html([ws('s', 'two', 1)], {}, { onrename() {} })).toContain('1 tab');
    expect(html([ws('s', 'two', 2)])).not.toContain('workspace-rename');
  });

  it('has no handle for a server\'s only workspace, and no actions button without a handler', () => {
    const out = html([ws('s', 'only')]);
    expect(out).not.toContain('workspace-grip');
    expect(out).not.toContain('workspace-actions');
  });

  it('names servers only when there are several, and keeps rows with their own server', () => {
    expect(html([ws('s', 'only')])).not.toContain('<h2');
    const out = html([ws('a', 'x'), ws('b', 'z'), ws('a', 'y')]);
    expect([...out.matchAll(/<h2[^>]*>([^<]*)</g)].map((m) => m[1].replace(/<!---->/g, '').trim())).toEqual(['Host a · 2', 'Host b · 1']);
    expect(names(out)).toEqual(['x', 'y', 'z']);
  });

  it('marks rows so the drag knows which list a row belongs to, and has a live region for moves', () => {
    const out = html([ws('a', 'x'), ws('a', 'y')]);
    expect(out).toContain('data-row="a/x"');
    expect(out).toContain('data-list="a"');
    expect(out).toMatch(/role="status"/);
  });

  it('offers a new workspace even when the list has rows', () => {
    const out = html([ws('s', 'only')]);
    expect(out).toContain('data-testid="workspaces-new"');
    expect(out).toContain('+ New');
  });

  it('says there are none yet, with a way to start one', () => {
    const out = html([]);
    expect(out).toContain('No workspaces yet');
    expect(out).toContain('Start a workspace');
  });
});
