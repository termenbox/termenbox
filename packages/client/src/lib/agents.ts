// The agent event store (plan v6, P.6): the latest state per workspace, kept up to date from the server's
// snapshot and event stream. No UI here (cards are v8). Text in events is untrusted: render it as text only.
import { get, writable } from 'svelte/store';
import type { Activity, AgentEvent, SubReason, AgentState, Attention, ClientMessage, ServerMessage, WorkspaceAgentState, Task } from './protocol';
import { settled, tierOf } from './status';

export interface WorkspaceAgent {
  state: AgentState;
  seq: number;
  /** The agent in the workspace, when the server's scan found one. */
  agent?: string;
  /** When the agent last answered (ms): the server's value, advanced by `done` and `message` events as they arrive. */
  lastResponseAt?: number;
  /** Start of the continuous needs-input/error wait; activity does not advance it. */
  waitingSince?: number;
  /** As the server saw it at the snapshot; use `isStale` for the live answer. */
  stale: boolean;
  /** Server time (ms) of the last event. */
  ts: number;
  tool?: { name: string; summary: string; phase: string };
  /** `seq` and `confidence` are the prompt event's: a one-tap answer carries the seq and needs `high`. */
  prompt?: { id: string; kind: string; choices: string[]; seq: number; confidence: string; /** Where the event came from (`acp` for a T0 workspace). */ source?: string };
  /** The last tool events, oldest first (capped). */
  timeline?: { name: string; summary: string; phase: string; ts: number }[];
  plan?: { text: string; status: string }[];
  message?: string;
  /** Set once the server sends Activity or Attention (decision 0095): `state` is then derived from them, not from raw events. */
  serverStatus?: boolean;
  activity?: Activity;
  /** Why the workspace works (decision 0188), and the background tasks still alive after its turn ended. */
  subReason?: SubReason;
  bg?: number;
  task?: Task;
  /** The most urgent open Attention item of the workspace. */
  attention?: Attention;
  /** Server clock minus this device's clock (ms) when the Activity arrived, for running times. */
  clockOffset?: number;
}

export type AgentMap = Record<string, WorkspaceAgent>;

const waiting = (state: AgentState) => state === 'needs_input' || state === 'error';

export const STALE_AFTER_MS = 60_000;
export const TIMELINE_CAP = 10;

export const agents = writable<AgentMap>({});

/** The latest state is `working` but nothing has been heard for a minute: it may be out of date. */
export function isStale(a: WorkspaceAgent, nowMs: number, skewMs = 0): boolean {
  // With server status a silent `working` already arrives as `unknown`.
  return !a.serverStatus && a.state === 'working' && (a.stale || nowMs + skewMs - a.ts > STALE_AFTER_MS);
}

/** Items to acknowledge (decision 0193: a human viewing it): `result` and `info` items of a workspace that is shown in
 *  the foreground of a visible page, in the tab that raised them. A `needs_you` item is never acknowledged by looking;
 *  only the ask going away clears it. Pure on purpose: no clock goes in, so no timer can acknowledge anything, and the
 *  caller re-evaluates only when the page, the active tab or the store changes. */
export function attentionToAck(a: WorkspaceAgent | undefined, foreground: boolean, pageVisible: boolean, done: Set<string>, visibleTab?: string | null): string[] {
  const it = a?.attention;
  if (!foreground || !pageVisible || !it || it.kind === 'needs_input' || tierOf(it) === 'needs_you' || done.has(it.id)) return [];
  // A finished or failed item belongs to the tab that raised it; looking at another tab is not seeing it.
  if (it.tab && it.tab !== visibleTab) return [];
  return [it.id];
}

/** How long the current task has run, on the server's clock; null without a task. */
export function runningMs(a: WorkspaceAgent, nowMs: number): number | null {
  return a.task ? Math.max(0, nowMs + (a.clockOffset ?? 0) - a.task.started_at) : null;
}

