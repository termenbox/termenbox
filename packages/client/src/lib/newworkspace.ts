// Starting a workspace (plan H.10): which agent, in which directory, under what name. The templates are the agents the
// server recognises; the server refuses any other name, so this list is only what the sheet offers.
import type { WindowMap } from './windows';

export const TEMPLATES: { id: string; label: string }[] = [
  { id: 'shell', label: 'Shell' },
  { id: 'claude', label: 'Claude Code' },
  { id: 'codex', label: 'Codex' },
  { id: 'copilot', label: 'Copilot' },
  { id: 'opencode', label: 'OpenCode' },
  { id: 'gemini', label: 'Gemini' },
  { id: 'kilo', label: 'Kilo' },
  { id: 'aider', label: 'Aider' },
  { id: 'goose', label: 'Goose' },
  { id: 'amp', label: 'Amp' },
  { id: 'qwen', label: 'Qwen' },
  { id: 'cursor-agent', label: 'Cursor' },
];

/** Template lookup by id. */
export const TEMPLATE_BY_ID: Record<string, { id: string; label: string }> = Object.fromEntries(TEMPLATES.map(t => [t.id, t]));

/** Filter templates to only those the server supports.
 * If tmux is available, all templates are supported (tmux can run any agent).
 * If only acp is available, only acpAgents + shell are supported. */
export function getSupportedTemplates(server: { backends: string[]; acpAgents: string[] } | undefined): { id: string; label: string }[] {
  if (!server) return TEMPLATES;
  const hasTmux = server.backends.includes('tmux');
  if (hasTmux) return TEMPLATES; // tmux can run any agent
  // Only acp (no tmux): filter to acpAgents + shell
  const supported = new Set([...server.acpAgents, 'shell']);
  return TEMPLATES.filter(t => supported.has(t.id));
}

const RECENT_AGENTS_KEY = 'termenbox_recent_agents';
const MAX_RECENT_AGENTS = 5;

/** Get recently used agents for a server (most recent first). */
export function getRecentAgents(serverId: string): string[] {
  try {
    const stored = localStorage.getItem(RECENT_AGENTS_KEY);
    if (!stored) return [];
    const all = JSON.parse(stored) as Record<string, string[]>;
    return all[serverId] ?? [];
  } catch {
    return [];
  }
}

/** Save an agent as recently used for a server (LRU, max 5). */
export function saveRecentAgent(serverId: string, templateId: string): void {
  try {
    const stored = localStorage.getItem(RECENT_AGENTS_KEY);
    const all: Record<string, string[]> = stored ? JSON.parse(stored) : {};
    const list = all[serverId] ?? [];
    // Remove if exists, then prepend
    const filtered = list.filter(id => id !== templateId);
    filtered.unshift(templateId);
    all[serverId] = filtered.slice(0, MAX_RECENT_AGENTS);
    localStorage.setItem(RECENT_AGENTS_KEY, JSON.stringify(all));
  } catch {
    // Private mode or quota exceeded - ignore
  }
}

/** Order templates: recent first, then remaining supported. */
export function orderTemplatesByRecent(supported: { id: string; label: string }[], recent: string[]): { id: string; label: string }[] {
  const recentTemplates = recent.map(id => TEMPLATE_BY_ID[id]).filter(Boolean) as { id: string; label: string }[];
  const recentIds = new Set(recentTemplates.map(t => t.id));
  const remaining = supported.filter(t => !recentIds.has(t.id));
  return [...recentTemplates, ...remaining];
}

/** What a cold start offers before the device has any history (decision 0141). */
export const COLD_START_AGENTS = ['claude', 'codex', 'shell'];

/** The tiles shown in the dialog: recent agents first, then the cold-start list, then the rest, all supported ones only. */
export function inlineAgents(supported: { id: string; label: string }[], recent: string[], count = 3): { id: string; label: string }[] {
  const ok = new Map(supported.map(t => [t.id, t]));
  const ids = [...new Set([...recent, ...COLD_START_AGENTS, ...supported.map(t => t.id)])].filter(id => ok.has(id));
  return ids.slice(0, count).map(id => ok.get(id)!);
}

/** The inline tiles with `selected` always among them: a pick from the full list replaces the last tile so it stays visible. */
export function visibleAgents(supported: { id: string; label: string }[], recent: string[], selected: string, count = 3): { id: string; label: string }[] {
  const tiles = inlineAgents(supported, recent, count);
  if (!selected || tiles.some(t => t.id === selected)) return tiles;
  const pick = supported.find(t => t.id === selected);
  return pick ? [...tiles.slice(0, Math.max(0, count - 1)), pick] : tiles;
}

