import { describe, expect, it } from 'vitest';
import { nodeTerminalTarget } from './fleet';
import { fleetCards } from './fleetFixtures';

describe('nodeTerminalTarget', () => {
  it('names the node, workspace and a client-only id for a live card', () => {
    expect(nodeTerminalTarget(fleetCards, 's1', 'n-laptop/c0')).toEqual({
      serverId: 's1', node: 'n-laptop', nodeName: 'laptop', workspace: 'task-0', tab: undefined, id: 'node:n-laptop:task-0',
    });
  });
  it('refuses a card of a silent node and an unknown card', () => {
    expect(nodeTerminalTarget(fleetCards, 's1', 'n-lab/c35')).toBeNull();
    expect(nodeTerminalTarget(fleetCards, 's1', 'nope')).toBeNull();
  });
});
