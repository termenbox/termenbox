import { describe, expect, it } from 'vitest';
import { FrozenOrder } from './cards';
import { tabIds, tabLabel, needsYouCount } from './tabs';
import type { AgentMap, WorkspaceAgent } from './agents';
import type { HubWorkspace } from './hub';

const workspace = (serverId: string, id: string): HubWorkspace => ({ id, name: id, agent: null, serverId, serverName: `Host ${serverId}`, key: `${serverId}/${id}` });
const state = (state: WorkspaceAgent['state']): WorkspaceAgent => ({ state, seq: 1, ts: 0, stale: false });
const workspaces = [workspace('a', 'one'), workspace('b', 'two'), workspace('b', 'three')];
const agents: AgentMap = { 'a/one': state('idle'), 'b/two': state('working'), 'b/three': state('needs_input') };

describe('tabs', () => {
  it('puts needs-you tabs first while preserving the order of everything else', () => {
    expect(tabIds(workspaces, agents)).toEqual(['b/three', 'a/one', 'b/two']);
    expect(needsYouCount({ ...agents, 'c/error': state('error') })).toBe(2);
  });
  it('freezes state changes, inserts and removals during touch and for two seconds after scroll', () => {
    const frozen = new FrozenOrder();
    const first = tabIds(workspaces, agents);
    expect(frozen.order(first, {}, 0)).toEqual(first);
    frozen.touchDown();
    const next = tabIds([workspaces[1], workspace('c', 'new')], { 'c/new': state('needs_input') });
    expect(frozen.order(next, {}, 1)).toEqual(first);
    frozen.touchUp(); frozen.scrolled(100);
    expect(frozen.order(next, {}, 2099)).toEqual(first);
    expect(frozen.order(next, {}, 2100)).toEqual(next);
  });
  it('names the server only for multiple saved servers and always includes an accessible state word', () => {
    expect(tabLabel(workspaces[0], agents['a/one'], 1, 0)).toBe('one · Idle');
    expect(tabLabel(workspaces[2], agents['b/three'], 3, 0)).toBe('three · Host b · Needs you');
    expect(tabLabel(workspaces[0], undefined, 2, 0)).toBe('one · Host a · No status');
  });
});
