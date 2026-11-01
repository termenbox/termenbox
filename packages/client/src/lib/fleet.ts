import type { AttentionKind, ConductorCardInfo, ConductorNodeInfo } from './protocol';

/** The merged Needs-you list across nodes (decision 0170): pure, no store and no wire. */
export interface FleetItem {
  card: string;
  node: string;
  nodeName: string;
  silent: boolean;
  workspace: string;
  kind: AttentionKind;
  since: number;
  summary: string;
}

const WORD: Record<AttentionKind, string> = { needs_input: 'waiting', failed: 'failed', finished_unseen: 'finished', unknown: 'check' };
export const kindWord = (k: AttentionKind): string => WORD[k];

/** Oldest first; a card of a silent node goes after the live ones, since the owner cannot answer it now. */
export function fleetItems(cards: ConductorCardInfo[]): FleetItem[] {
  return cards
    .map((c): FleetItem => ({
      card: c.card, node: c.node, nodeName: c.node_name, silent: c.silent_since !== undefined,
      workspace: c.obligation.workspace, kind: c.obligation.kind, since: c.obligation.raised_at,
      summary: c.obligation.prompt ?? c.obligation.task ?? '',
    }))
    .sort((a, b) => Number(a.silent) - Number(b.silent) || a.since - b.since || a.card.localeCompare(b.card));
}

/** Nodes the owner should be told about: enrolled and not connected. */
export const silentNodes = (nodes: ConductorNodeInfo[]): ConductorNodeInfo[] =>
  nodes.filter((n) => n.standing === 'enrolled' && !n.connected);

export function silentText(n: ConductorNodeInfo, now: number): string {
  if (n.silent_since === undefined) return `${n.name} is not connected`;
  const m = Math.max(1, Math.round((now - n.silent_since) / 60_000));
  return `${n.name} has been silent for ${m} min; its ${n.open} open asks wait`;
}

/** What a tap on a fleet card opens: the node's workspace (and tab) on that node, through the conductor (step 2.6). */
export interface NodeTerminalTarget {
  serverId: string;
  node: string;
  nodeName: string;
  workspace: string;
  tab?: string;
  /** Stands in for the workspace id the terminal view sends input and resizes under; never leaves the client. */
  id: string;
}

export function nodeTerminalTarget(cards: ConductorCardInfo[], serverId: string, card: string): NodeTerminalTarget | null {
  const c = cards.find((x) => x.card === card);
  if (!c || c.silent_since !== undefined) return null;
  return { serverId, node: c.node, nodeName: c.node_name, workspace: c.obligation.workspace, tab: c.obligation.tab, id: `node:${c.node}:${c.obligation.workspace}` };
}
