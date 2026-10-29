// What the sidebar and the palette list: workspaces grouped by server with a fleet state each, and a view of that list
// that holds still while a finger or pointer is down (guardrail ui-never-moves-under-the-finger).
import { FrozenOrder } from './cards';
import { needsYou } from './tabs';
import type { AgentMap, WorkspaceAgent } from './agents';
import type { HubServer, HubWorkspace } from './hub';
import type { FleetState } from './ds/states';
import { aggregateStatus } from './workspaceTabs';

export function fleetOf(agent?: WorkspaceAgent): FleetState {
  if (!agent) return 'idle';
  if (needsYou(agent)) return 'needs-you';
  if (agent.state === 'error') return 'failed';
  if (agent.state === 'working') return 'working';
  if (agent.state === 'done') return 'finished';
  return 'idle';
}

export interface NavWorkspace { key: string; name: string; serverId: string; serverName: string; state: FleetState; agent?: string }
export interface NavGroup { server: HubServer; workspaces: NavWorkspace[] }

/** Servers in the hub's order; inside a server, the user's own order of workspaces (a state never moves one). */
export function navGroups(servers: HubServer[], workspaces: HubWorkspace[], agents: AgentMap): NavGroup[] {
  return servers.map((server) => ({
    server,
    workspaces: workspaces
      .filter((s) => s.serverId === server.id)
      .map((s): NavWorkspace => ({ key: s.key, name: s.name, serverId: s.serverId, serverName: s.serverName, state: aggregateStatus(s.key, s, agents[s.key]), agent: agents[s.key]?.agent })),
  }));
}

/**
 * The list as shown. While a pointer is down or the list was scrolled in the last 2 s, order and membership stay as they
 * were (nothing is inserted, removed or moved); a workspace that is still there gets its fresh state in place.
 */
export class HeldView {
  private hold = new FrozenOrder();
  private shown: NavGroup[] = [];
  down(pointerId: number) { this.hold.touchDown(pointerId); }
  up(pointerId: number) { this.hold.touchUp(pointerId); }
  scrolled(nowMs: number) { this.hold.scrolled(nowMs); }
  frozen(nowMs: number) { return this.hold.frozen(nowMs); }
  view(groups: NavGroup[], nowMs: number): NavGroup[] {
    if (!this.hold.frozen(nowMs) || this.shown.length === 0) return (this.shown = groups);
    const fresh = new Map(groups.flatMap((g) => g.workspaces.map((s) => [s.key, s] as const)));
    const servers = new Map(groups.map((g) => [g.server.id, g.server] as const));
    return (this.shown = this.shown.map((g) => ({ server: servers.get(g.server.id) ?? g.server, workspaces: g.workspaces.map((s) => fresh.get(s.key) ?? s) })));
  }
}
