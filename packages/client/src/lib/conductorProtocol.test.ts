import { describe, expect, it } from 'vitest';
import type { ConductorMessage, NodeMessage } from './protocol';

// The same JSON the round-trip tests in packages/core/src/conductor.rs write and read.
describe('conductor link messages', () => {
  it('a node message parses with the fields this client knows', () => {
    const wire = '{"type":"answered","epoch":7,"seq":4,"id":"a1","actor":"me","owner":"me","device":"phone","option":"1"}';
    const m = JSON.parse(wire) as NodeMessage;
    expect(m.type).toBe('answered');
    if (m.type !== 'answered') return;
    expect(m.actor).toBe('me');
    expect(m.owner).toBe('me');
  });

  it('an answer from the conductor names the actor and the owner apart', () => {
    const m: ConductorMessage = {
      type: 'answer',
      target: { workspace: 'api', prompt_id: 'p' },
      id: 'a1',
      actor: 'me',
      owner: 'me',
      device: 'desktop',
      option: '2',
      idem: 'i',
    };
    const back = JSON.parse(JSON.stringify(m)) as ConductorMessage;
    expect(back).toEqual(m);
  });
});
