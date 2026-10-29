import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import NewTabSheet from './NewTabSheet.svelte';
import type { HubWorkspace } from './lib/hub';

const workspace: HubWorkspace = {
  id: 'w', name: 'Work', agent: null, serverId: 's', serverName: 'Host', key: 's/w', dir: '/srv/app',
  tabs: [{ id: 't1', label: 'Shell', kind: 'shell', dir: '', order: 0, window: '@1' }],
};
const out = render(NewTabSheet, { props: { workspace, oncreate() {}, onclose() {} } }).body;

describe('the new tab sheet', () => {
  it('offers the terminal templates, a name and a directory inside the workspace', () => {
    expect(out).toContain('New tab in Work');
    expect(out).toContain('Claude Code');
    expect(out).toContain('Cursor');
    expect(out).toContain('optional, like packages/api');
    expect(out).toContain('/srv/app');
    expect(out).not.toContain('Manager');
    expect(out).not.toContain('Agent view');
  });
  it('suggests a name that does not collide with the open tabs', () => {
    const withShell = render(NewTabSheet, {
      props: { workspace: { ...workspace, tabs: [{ ...workspace.tabs![0], label: 'Shell' }] }, oncreate() {}, onclose() {} },
    }).body;
    expect(withShell).toContain('value="Shell 2"');
  });
});
