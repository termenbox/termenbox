import { describe, expect, it } from 'vitest';
import { aggregateStatus, alerts, applyStatusFrame, bgBadge, subReasonWord, tierOf } from './status';
import { tabStateKey } from './workspaceTabs';
import type { WorkspaceAgent } from './agents';
import type { AgentState, Attention, StatusFrame, TabInfo } from './protocol';

const att = (rest: Partial<Attention> = {}): Attention => ({ id: 'a', workspace: 'w', kind: 'needs_input', raised_at: 5, ...rest });
const frame = (rest: Partial<StatusFrame> = {}): StatusFrame => ({ v: 1, workspaces: [], attention: [], tabs: [], ...rest });

describe('applyStatusFrame', () => {
  it('lands the activity, the attention and the tab states together', () => {
    const f = frame({
      workspaces: [{ workspace: 'w', activity: 'working', ts: 10, sub_reason: 'tool', bg: 2 }],
      attention: [att({ kind: 'failed', tier: 'result', cause: 'rate_limit' })],
      tabs: [{ workspace: 'w', tab: 't1', state: 'working', ts: 10 }],
    });
    const r = applyStatusFrame({ agents: {}, tabStates: {} }, f, 100);
    expect(r.agents.w.subReason).toBe('tool');
    expect(r.agents.w.bg).toBe(2);
    expect(r.agents.w.attention?.cause).toBe('rate_limit');
    expect(r.agents.w.state).toBe('error');
    expect(Object.keys(r.tabStates)).toEqual(['w/t1']);
  });

  it('replaces the open items and the tabs: what the frame lacks is gone', () => {
    const first = applyStatusFrame({ agents: {}, tabStates: {} }, frame({ attention: [att()], tabs: [{ workspace: 'w', tab: 't1', state: 'needs_input', ts: 1 }] }), 1);
    const next = applyStatusFrame(first, frame({ workspaces: [{ workspace: 'w', activity: 'idle', ts: 2 }] }), 2);
    expect(next.agents.w.attention).toBeUndefined();
    expect(next.tabStates).toEqual({});
  });

  it('ignores a frame of a version it does not know', () => {
    const cur = { agents: {}, tabStates: {} };
    expect(applyStatusFrame(cur, frame({ v: 2, attention: [att()] }), 1)).toBe(cur);
  });
});

describe('selectors', () => {
  it('derives the tier from the kind when an older server sends none, and lets the server override', () => {
    expect(tierOf(att())).toBe('needs_you');
    expect(tierOf(att({ kind: 'finished_unseen' }))).toBe('result');
    expect(tierOf(att({ kind: 'failed' }))).toBe('result');
    expect(tierOf(att({ kind: 'finished_unseen', tier: 'info' }))).toBe('info');
  });

  it('alerts only for needs_you and result', () => {
    expect(alerts(att())).toBe(true);
    expect(alerts(att({ kind: 'finished_unseen', tier: 'result' }))).toBe(true);
    expect(alerts(att({ kind: 'finished_unseen', tier: 'info' }))).toBe(false);
    expect(alerts(att({ kind: 'unknown' }))).toBe(false);
  });

  it('words a sub-reason and falls back to Working; shows bg only when above zero', () => {
    expect(subReasonWord('retrying')).toBe('Retrying');
    expect(subReasonWord('unknown')).toBe('Working');
    expect(subReasonWord(undefined)).toBe('Working');
    expect(bgBadge(3)).toBe('bg: 3');
    expect(bgBadge(0)).toBe('');
    expect(bgBadge(undefined)).toBe('');
  });
});

const tab = (id: string, order: number, kind = 'shell'): TabInfo => ({ id, label: id, kind, dir: '', order });
const workspace = (tabs: TabInfo[]) => ({ tabs });
const agent = (state: WorkspaceAgent['state'], extra: Partial<WorkspaceAgent> = {}): WorkspaceAgent =>
  ({ state, seq: 1, stale: false, ts: 1, ...extra });

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
