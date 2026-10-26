import { chipFor } from './cards';
import { needsYou } from './tabs';
import type { AgentMap, WorkspaceAgent } from './agents';
import type { AgentState } from './protocol';
import type { HubWorkspace, HubServer } from './hub';
import { shortPath } from './windows';

export type Preset = 'needs' | 'working' | 'cache' | 'recent' | 'server';
export type GroupBy = 'server' | 'agent' | 'status' | 'project' | 'none';
export type SortBy = 'manual' | 'response' | 'cache' | 'name';
export interface SessionView {
  preset?: Preset;
  group: GroupBy;
  sort: SortBy;
  filter: { statuses: AgentState[]; agents: string[]; servers: string[]; query: string; cacheExpiring?: boolean };
}
export interface WorkspaceGroup { key: string; label: string; serverId?: string; workspaces: HubWorkspace[] }
export interface ViewGroup extends WorkspaceGroup { server?: HubServer }
export function withServerStatus(groups: WorkspaceGroup[], servers: HubServer[]): ViewGroup[] {
  return groups.map(group => ({ ...group, server: servers.find(s => s.id === group.serverId) }));
}
export const PRESETS: { id: Preset; label: string }[] = [
  { id: 'needs', label: 'Needs me' }, { id: 'working', label: 'Working' }, { id: 'cache', label: 'Cache expiring' },
  { id: 'recent', label: 'Recent' }, { id: 'server', label: 'By server' },
];
const GROUPS: GroupBy[] = ['server', 'agent', 'status', 'project', 'none'];
const SORTS: SortBy[] = ['manual', 'response', 'cache', 'name'];
const STATES: AgentState[] = ['idle', 'working', 'needs_input', 'done', 'error', 'unknown'];
const KEY = 'termenbox_session_view';

/** A fresh object: editing free controls never mutates a shared preset. */
export function presetView(preset: Preset): SessionView {
  const filter: SessionView['filter'] = { statuses: [], agents: [], servers: [], query: '' };
  let group: GroupBy = 'none', sort: SortBy = 'manual';
  if (preset === 'needs') filter.statuses = ['needs_input', 'error'];
  if (preset === 'working') { filter.statuses = ['working']; group = 'server'; sort = 'name'; }
  if (preset === 'cache') { filter.statuses = ['idle', 'done']; filter.cacheExpiring = true; sort = 'cache'; }
  if (preset === 'recent') sort = 'response';
  if (preset === 'server') group = 'server';
  return { preset, group, sort, filter };
}
export function adaptiveDefault(agents: AgentMap): SessionView {
  return presetView(Object.values(agents).some(needsYou) ? 'needs' : 'server');
}

interface CacheOptions {
  ttlMs?: number;
  /** Working directory per workspace key (the tab's window); the Project grouping reads it. */
  paths?: Record<string, string>;
  /** G.8 supplies exact expiry here; until then use the last-response estimate. Null means unknown. */
  remaining?: (agent: WorkspaceAgent | undefined, key: string, now: number) => number | null;
}
/** Pure grouping, sorting and filtering. Global keys are retained all the way to the card's action. */
/** Subtraction gives NaN for two equal infinities, which breaks sorting. */
const cmp = (x: number, y: number) => (x === y ? 0 : x < y ? -1 : 1);

