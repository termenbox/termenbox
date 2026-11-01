import { describe, expect, it } from 'vitest';
import { AskDwell } from './askDwell';

const a = { serverId: 's', id: 'a1' };

function setup() {
  const got: [string, string, number][] = [];
  return { got, d: new AskDwell((s, id, ms) => got.push([s, id, ms])) };
}

describe('AskDwell', () => {
  it('reports the time from the question coming into view to it going away', () => {
    const { got, d } = setup();
    d.watch(a, 1000);
    d.watch(a, 3000);
    expect(got).toEqual([]);
    d.watch(null, 6000);
    expect(got).toEqual([['s', 'a1', 5000]]);
  });

  it('does not count time while the question is out of view', () => {
    const { got, d } = setup();
    d.watch(null, 0);
    d.watch(a, 1000);
    d.watch(null, 3000);
    d.watch(null, 9000);
    expect(got).toEqual([['s', 'a1', 2000]]);
  });

  it('ends one question and starts the next', () => {
    const { got, d } = setup();
    d.watch(a, 0);
    d.watch({ serverId: 's', id: 'a2' }, 4000);
    d.watch(null, 5000);
    expect(got).toEqual([['s', 'a1', 4000], ['s', 'a2', 1000]]);
  });

  it('ignores a glance under half a second', () => {
    const { got, d } = setup();
    d.watch(a, 0);
    d.watch(null, 400);
    expect(got).toEqual([]);
  });
});
