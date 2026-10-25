import { chipFor } from './cards';
import type { AgentMap, WorkspaceAgent } from './agents';
import type { HubWorkspace } from './hub';

/** With server status, an open Attention item; otherwise (an older server) the raw state. */
export const needsYou = (agent?: WorkspaceAgent) => agent?.serverStatus ? !!agent.attention : agent?.state === 'needs_input' || agent?.state === 'error';
export const needsYouCount = (agents: AgentMap) => Object.values(agents).filter(needsYou).length;
/** Only needs-you moves to the front; otherwise tabs mirror the servers' workspace order. */
export function tabIds(workspaces: HubWorkspace[], agents: AgentMap): string[] {
  return [...workspaces.filter(s => needsYou(agents[s.key])), ...workspaces.filter(s => !needsYou(agents[s.key]))].map(s => s.key);
}
export function tabLabel(workspace: HubWorkspace, agent: WorkspaceAgent | undefined, serverCount: number, now: number): string {
  return [workspace.name, ...(serverCount > 1 ? [workspace.serverName] : []), agent ? chipFor(agent, now).label : 'No status'].join(' · ');
}

const HIDDEN_KEY = 'termenbox_hidden_tabs';
const store = () => typeof localStorage === 'undefined' ? undefined : localStorage;
/** Workspaces the user detached on this device: their tab is closed while the workspace keeps running. */
export function loadHidden(storage: Pick<Storage, 'getItem'> | undefined = store()): Set<string> {
  try {
    const v = JSON.parse(storage?.getItem(HIDDEN_KEY) ?? '[]');
    return new Set(Array.isArray(v) ? v.filter((x): x is string => typeof x === 'string') : []);
  } catch { return new Set(); }
}
export function saveHidden(hidden: Set<string>, storage: Pick<Storage, 'setItem'> | undefined = store()) {
  try { storage?.setItem(HIDDEN_KEY, JSON.stringify([...hidden])); } catch { /* Private mode: kept in memory. */ }
}
/**
 * The workspaces that have a tab: all but the detached ones. A detached workspace whose agent needs you shows again
 * for as long as it does, so a closed tab never hides a question. Opening a workspace from the sheet un-hides it.
 */
export function openTabs<T extends { key: string }>(workspaces: T[], agents: AgentMap, hidden: Set<string>): T[] {
  return workspaces.filter(s => !hidden.has(s.key) || needsYou(agents[s.key]));
}