/** The agent preselected when the dialog opens: the last one used here, else Claude Code, else the first one offered. */
export function defaultAgent(supported: { id: string }[], recent: string[]): string {
  const ids = supported.map(t => t.id);
  return recent.find(id => ids.includes(id)) ?? (ids.includes('claude') ? 'claude' : ids[0] ?? 'claude');
}

const LAST_SERVER_KEY = 'termenbox_new_workspace_server';
let lastServerMemory = '';
/** The server a workspace was last created on from this device. */
export function getLastServer(storage: Pick<Storage, 'getItem'> | undefined = viewStore()): string {
  try { return storage?.getItem(LAST_SERVER_KEY) || lastServerMemory; } catch { return lastServerMemory; }
}
export function saveLastServer(serverId: string, storage: Pick<Storage, 'setItem'> | undefined = viewStore()): void {
  lastServerMemory = serverId;
  try { storage?.setItem(LAST_SERVER_KEY, serverId); } catch { /* Private mode: kept in memory. */ }
}

const HOMES_KEY = 'termenbox_server_homes';
const homeMemory = new Map<string, string>();
/** A server user's home folder as its last listing said, so `~/…` shows before the picker is opened again. */
export function getHome(serverId: string, storage: Pick<Storage, 'getItem'> | undefined = viewStore()): string {
  try { return (JSON.parse(storage?.getItem(HOMES_KEY) || '{}') as Record<string, string>)[serverId] || homeMemory.get(serverId) || ''; } catch { return homeMemory.get(serverId) ?? ''; }
}
export function saveHome(serverId: string, home: string, storage: Pick<Storage, 'getItem' | 'setItem'> | undefined = viewStore()): void {
  homeMemory.set(serverId, home);
  try {
    const all = JSON.parse(storage?.getItem(HOMES_KEY) || '{}') as Record<string, string>;
    storage?.setItem(HOMES_KEY, JSON.stringify({ ...all, [serverId]: home }));
  } catch { /* Private mode: kept in memory. */ }
}

const RECENT_DIRS_KEY = 'termenbox_recent_dirs';
const MAX_RECENT_DIRS = 10;
// Where storage is missing or throws, the list lives here for the rest of the page's life.
const dirMemory = new Map<string, string[]>();

/** Folders used for new workspaces on this server from this device, most recent first (up to 10). */
export function getRecentDirs(serverId: string, storage: Pick<Storage, 'getItem'> | undefined = viewStore()): string[] {
  if (!storage) return dirMemory.get(serverId) ?? [];
  try {
    const all = JSON.parse(storage.getItem(RECENT_DIRS_KEY) ?? '{}');
    const list = all && typeof all === 'object' ? all[serverId] : undefined;
    return Array.isArray(list) ? list.filter((d): d is string => typeof d === 'string').slice(0, MAX_RECENT_DIRS) : [];
  } catch { return dirMemory.get(serverId) ?? []; } // Unreadable: what this page remembers.
}

/** Remembers a folder a workspace was created in (MRU, max 10). Call it on create, not on click. */
export function saveRecentDir(serverId: string, path: string, storage: Pick<Storage, 'getItem' | 'setItem'> | undefined = viewStore()): void {
  if (!path.startsWith('/')) return;
  const next = [path, ...getRecentDirs(serverId, storage).filter(d => d !== path)].slice(0, MAX_RECENT_DIRS);
  dirMemory.set(serverId, next);
  try {
    let all: Record<string, unknown> = {};
    try { const v = JSON.parse(storage?.getItem(RECENT_DIRS_KEY) ?? '{}'); if (v && typeof v === 'object' && !Array.isArray(v)) all = v; } catch { /* Start again. */ }
    storage?.setItem(RECENT_DIRS_KEY, JSON.stringify({ ...all, [serverId]: next }));
  } catch { /* Private mode: kept in memory. */ }
}

/** The recent folders to offer: the stored ones, or, on a device with none yet, the folders open workspaces use. */
export function mergeRecentDirs(stored: string[], windows: WindowMap, serverPrefix: string, limit = MAX_RECENT_DIRS): string[] {
  return (stored.length ? stored : recentDirs(windows, serverPrefix, limit)).slice(0, limit);
}

/** Directories already in use on the server, most used first: the pick list for "where". */
export function recentDirs(windows: WindowMap, serverPrefix: string, limit = 6): string[] {
  const count = new Map<string, number>();
  for (const [key, w] of Object.entries(windows)) {
    if (!key.startsWith(serverPrefix)) continue;
    for (const x of w.windows) if (x.path.startsWith('/')) count.set(x.path, (count.get(x.path) ?? 0) + 1);
  }
  return [...count.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, limit).map(([p]) => p);
}

