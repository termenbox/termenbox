// The one place a status snapshot is applied (decisions 0096, 0187, 0191). The server sends a `status_frame`
// holding the Activity of every workspace, the open Attention items and every tab state, read at one instant; this
// module turns it into the client's stores and answers the questions each surface asks of them. An older server sends
// no frame: its `activity`, `attention` and `tab_states` messages keep the same stores up to date (hub.ts).
import type { Attention, AttentionTier, SubReason, TabAgentState } from './protocol';
import type { StatusFrame } from './protocol';
import { applyActivity, applyAttention, type AgentMap } from './agents';

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