/** A snapshot replaces what we know about a workspace's state but keeps the details we already have. */
export function applySnapshot(map: AgentMap, states: WorkspaceAgentState[]): AgentMap {
  const next: AgentMap = {};
  for (const s of states) {
    const old = map[s.workspace];
    // A lower seq than ours means the server restarted its counters: what we hold is from before, drop it.
    const keep = old && old.seq <= s.seq ? old : {};
    next[s.workspace] = settled({ ...keep, state: s.state, seq: s.seq, stale: s.stale, ts: s.ts, agent: s.agent, lastResponseAt: s.last_response_at,
      waitingSince: waiting(s.state) ? s.waiting_since ?? (old && old.seq <= s.seq && waiting(old.state) ? old.waitingSince ?? old.ts : s.ts) : undefined });
  }
  return next;
}

/** Applies events in order. `gap` is true when a `seq` was skipped: ask the server for a new snapshot. */
export function applyEvents(map: AgentMap, events: AgentEvent[]): { map: AgentMap; gap: boolean } {
  let gap = false;
  const next: AgentMap = { ...map };
  for (const e of events) {
    const cur = next[e.workspace];
    if (cur && e.seq <= cur.seq) continue; // already seen
    if (e.seq !== (cur ? cur.seq + 1 : 1)) gap = true;
    const a: WorkspaceAgent = { ...(cur ?? { state: 'unknown' as AgentState }), seq: e.seq, ts: e.ts, stale: false } as WorkspaceAgent;
    const ev = e as Record<string, unknown>;
    switch (e.kind) {
      case 'state':
        if (!a.serverStatus) a.waitingSince = waiting(ev.state as AgentState) ? (waiting(a.state) ? a.waitingSince ?? cur?.ts ?? e.ts : e.ts) : undefined;
        if (ev.state === 'done' || (a.state === 'working' && (ev.state === 'idle' || ev.state === 'needs_input'))) a.lastResponseAt = e.ts;
        // With server status the state is derived from Activity and Attention; a raw event only moves the details.
        if (!a.serverStatus) a.state = ev.state as AgentState;
        if (ev.state !== 'needs_input') delete a.prompt;
        break;
      case 'tool':
        a.tool = { name: String(ev.name ?? ''), summary: String(ev.summary ?? ''), phase: String(ev.phase ?? '') };
        a.timeline = [...(a.timeline ?? []), { ...a.tool, ts: e.ts }].slice(-TIMELINE_CAP);
        break;
      case 'prompt':
        a.prompt = { id: String(ev.id ?? ''), kind: String(ev.prompt_kind ?? ''), choices: Array.isArray(ev.choices) ? ev.choices.map(String) : [], seq: e.seq, confidence: e.confidence, source: e.source };
        break;
      case 'plan':
        a.plan = Array.isArray(ev.items) ? ev.items.map((i: { text?: unknown; status?: unknown }) => ({ text: String(i.text ?? ''), status: String(i.status ?? '') })) : [];
        break;
      case 'message':
        a.message = String(ev.summary ?? '');
        a.lastResponseAt = e.ts;
        break;
      default:
        break; // a kind this client does not know
    }
    next[e.workspace] = a;
  }
  return { map: next, gap };
}

/** What to send in `agent_sync`: the last `seq` seen per workspace. */
export function syncRequest(map: AgentMap): Record<string, number> {
  return Object.fromEntries(Object.entries(map).map(([k, v]) => [k, v.seq]));
}

export function resetAgents() {
  agents.set({});
}

/** Feeds the store from the server's messages. Returns false for any other message (an old client simply never calls this). */
export function handleAgentMessage(msg: ServerMessage, send: (m: ClientMessage) => void): boolean {
  if (msg.type === 'agent_snapshot') {
    agents.update((m) => applySnapshot(m, msg.states));
    return true;
  }
  if (msg.type === 'agent_events') {
    const r = applyEvents(get(agents), msg.events);
    agents.set(r.map);
    // A skipped seq: take a fresh snapshot instead of guessing what was missed.
    if (r.gap) send({ type: 'agent_sync', after: syncRequest(r.map) });
    return true;
  }
  return false;
}

export function requestAgentSync(send: (m: ClientMessage) => void) {
  send({ type: 'agent_sync', after: syncRequest(get(agents)) });
}