/** A name the server accepts (letters, digits, - and _; at most 64) that is not taken: `termenbox-claude`, `termenbox-claude-2`. */
export function suggestName(dir: string, template: string, taken: string[]): string {
  const tail = dir.split('/').filter(Boolean).pop() ?? '';
  const base = (tail || 'workspace').replace(/[^A-Za-z0-9_-]+/g, '-').replace(/^-+|-+$/g, '') || 'workspace';
  const head = (template && template !== 'shell' ? `${base}-${template}` : base).slice(0, 58);
  let name = head, n = 2;
  while (taken.includes(name)) name = `${head}-${n++}`;
  return name;
}

export const validDir = (dir: string) => dir === '' || (dir.startsWith('/') && dir.length <= 512 && !/[\u0000-\u001f\u007f]/.test(dir));
export const validName = (name: string) => /^[A-Za-z0-9_-]{1,64}$/.test(name);

// ---------------------------------------------------------------------------------------------------------------------
// The view: how an agent runs. "Terminal" is the agent's own TUI in a tmux workspace; "Agent view" is an ACP workspace the
// server starts and termenbox draws. (Not the agent's own workspace "mode", which AcpView has a select for.)
// ---------------------------------------------------------------------------------------------------------------------
export type View = 'terminal' | 'acp';
type Caps = { backends: string[]; acpAgents: string[] };

/** The server offers both views (tmux and acp), so the choice is a View rather than a Manager. */
export const hasViews = (s: Caps | undefined) => !!s && s.backends.includes('tmux') && s.backends.includes('acp');

/**
 * What the View control does for this template: `choose` shows it, otherwise the view is fixed. A template with no ACP
 * agent is always the terminal; an ACP agent tmux cannot start (one the owner added) is always the agent view.
 */
export function viewControl(s: Caps | undefined, template: string): 'choose' | View {
  if (!s || !hasViews(s) || !s.acpAgents.includes(template)) return 'terminal';
  return TEMPLATES.some(t => t.id === template) ? 'choose' : 'acp';
}

/** The view to use: the wanted one where there is a choice, else the fixed one. A remembered `acp` for a gone agent is `terminal`. */
export function resolveView(s: Caps | undefined, template: string, wanted: View): View {
  const c = viewControl(s, template);
  return c === 'choose' ? wanted : c;
}

/** Managers for the Manager dropdown: tmux and acp are one entry (`tmux`, steered by the View) when both exist. */
export const managerOptions = (s: Caps | undefined): string[] =>
  !s ? [] : hasViews(s) ? ['tmux', ...s.backends.filter(b => b !== 'tmux' && b !== 'acp')] : s.backends;

/** The backend a create request names: the View decides between tmux and acp, any other manager is itself. */
export function backendFor(s: Caps | undefined, manager: string, template: string, wanted: View): string {
  return hasViews(s) && manager === 'tmux' ? (resolveView(s, template, wanted) === 'acp' ? 'acp' : 'tmux') : manager;
}

const VIEW_KEY = 'termenbox_new_session_view';
/** The browser's storage, or none (a server-side render, or storage turned off). */
export const viewStore = () => { try { return typeof localStorage === 'undefined' ? undefined : localStorage; } catch { return undefined; } };
// Where storage is missing or throws, the choice lives here for the rest of the page's life.
const memory = new WeakMap<object, Map<string, View>>();
const kept = (storage: object | undefined) => {
  const key = storage ?? memory;
  if (!memory.has(key)) memory.set(key, new Map());
  return memory.get(key)!;
};
const viewId = (serverId: string, template: string) => `${serverId}/${template}`;

/** The last view chosen for this agent on this server; the terminal when none. */
export function loadView(storage: Pick<Storage, 'getItem'> | undefined, serverId: string, template: string): View {
  try {
    const v = JSON.parse(storage?.getItem(VIEW_KEY) ?? '{}');
    const x = v && typeof v === 'object' ? v[viewId(serverId, template)] : undefined;
    if (x === 'acp' || x === 'terminal') return x;
  } catch { /* Unreadable: fall back to what this page remembers. */ }
  return kept(storage).get(viewId(serverId, template)) ?? 'terminal';
}

export function saveView(storage: Pick<Storage, 'getItem' | 'setItem'> | undefined, serverId: string, template: string, view: View) {
  kept(storage).set(viewId(serverId, template), view);
  try {
    let all: Record<string, unknown> = {};
    try { const v = JSON.parse(storage?.getItem(VIEW_KEY) ?? '{}'); if (v && typeof v === 'object' && !Array.isArray(v)) all = v; } catch { /* Start again. */ }
    storage?.setItem(VIEW_KEY, JSON.stringify({ ...all, [viewId(serverId, template)]: view }));
  } catch { /* Private mode: kept in memory. */ }
}
