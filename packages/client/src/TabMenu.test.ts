import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import TabMenu from './TabMenu.svelte';
import type { HubWorkspace } from './lib/hub';
import type { TabInfo } from './lib/protocol';

const tab = (id: string, label: string, order: number, kind = 'shell'): TabInfo => ({ id, label, order, kind, dir: '' });
const workspace: HubWorkspace = {
  id: 'w', name: 'Work', agent: null, serverId: 's', serverName: 'Host', key: 's/w',
  tabs: [tab('t2', 'Build', 1, 'claude'), tab('t1', 'Shell', 0)],
};
const html = (extra: Record<string, unknown> = {}) => render(TabMenu, {
  props: { groups: [{ workspace, tabs: workspace.tabs! }], activeKey: 's/w', activeTab: 't1', onopen() {}, onclose() {}, ...extra },
}).body;

describe('the tab menu', () => {
  it('lists the workspace tabs in server order, marks the current one and never offers Detach', () => {
    const out = html();
    expect(out.indexOf('Shell')).toBeLessThan(out.indexOf('Build'));
    expect(out).toContain('current tab');
    expect(out).not.toContain('Detach');
  });
  it('offers rename, delete, a new tab and Close workspace only when the handlers are given', () => {
    const out = html({ onrename() {}, onclosetab() {}, onnew() {}, oncloseworkspace() {} });
    expect(out).toContain('Actions for Shell');
    expect(out).toContain('+ New window');
    expect(out).toContain('Close workspace');
    const bare = html();
    expect(bare).not.toContain('Actions for Shell');
    expect(bare).not.toContain('+ New window');
    expect(bare).not.toContain('Close workspace');
  });
  it('groups tabs by workspace on the Workspaces page', () => {
    const other = { ...workspace, id: 'v', name: 'Docs', key: 's/v', tabs: [tab('t1', 'Docs shell', 0)] };
    const out = render(TabMenu, {
      props: { groups: [{ workspace, tabs: workspace.tabs! }, { workspace: other, tabs: other.tabs! }], onopen() {}, onclose() {} },
    }).body;
    expect(out).toContain('Work');
    expect(out).toContain('Docs');
    expect(out).toContain('Docs shell');
  });
});
