// Home (decision 0100): the Needs-you queue, the four fleet counts and the Continue card, as pure functions so the
// ordering, the settle rule and the Continue rule are tested without a browser. Text from agents (task titles,
// prompts) is plain text: nothing here builds markup.
import type { AgentMap, WorkspaceAgent } from './agents';
import { bannerChoices } from './notify';
import type { HubWorkspace } from './hub';
import type { AttentionKind, TabAgentState } from './protocol';
import type { FleetState } from './ds/states';
import { aggregateStatus, tabAgentStates } from './status';
import { tabOrder, tabStateKey } from './workspaceTabs';

export type QueueKind = Exclude<AttentionKind, 'unknown'>;

export interface Row {
  key: string;
  name: string;
  serverId: string;
  serverName: string;
  agent?: string;
  state: FleetState;
  /** The open Attention kind, when the workspace is in the queue. */
  kind?: QueueKind;
  /** When the wait began (ms). */
  since?: number;
  /** The task title. */
  title?: string;
  /** The prompt a one-tap answer is attached to, captured when the row was built (an answer carries the prompt it was read from). */
  prompt?: NonNullable<WorkspaceAgent['prompt']>;
  /** The question of a needs_input, when the server sent one. */
  question?: string;
  /** The tab the answer belongs to (the server's focus); absent on a server without tabs. */
  tab?: string;
  /** How many tabs the workspace holds. */
  tabs?: number;
  /** The server this workspace runs on says it is unhealthy (U14). */
  unhealthy?: boolean;
}

const KIND_RANK: Record<QueueKind, number> = { needs_input: 0, failed: 1, finished_unseen: 2 };
const KIND_STATE: Record<QueueKind, FleetState> = { needs_input: 'needs-you', failed: 'failed', finished_unseen: 'finished' };

/** The open item of a workspace: the server's Attention, else (an older server) the raw state. */
export function kindOf(a: WorkspaceAgent | undefined): QueueKind | undefined {
  if (!a) return undefined;
  const k = a.attention?.kind;
  if (k && k !== 'unknown') return k;
  if (a.serverStatus) return undefined;
  return a.state === 'needs_input' ? 'needs_input' : a.state === 'error' ? 'failed' : undefined;
}

export function fleetState(a: WorkspaceAgent | undefined): FleetState {
  const kind = kindOf(a);
  if (kind) return KIND_STATE[kind];
  return a?.state === 'working' ? 'working' : 'idle';
}

export function rowOf(s: HubWorkspace, a: WorkspaceAgent | undefined, unhealthy = false, tabStates: Record<string, TabAgentState> = {}): Row {
  const kind = kindOf(a);
  const asking = kind === 'needs_input'
    ? tabOrder(s.tabs).find((t) => tabStates[tabStateKey(s.key, t.id)]?.state === 'needs_input')
    : undefined;
  return {
    key: s.key, name: s.name, serverId: s.serverId, serverName: s.serverName, agent: a?.agent,
    state: aggregateStatus(s.key, s, a, tabAgentStates(s.key, tabStates)), kind,
    since: kind ? a?.attention?.raised_at ?? a?.waitingSince ?? a?.ts : undefined,
    title: a?.attention?.task?.title ?? a?.task?.title,
    prompt: kind === 'needs_input' ? a?.prompt : undefined,
    question: kind === 'needs_input' ? a?.attention?.prompt : undefined,
    tab: asking?.id ?? s.focus,
    tabs: s.tabs?.length,
    unhealthy: unhealthy || undefined,
  };
}

/** `unhealthy` holds the ids of servers whose own verdict is unhealthy (U14); their rows carry the flag.
 *  Every open item across all servers: needs_input, then failed, then finished_unseen; the oldest wait first inside a kind. */