export function applyView(workspaces: HubWorkspace[], agents: AgentMap, view: SessionView, now: number, cache: CacheOptions = {}): WorkspaceGroup[] {
  const agentName = (s: HubWorkspace) => agents[s.key]?.agent ?? s.agent ?? 'shell';
  const state = (s: HubWorkspace) => agents[s.key]?.state ?? 'unknown';
  const remaining = (s: HubWorkspace) => {
    const a = agents[s.key];
    if (cache.remaining) return cache.remaining(a, s.key, now);
    return a?.lastResponseAt === undefined ? null : Math.max(0, (cache.ttlMs ?? 300000) - Math.max(0, now - a.lastResponseAt));
  };
  const query = view.filter.query.trim().toLocaleLowerCase();
  const selected = workspaces.filter(s => {
    const f = view.filter;
    if (f.statuses.length && !f.statuses.includes(state(s))) return false;
    if (f.agents.length && !f.agents.includes(agentName(s))) return false;
    if (f.servers.length && !f.servers.includes(s.serverId)) return false;
    if (query && !`${s.name} ${s.serverName} ${agentName(s)}`.toLocaleLowerCase().includes(query)) return false;
    if (f.cacheExpiring && (!['idle', 'done'].includes(state(s)) || agentName(s) === 'shell' || (remaining(s) ?? 0) <= 0)) return false;
    return true;
  });
  const byName = (a: HubWorkspace, b: HubWorkspace) => a.name.localeCompare(b.name) || a.key.localeCompare(b.key);
  // "My order" is the order given (the user's own, lib/order.ts); the others tie-break by name.
  const given = new Map(selected.map((s, i) => [s.key, i]));
  selected.sort((a, b) => {
    const aa = agents[a.key], bb = agents[b.key];
    let order = 0;
    if (view.sort === 'manual') return given.get(a.key)! - given.get(b.key)!;
    if (view.sort === 'response') order = cmp(bb?.lastResponseAt ?? -Infinity, aa?.lastResponseAt ?? -Infinity);
    if (view.sort === 'cache') order = cmp(remaining(a) ?? Infinity, remaining(b) ?? Infinity);
    return order || byName(a, b);
  });
  const groups = new Map<string, WorkspaceGroup>();
  for (const s of selected) {
    const value = view.group === 'server' ? s.serverId : view.group === 'agent' ? agentName(s) : view.group === 'status' ? state(s)
      : view.group === 'project' ? shortPath(cache.paths?.[s.key] ?? '') || 'No directory' : '';
    const key = `${view.group}:${value}`;
    if (!groups.has(key)) groups.set(key, { key,
      label: view.group === 'server' ? s.serverName : view.group === 'project' ? value : view.group === 'status' ? chipFor({ state: state(s) } as WorkspaceAgent, now).label : value,
      ...(view.group === 'server' ? { serverId: s.serverId } : {}), workspaces: [] });
    groups.get(key)!.workspaces.push(s);
  }
  const result = [...groups.values()];
  // Agent/status categories stay easy to find; server groups follow the view's workspace priority.
  return view.group === 'agent' || view.group === 'status' || view.group === 'project' ? result.sort((a, b) => a.label.localeCompare(b.label)) : result;
}

const deviceStorage = () => typeof localStorage === 'undefined' ? undefined : localStorage;
export function saveView(view: SessionView, storage: Pick<Storage, 'setItem'> | undefined = deviceStorage()) {
  try { storage?.setItem(KEY, JSON.stringify(view)); } catch { /* Keep the in-memory choice in private mode. */ }
}
export function loadView(storage: Pick<Storage, 'getItem'> | undefined = deviceStorage()): SessionView | null {
  try {
    const v = JSON.parse(storage?.getItem(KEY) ?? 'null');
    if (v?.sort === 'human') v.sort = 'manual'; // "Needs you first" is gone: a saved view keeps everything else
    if (!v || !GROUPS.includes(v.group) || !SORTS.includes(v.sort) || !v.filter) return null;
    const f = v.filter;
    if (!Array.isArray(f.statuses) || !f.statuses.every((s: AgentState) => STATES.includes(s)) ||
        !Array.isArray(f.agents) || !f.agents.every((s: unknown) => typeof s === 'string') ||
        !Array.isArray(f.servers) || !f.servers.every((s: unknown) => typeof s === 'string') || typeof f.query !== 'string') return null;
    if (v.preset !== undefined && !PRESETS.some(p => p.id === v.preset)) return null;
    return { ...(v.preset ? { preset: v.preset } : {}), group: v.group, sort: v.sort,
      filter: { statuses: f.statuses, agents: f.agents, servers: f.servers, query: f.query, ...(f.cacheExpiring === true ? { cacheExpiring: true } : {}) } };
  } catch { return null; }
}
