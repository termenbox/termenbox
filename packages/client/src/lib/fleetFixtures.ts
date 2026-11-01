import type { ConductorCardInfo, ConductorNodeInfo, AttentionKind } from './protocol';

/** A fleet for the Needs-you templates (decision 0170): 3 nodes, 40 obligations, one node silent. Fixed times, so tests do not drift. */
export const FLEET_NOW = 1_800_000_000_000;

export const fleetNodes: ConductorNodeInfo[] = [
  { node: 'n-laptop', name: 'laptop', standing: 'enrolled', connected: true, open: 14, agents: 20, working: 12, waiting: 6, dropped: 0 },
  { node: 'n-build', name: 'build box', standing: 'enrolled', connected: true, open: 16, agents: 30, working: 20, waiting: 8, dropped: 0 },
  { node: 'n-lab', name: 'lab', standing: 'enrolled', connected: false, silent_since: FLEET_NOW - 12 * 60_000, open: 10, agents: 10, working: 0, waiting: 10, dropped: 0 },
];

const KINDS: AttentionKind[] = ['needs_input', 'needs_input', 'failed', 'finished_unseen'];

/** 40 cards: 14 on the laptop, 16 on the build box, 10 on the silent lab. Older cards have lower numbers. */
export const fleetCards: ConductorCardInfo[] = Array.from({ length: 40 }, (_, i) => {
  const n = i < 14 ? fleetNodes[0] : i < 30 ? fleetNodes[1] : fleetNodes[2];
  return {
    card: `${n.node}/c${i}`,
    node: n.node,
    node_name: n.name,
    ...(n.silent_since ? { silent_since: n.silent_since } : {}),
    obligation: {
      id: `c${i}`,
      workspace: `task-${i}`,
      kind: KINDS[i % KINDS.length],
      raised_at: FLEET_NOW - (40 - i) * 90_000,
      task: `Task ${i}`,
      prompt: i % KINDS.length < 2 ? `Run step ${i}?` : undefined,
    },
  };
});