export function queueOf(workspaces: HubWorkspace[], agents: AgentMap, unhealthy: ReadonlySet<string> = new Set(), tabStates: Record<string, TabAgentState> = {}): Row[] {
  return workspaces.map((s, i) => ({ r: rowOf(s, agents[s.key], unhealthy.has(s.serverId), tabStates), i }))
    .filter((x) => x.r.kind)
    .sort((a, b) => KIND_RANK[a.r.kind!] - KIND_RANK[b.r.kind!] || (a.r.since ?? 0) - (b.r.since ?? 0) || a.i - b.i)
    .map((x) => x.r);
}

export interface HomeQueue {
  /** The oldest waiting decision, when one needs an answer. */
  hero?: Row;
  /** Everything else in the queue, in order. */
  rest: Row[];
}

/** The oldest waiting decision is the hero; a push's deep link (`focusKey`) puts its own workspace there while it still waits. */
export function splitQueue(queue: Row[], focusKey?: string | null): HomeQueue {
  const focused = focusKey ? queue.find((r) => r.key === focusKey && r.kind === 'needs_input') : undefined;
  const hero = focused ?? (queue[0]?.kind === 'needs_input' ? queue[0] : undefined);
  return { hero, rest: hero ? queue.filter((r) => r !== hero) : queue };
}

export type FleetCounts = Record<'needs-you' | 'working' | 'failed' | 'finished', number>;

export function fleetCounts(workspaces: HubWorkspace[], agents: AgentMap, tabStates: Record<string, TabAgentState> = {}): FleetCounts {
  const c: FleetCounts = { 'needs-you': 0, working: 0, failed: 0, finished: 0 };
  for (const s of workspaces) {
    const st = aggregateStatus(s.key, s, agents[s.key], tabAgentStates(s.key, tabStates));
    if (st !== 'idle') c[st]++;
  }
  return c;
}

/** The options the data already provides for the hero: a prompt the agent's own hook confirmed, at most three, never stale. */
export function heroChoices(row: Row, a: WorkspaceAgent | undefined, now: number): string[] {
  return a && row.prompt ? bannerChoices(a, now) : [];
}

/**
 * The Workspaces destination: every workspace, by server, in the user's own order (lib/order.ts). A state never moves a row;
 * it shows on the row. Servers without workspaces are left out.
 */
export function workspaceGroups(workspaces: HubWorkspace[], agents: AgentMap, unhealthy: ReadonlySet<string> = new Set(), tabStates: Record<string, TabAgentState> = {}): { serverId: string; title: string; rows: Row[] }[] {
  const servers = [...new Set(workspaces.map((s) => s.serverId))];
  return servers.map((id) => {
    const mine = workspaces.filter((s) => s.serverId === id);
    return { serverId: id, title: mine[0].serverName, rows: mine.map((s) => rowOf(s, agents[s.key], unhealthy.has(s.serverId), tabStates)) };
  });
}

/** The Continue card: the workspace last used, offered only when nothing needs a decision and its server is connected. */
export function continueTarget(lastKey: string | null, workspaces: HubWorkspace[], connected: Set<string>, queue: Row[]): HubWorkspace | null {
  if (!lastKey || queue.some((r) => r.kind === 'needs_input')) return null;
  const s = workspaces.find((x) => x.key === lastKey);
  return s && connected.has(s.serverId) ? s : null;
}

/**
 * Holds the queue still while it could move under a finger (ui-never-moves-under-the-finger): while a touch is down
 * and for `holdMs` after a scroll. Otherwise it follows the data. The first value is taken at once.
 */
export class Settle<T> {
  private shown: T | undefined;
  private pointers = new Set<number>();
  private lastScroll = -Infinity;
  constructor(private holdMs = 2000) {}
  touchDown(id: number) { this.pointers.add(id); }
  touchUp(id: number) { this.pointers.delete(id); }
  scrolled(nowMs: number) { this.lastScroll = nowMs; }
  frozen(nowMs: number): boolean { return this.pointers.size > 0 || nowMs - this.lastScroll < this.holdMs; }
  view(next: T, nowMs: number): T {
    if (this.shown === undefined || !this.frozen(nowMs)) this.shown = next;
    return this.shown;
  }
}
