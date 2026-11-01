// The one place a status snapshot is applied (decisions 0096, 0187, 0191). The server sends a `status_frame`
// holding the Activity of every workspace, the open Attention items and every tab state, read at one instant; this
// module turns it into the client's stores and answers the questions each surface asks of them. An older server sends
// no frame: its `activity`, `attention` and `tab_states` messages keep the same stores up to date (hub.ts).
import type { AgentState, Attention, AttentionTier, StatusFrame, SubReason, TabAgentState, WorkspaceActivity } from './protocol';
import type { AgentMap, WorkspaceAgent } from './agents';
import type { HubWorkspace } from './hub';
import type { FleetState } from './ds/states';
import { tabOrder, tabStateKey } from './workspaceTabs';

/** The frame version this client reads. A newer one is ignored and the legacy messages answer. */
export const FRAME_VERSION = 1;

export interface StatusState {
  agents: AgentMap;
  tabStates: Record<string, TabAgentState>;
}

/** True when the client understands the frame. */
export const knowsFrame = (f: Pick<StatusFrame, 'v'>) => f.v === FRAME_VERSION;

/** Applies one frame: the three parts land together, so no surface sees a mix of two moments. */
export function applyStatusFrame(cur: StatusState, f: StatusFrame, receivedAt: number): StatusState {
  if (!knowsFrame(f)) return cur;
  let agents = applyActivity(cur.agents, f.workspaces, receivedAt);
  agents = applyAttention(agents, f.attention);
  const tabStates = Object.fromEntries(f.tabs.map((t) => [`${t.workspace}/${t.tab}`, t]));
  return { agents, tabStates };
}

/** The tier of an item: the server's, else derived from its kind (an older server). `info` is never loud. */
export function tierOf(it: Pick<Attention, 'kind' | 'tier'>): AttentionTier {
  if (it.tier && it.tier !== 'unknown') return it.tier;
  if (it.kind === 'needs_input') return 'needs_you';
  if (it.kind === 'failed' || it.kind === 'finished_unseen') return 'result';
  return 'unknown';
}

/** Whether an item may play a sound or a push: only `needs_you` and `result` (decision 0191). */
export const alerts = (it: Pick<Attention, 'kind' | 'tier'>) => {
  const t = tierOf(it);
  return t === 'needs_you' || t === 'result';
};

const REASON: Record<string, string> = {
  inferring: 'Thinking',
  tool: 'Running a tool',
  subagent: 'Running a subagent',
  compacting: 'Compacting',
  retrying: 'Retrying',
  quiet: 'Quiet for a while',
};

/** The words for a working session's sub-reason; an unknown or absent one is plain "Working". */
export const subReasonWord = (r: SubReason | undefined): string => (r && REASON[r]) || 'Working';

/** `bg: N` while background work lives after the turn ended; empty for none. */
export const bgBadge = (bg: number | undefined): string => (bg && bg > 0 ? `bg: ${bg}` : '');

const URGENCY: Record<string, number> = { needs_input: 0, failed: 1, finished_unseen: 2 };

/** With server status, the legacy `state` is what Activity and Attention say, so cards and tabs read one thing. */
export function settled(a: WorkspaceAgent): WorkspaceAgent {
  if (!a.serverStatus) return a;
  const kind = a.attention?.kind;
  let state: AgentState;
  if (kind === 'needs_input') state = 'needs_input';
  else if (kind === 'failed') state = 'error';
  else if (a.activity === 'working') state = 'working';
  else if (a.activity === 'blocked') state = 'needs_input';
  else if (a.activity === 'idle') state = kind === 'finished_unseen' ? 'done' : 'idle';
  else state = 'unknown';
  const waitingSince = kind === 'needs_input' || kind === 'failed' ? a.attention?.raised_at : undefined;
  return { ...a, state, stale: false, waitingSince };
}

/** Activity of workspaces, as the server derived it. An unknown activity value counts as `unknown`. */
export function applyActivity(map: AgentMap, workspaces: WorkspaceActivity[], receivedAt: number): AgentMap {
  const next: AgentMap = { ...map };
  for (const s of workspaces) {
    const cur = next[s.workspace] ?? { state: 'unknown' as AgentState, seq: 0, stale: false, ts: s.ts };
    next[s.workspace] = settled({ ...cur, serverStatus: true, activity: s.activity, task: s.task, subReason: s.sub_reason, bg: s.bg ?? 0, agent: s.agent ?? cur.agent,
      clockOffset: s.ts - receivedAt, ts: Math.max(cur.ts, s.ts) });
  }
  return next;
}

/** The server sends all open items at once: a workspace without one has none. */
export function applyAttention(map: AgentMap, items: Attention[]): AgentMap {
  const best = new Map<string, Attention>();
  for (const it of items) {
    const cur = best.get(it.workspace);
    if (!cur || (URGENCY[it.kind] ?? 9) < (URGENCY[cur.kind] ?? 9)) best.set(it.workspace, it);
  }
  const next: AgentMap = {};
  for (const [k, a] of Object.entries(map)) next[k] = settled({ ...a, serverStatus: true, attention: best.get(k) });
  for (const [k, it] of best) {
    if (!next[k]) next[k] = settled({ state: 'unknown', seq: 0, stale: false, ts: it.raised_at, serverStatus: true, attention: it });
  }
  return next;
}

/** `hub.tabStates` for one workspace, in the shape `aggregateStatus` reads. */
export function tabAgentStates(workspaceKey: string, states: Record<string, TabAgentState> = {}): Record<string, AgentState> {
  const prefix = `${workspaceKey}/`;
  const out: Record<string, AgentState> = {};
  for (const [key, state] of Object.entries(states)) if (key.startsWith(prefix)) out[key] = state.state;
  return out;
}

/**
 * The workspace's status from its tabs: Needs you → Error → Working only when every agent tab works → Idle →
 * Done/Unknown. Terminal tabs with no agent state are ignored; a workspace of shells is Idle. Without per-tab states
 * (an older server, or before the first `tab_states`) the workspace's own agent state answers.
 */
export function aggregateStatus(
  workspaceKey: string,
  workspace: Pick<HubWorkspace, 'tabs'>,
  agent: WorkspaceAgent | undefined,
  states: Record<string, AgentState> = {},
): FleetState {
  const known = tabOrder(workspace.tabs)
    .map((t) => states[tabStateKey(workspaceKey, t.id)])
    .filter((s): s is AgentState => !!s);
  if (known.length) {
    if (known.includes('needs_input')) return 'needs-you';
    if (known.includes('error')) return 'failed';
    if (known.every((s) => s === 'working')) return 'working';
    if (known.includes('idle')) return 'idle';
    if (known.includes('done')) return 'finished';
    return 'idle';
  }
  if (!agent) return 'idle';
  const attention = agent.attention?.kind;
  if (attention === 'needs_input') return 'needs-you';
  if (attention === 'failed') return 'failed';
  if (attention === 'finished_unseen') return 'finished';
  if (!agent.serverStatus && agent.state === 'needs_input') return 'needs-you';
  if (!agent.serverStatus && agent.state === 'error') return 'failed';
  if (agent.state === 'working') return 'working';
  if (!agent.serverStatus && agent.state === 'done') return 'finished';
  return 'idle';
}
