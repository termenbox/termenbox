// Tabs of a workspace (decision 0149): the server sends them in order, the device remembers which one it opened last,
// and the workspace row shows the aggregate of what its agents do. Pure functions, so the rules are testable without a
// browser; text from an agent or the owner is never turned into markup here.
import type { AgentState, TabAgentState, TabInfo } from './protocol';
import type { WorkspaceAgent } from './agents';
import type { HubWorkspace } from './hub';
import type { FleetState } from './ds/states';
import { TEMPLATES } from './newworkspace';

/** Tabs in the server's order; an older server sends none. */
export function tabOrder(tabs: TabInfo[] | undefined): TabInfo[] {
  return [...(tabs ?? [])].sort((a, b) => (a.order - b.order) || a.id.localeCompare(b.id));
}

/** The tab a new device opens: the server's focus when it still exists, else the first. */
export function focusedTab(tabs: TabInfo[] | undefined, focus?: string | null): TabInfo | undefined {
  const ordered = tabOrder(tabs);
  return ordered.find((t) => t.id === focus) ?? ordered[0];
}

/** A tab id that is still in the list, else the focused one. */
export function liveTab(tabs: TabInfo[] | undefined, wanted: string | null | undefined, focus?: string | null): TabInfo | undefined {
  const ordered = tabOrder(tabs);
  return ordered.find((t) => t.id === wanted) ?? focusedTab(ordered, focus);
}

/** The per-tab state key; PR4 fills the map, the aggregate already understands an empty one. */
export const tabStateKey = (workspaceKey: string, tabId: string) => `${workspaceKey}/${tabId}`;

/**
 * The tab that owns a prompt. An answer is sent only when the prompt id still names one tab; a workspace answer is
 * never guessed from a tab that merely happens to be waiting (decision 0149).
 */
export function tabForPrompt(states: TabAgentState[], promptId: string): string | null {
  return states.find((t) => t.prompt?.id === promptId)?.tab ?? null;
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

/** The word for a tab's own state (PR4 fills these; the fallback is the workspace's). */
export function tabStateWord(state: AgentState | undefined): string {
  return { needs_input: 'Needs you', working: 'Working', error: 'Error', done: 'Done', idle: 'Idle', unknown: 'No status' }[state ?? 'unknown'];
}

/** The template label, with `shell` as Shell. */
export function tabKindLabel(kind: string): string {
  return TEMPLATES.find((t) => t.id === kind)?.label ?? (kind || 'Shell');
}

export const tabIconCommand = (tab: TabInfo) => (tab.kind === 'shell' ? 'bash' : tab.kind);
export const tabIconAgent = (tab: TabInfo) => (tab.kind === 'shell' ? undefined : tab.kind);

/** A label no tab of this workspace uses yet: `Claude Code`, `Claude Code 2`, ... */
export function uniqueTabLabel(base: string, tabs: TabInfo[] | undefined): string {
  const taken = new Set((tabs ?? []).map((t) => t.label));
  if (!taken.has(base)) return base;
  for (let n = 2; n < 1000; n++) {
    const candidate = `${base} ${n}`;
    if (!taken.has(candidate)) return candidate;
  }
  return base;
}

/** `tabs` with `id` at `index`; the same array when nothing moves. */
export function moveTab(tabs: TabInfo[] | undefined, id: string, index: number): TabInfo[] {
  const ordered = tabOrder(tabs);
  const from = ordered.findIndex((t) => t.id === id);
  if (from < 0) return ordered;
  const rest = ordered.filter((t) => t.id !== id);
  const at = Math.max(0, Math.min(index, rest.length));
  if (at === from) return ordered;
  return [...rest.slice(0, at), ordered[from], ...rest.slice(at)];
}

/** What is wrong with a tab's relative directory, or an empty string. Mirrors the server's check; the server decides. */
export function relativeDirProblem(dir: string): string {
  if (!dir) return '';
  if (dir.startsWith('/')) return 'Use a directory inside the workspace';
  if (dir.split('/').some((part) => part === '..')) return 'The directory must stay inside the workspace';
  if (/[\u0000-\u001f\u007f]/.test(dir)) return 'That directory has an unsupported character';
  if (dir.length > 512) return 'That directory is too long';
  return '';
}

/** A tab's absolute directory for display; empty when the workspace has no directory. */
export function tabPath(root: string | undefined, tab: Pick<TabInfo, 'dir'>): string {
  if (!root) return tab.dir;
  if (!tab.dir) return root;
  return `${root.replace(/\/+$/, '')}/${tab.dir.replace(/^\/+/, '')}`;
}

// The device's own last tab per workspace; the server's focus is only the fallback for a new device.
const ACTIVE_KEY = 'termenbox_active_tab';
type Reader = Pick<Storage, 'getItem'>;
type Writer = Pick<Storage, 'setItem'>;
const storage = (): Storage | undefined => { try { return typeof localStorage === 'undefined' ? undefined : localStorage; } catch { return undefined; } };

export function loadActiveTab(workspaceKey: string, store: Reader | undefined = storage()): string | null {
  try {
    const all = JSON.parse(store?.getItem(ACTIVE_KEY) ?? '{}');
    const id = all && typeof all === 'object' ? all[workspaceKey] : undefined;
    return typeof id === 'string' ? id : null;
  } catch { return null; }
}

export function saveActiveTab(workspaceKey: string, tabId: string, store: (Reader & Writer) | undefined = storage()) {
  try {
    let all: Record<string, unknown> = {};
    try { const v = JSON.parse(store?.getItem?.(ACTIVE_KEY) ?? '{}'); if (v && typeof v === 'object' && !Array.isArray(v)) all = v; } catch { /* Start again. */ }
    store?.setItem(ACTIVE_KEY, JSON.stringify({ ...all, [workspaceKey]: tabId }));
  } catch { /* Private mode: the server's focus is still a sensible default. */ }
}
