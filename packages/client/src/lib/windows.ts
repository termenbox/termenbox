// Windows of a multiplexer workspace (plan H.3/H.6): the tab speaks for one of them, the dropdown lists all.
// Names and paths come from the machine's shell and are untrusted: they are only ever rendered as text.
import type { ServerMessage, WindowInfo } from './protocol';

export interface WorkspaceWindows {
  windows: WindowInfo[];
  /** Index of the window the tab speaks for (the agent window used last, else the active one). */
  representative: number | null;
}
export type WindowMap = Record<string, WorkspaceWindows>;

export function applyWindowList(map: WindowMap, msg: Extract<ServerMessage, { type: 'window_list' }>): WindowMap {
  return { ...map, [msg.session]: { windows: msg.windows, representative: msg.representative ?? null } };
}

/** Keeps only workspaces that still exist. */
export function forgetWindows(map: WindowMap, live: string[]): WindowMap {
  const keep = new Set(live);
  return Object.fromEntries(Object.entries(map).filter(([k]) => keep.has(k)));
}

export function representativeWindow(w: WorkspaceWindows | undefined): WindowInfo | undefined {
  if (!w) return undefined;
  return w.windows.find(x => x.index === w.representative) ?? w.windows.find(x => x.active) ?? w.windows[0];
}

/** The window to switch to before opening a workspace that needs you: the one its tab speaks for, when it is not already
 *  the current one. Null leaves tmux's current window alone (no window list, or it is already there). */
export function windowToOpen(w: WorkspaceWindows | undefined): number | null {
  const rep = representativeWindow(w);
  return rep && !rep.active ? rep.index : null;
}

/** `/home/u/projects/app` → `~/projects/app`; a deeper path keeps its last two segments (`…/b/c`). */
export function shortPath(path: string, maxSegments = 2): string {
  if (!path) return '';
  let parts = path.split('/').filter(Boolean);
  let home = false;
  // The client does not know the home directory; /home/<user> and /root are the usual ones.
  if (parts[0] === 'home' && parts.length >= 2) { home = true; parts = parts.slice(2); }
  else if (parts[0] === 'root') { home = true; parts = parts.slice(1); }
  if (!parts.length) return home ? '~' : '/';
  return parts.length > maxSegments ? `…/${parts.slice(-maxSegments).join('/')}` : `${home ? '~' : ''}/${parts.join('/')}`;
}

export interface TabText { label: string; hint: string }

/**
 * What each tab reads. A tab is the path of its representative window; when two tabs would read the same, the
 * workspace name is added as a second line so three agents in one repository stay apart. Without a path (an old
 * server, no control mode) the label is the workspace name.
 */
export function tabTexts(workspaces: { key: string; name: string }[], windows: WindowMap): Record<string, TabText> {
  const base = (s: { key: string; name: string }) => shortPath(representativeWindow(windows[s.key])?.path ?? '');
  const counts = new Map<string, number>();
  for (const s of workspaces) { const b = base(s); if (b) counts.set(b, (counts.get(b) ?? 0) + 1); }
  const out: Record<string, TabText> = {};
  for (const s of workspaces) {
    const b = base(s);
    const full = representativeWindow(windows[s.key])?.path ?? '';
    out[s.key] = !b ? { label: s.name, hint: s.name }
      : { label: (counts.get(b) ?? 0) > 1 ? `${b} · ${s.name}` : b, hint: full };
  }
  return out;
}
