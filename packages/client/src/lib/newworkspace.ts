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
/** The server's label rule: one line, at most 64 characters, no `|`; a minted id keeps the label free of id syntax. */
export const validLabel = (name: string) => { const t = name.trim(); return t.length > 0 && t.length <= 64 && !/[\u0000-\u001f\u007f|]/.test(t); };

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
