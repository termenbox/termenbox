import { describe, expect, it } from 'vitest';
import type { AgentState, TabAgentState, TabInfo } from './protocol';
import type { WorkspaceAgent } from './agents';
import {
  aggregateStatus,
  focusedTab,
  followFocus,
  liveTab,
  tabForPrompt,
  loadActiveTab,
  moveTab,
  relativeDirProblem,
  saveActiveTab,
  tabOrder,
  tabStateKey,
  uniqueTabLabel,
} from './workspaceTabs';

const tab = (id: string, order: number, kind = 'shell'): TabInfo => ({ id, label: id, kind, dir: '', order });
const workspace = (tabs: TabInfo[]) => ({ tabs });
const agent = (state: WorkspaceAgent['state'], extra: Partial<WorkspaceAgent> = {}): WorkspaceAgent =>
  ({ state, seq: 1, stale: false, ts: 1, ...extra });

describe('routing an answer to the tab that asked', () => {
  const state = (tab: string, promptId: string): TabAgentState => ({
    workspace: 'w', tab, state: 'needs_input', ts: 1,
    prompt: { id: promptId, kind: 'yes_no', choices: ['Yes', 'No'] },
  });
  it('uses the prompt id and never another tab that happens to be waiting', () => {
    const states = [state('t1', 'p1'), state('t2', 'p2')];
    expect(tabForPrompt(states, 'p2')).toBe('t2');
    expect(tabForPrompt(states, 'gone')).toBeNull();
    expect(tabForPrompt([state('t1', 'p1')], 'gone')).toBeNull();
  });
});

describe('the workspace aggregate', () => {
  it('follows the server states: needs you, error, all working, idle, done, and a shell-only workspace', () => {
    const tabs = [tab('t1', 0, 'shell'), tab('t2', 1, 'claude'), tab('t3', 2, 'codex')];
    const w = workspace(tabs);
    const states = (a: AgentState, b: AgentState): Record<string, AgentState> => ({
      [tabStateKey('s/w', 't2')]: a,
      [tabStateKey('s/w', 't3')]: b,
    });
    expect(aggregateStatus('s/w', w, undefined, states('needs_input', 'working'))).toBe('needs-you');
    expect(aggregateStatus('s/w', w, undefined, states('error', 'working'))).toBe('failed');
    expect(aggregateStatus('s/w', w, undefined, states('working', 'working'))).toBe('working');
    expect(aggregateStatus('s/w', w, undefined, states('working', 'idle'))).toBe('idle');
    expect(aggregateStatus('s/w', w, undefined, states('done', 'done'))).toBe('finished');
    expect(aggregateStatus('s/w', workspace([tab('t1', 0)]), undefined)).toBe('idle');
  });

  it('falls back to the workspace agent on a server without per-tab states', () => {
    const w = workspace([tab('t1', 0, 'claude')]);
    expect(aggregateStatus('s/w', w, agent('working'))).toBe('working');
    expect(aggregateStatus('s/w', w, agent('unknown', { serverStatus: true, attention: { id: 'a', workspace: 'w', kind: 'failed', raised_at: 1 } }))).toBe('failed');
    expect(aggregateStatus('s/w', w, agent('needs_input'))).toBe('needs-you');
    expect(aggregateStatus('s/w', w, undefined)).toBe('idle');
  });
});

describe('the tabs a device opens', () => {
  const tabs = [tab('t2', 1), tab('t1', 0), tab('t3', 2)];
  it('orders by the server order and falls back from the focus to the first', () => {
    expect(tabOrder(tabs).map((t) => t.id)).toEqual(['t1', 't2', 't3']);
    expect(focusedTab(tabs, 't2')?.id).toBe('t2');
    expect(focusedTab(tabs, 'gone')?.id).toBe('t1');
    expect(focusedTab([], 't1')).toBeUndefined();
  });
  it('keeps the device tab when it still exists and otherwise uses the focus', () => {
    expect(liveTab(tabs, 't3', 't2')?.id).toBe('t3');
    expect(liveTab(tabs, 'gone', 't2')?.id).toBe('t2');
    expect(liveTab(tabs, null, 'gone')?.id).toBe('t1');
  });
  it('follows a focus that moved to another live tab, and nothing else', () => {
    expect(followFocus(tabs, 't1', 't3')).toBe('t3');
    expect(followFocus(tabs, 't3', 't3')).toBeNull();
    expect(followFocus(tabs, 't3', 'gone')).toBeNull();
    expect(followFocus(tabs, 't3', null)).toBeNull();
    expect(followFocus([], 't1', 't2')).toBeNull();
  });
  it('remembers the last tab per workspace on this device', () => {
    const map = new Map<string, string>();
    const store = {
      getItem: (k: string) => map.get(k) ?? null,
      setItem: (k: string, v: string) => void map.set(k, v),
    };
    saveActiveTab('s/w', 't2', store);
    saveActiveTab('s/other', 't9', store);
    expect(loadActiveTab('s/w', store)).toBe('t2');
    expect(loadActiveTab('s/other', store)).toBe('t9');
    expect(loadActiveTab('s/none', store)).toBeNull();
  });
});

describe('tab editing', () => {
  it('moves a tab and leaves the others in order', () => {
    const tabs = [tab('t1', 0), tab('t2', 1), tab('t3', 2)];
    expect(moveTab(tabs, 't3', 0).map((t) => t.id)).toEqual(['t3', 't1', 't2']);
    expect(moveTab(tabs, 't1', 2).map((t) => t.id)).toEqual(['t2', 't3', 't1']);
    expect(moveTab(tabs, 'gone', 0).map((t) => t.id)).toEqual(['t1', 't2', 't3']);
  });
  it('names a new tab without colliding', () => {
    expect(uniqueTabLabel('Claude Code', [tab('t1', 0)])).toBe('Claude Code');
    expect(uniqueTabLabel('Claude Code', [{ ...tab('t1', 0), label: 'Claude Code' }])).toBe('Claude Code 2');
  });
  it('refuses an absolute or escaping subdirectory and accepts a relative one', () => {
    expect(relativeDirProblem('')).toBe('');
    expect(relativeDirProblem('packages/api')).toBe('');
    expect(relativeDirProblem('/etc')).not.toBe('');
    expect(relativeDirProblem('../outside')).not.toBe('');
    expect(relativeDirProblem('a/../../b')).not.toBe('');
  });
});
