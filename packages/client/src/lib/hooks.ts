// The hook state of each agent on each server (U10, decisions 0079 and 0106), as the server reports it, and which card or
// row that gives Home and Settings. Claude Code has hooks and OpenCode has plugins (fact 0038); every other agent is
// simply never offered. Text from the server (a preview, an error) is shown as text only.
import type { AgentMap, WorkspaceAgent } from './agents';
import type { HubServer, HubWorkspace } from './hub';
import { workspaceKey } from './hub';
import type { HookOffers } from './hookOffer';
import type { ClientMessage, ServerMessage } from './protocol';

export const HOOK_AGENTS = ['claude', 'opencode'] as const;
export const HOOK_LABEL: Record<string, string> = { claude: 'Claude Code', opencode: 'OpenCode' };
export type HookState = 'connected' | 'not_connected' | 'unsupported' | 'unavailable';
export interface HookInfo { state: HookState; /** The whole file (settings or plugin) as Enable will write it. */ preview?: string; error?: string; busy?: 'apply' | 'remove' }
export type HookMap = Record<string, Record<string, HookInfo>>;
type HookReply = Extract<ServerMessage, { type: 'agent_hooks' }>;

const STATES: readonly string[] = ['connected', 'not_connected', 'unsupported', 'unavailable'];

/** What to ask a server when it connects: the state and the exact change in one answer. */
export const hookQueries = (): ClientMessage[] => HOOK_AGENTS.map((agent) => ({ type: 'agent_hooks', agent, action: 'preview' }));

/** An unknown state is read as `unavailable`. A failed change keeps the state the server reports and shows its reason. */
export function applyHookReply(map: HookMap, serverId: string, msg: HookReply): HookMap {
  const state = (STATES.includes(msg.state) ? msg.state : 'unavailable') as HookState;
  const prev = map[serverId]?.[msg.agent];
  const info: HookInfo = { state, error: msg.error, preview: msg.preview ?? (state === 'connected' ? undefined : prev?.preview) };
  return { ...map, [serverId]: { ...map[serverId], [msg.agent]: info } };
}

export function markBusy(map: HookMap, serverId: string, agent: string, busy: HookInfo['busy']): HookMap {
  const cur = map[serverId]?.[agent];
  if (!cur) return map;
  return { ...map, [serverId]: { ...map[serverId], [agent]: { ...cur, busy, error: undefined } } };
}

export interface HookOffer { serverId: string; serverName: string; agent: string; label: string; info: HookInfo }

/** The one card Home shows: a connected server that has an agent with a hook surface running, hooks not yet on, and an
 *  offer that is due. The scan names the agent, so no hook is needed to detect it. */
export function pendingOffer(servers: HubServer[], workspaces: HubWorkspace[], agents: AgentMap, hooks: HookMap, offers: HookOffers): HookOffer | undefined {
  for (const s of servers) {
    if (s.state !== 'connected') continue;
    for (const agent of HOOK_AGENTS) {
      const info = hooks[s.id]?.[agent];
      if (!info || info.state !== 'not_connected') continue;
      const seen = workspaces.some((x) => x.serverId === s.id && agents[workspaceKey(s.id, x.id)]?.agent === agent);
      if (!seen || !offers.due(s.id, agent)) continue;
      return { serverId: s.id, serverName: s.server.server_name || 'Server', agent, label: HOOK_LABEL[agent] ?? agent, info };
    }
  }
  return undefined;
}

export interface HookRow extends HookOffer { never: boolean }

/** Settings: every agent with a hook surface on every connected server, whether or not it is running now. */
export function hookRows(servers: HubServer[], hooks: HookMap, offers: HookOffers): HookRow[] {
  const rows: HookRow[] = [];
  for (const s of servers) {
    if (s.state !== 'connected') continue;
    for (const agent of HOOK_AGENTS) {
      const info = hooks[s.id]?.[agent];
      if (!info || info.state === 'unsupported' || info.state === 'unavailable') continue;
      rows.push({ serverId: s.id, serverName: s.server.server_name || 'Server', agent, label: HOOK_LABEL[agent] ?? agent, info, never: offers.isNever(s.id, agent) });
    }
  }
  return rows;
}

/** The agent is waiting on a prompt, with or without hooks (the screen scan alone also reads it as blocked): keys typed in
 *  the terminal now are the user answering by hand, which is what brings a "Not now" back (decision 0106). */
export const waitingOnPrompt = (a: WorkspaceAgent | undefined): boolean => a?.state === 'needs_input';
