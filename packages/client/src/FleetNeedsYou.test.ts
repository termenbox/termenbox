import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import FleetNeedsYou from './FleetNeedsYou.svelte';
import { fleetItems, silentNodes, silentText } from './lib/fleet';
import { FLEET_NOW, fleetCards, fleetNodes } from './lib/fleetFixtures';

const html = (props: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(FleetNeedsYou, { props }).body.replace(/<!--.*?-->/g, '');
const items = fleetItems(fleetCards);

describe('fleet items', () => {
  it('keeps all 40 cards, oldest first among live nodes, silent nodes last', () => {
    expect(items).toHaveLength(40);
    const live = items.filter((i) => !i.silent);
    expect(live.map((i) => i.since)).toEqual([...live.map((i) => i.since)].sort((a, b) => a - b));
    expect(items.slice(-10).every((i) => i.silent)).toBe(true);
    expect(items.slice(0, 30).every((i) => !i.silent)).toBe(true);
  });
  it('names the silent node and how long it has been silent', () => {
    expect(silentNodes(fleetNodes).map((n) => n.node)).toEqual(['n-lab']);
    expect(silentText(fleetNodes[2], FLEET_NOW)).toBe('lab has been silent for 12 min; its 10 open asks wait');
  });
});

describe('FleetNeedsYou', () => {
  const out = html({ items, nodes: fleetNodes, now: FLEET_NOW, onopen: () => {} });
  it('shows a node chip on every item and a notice for the silent node', () => {
    expect(out.match(/data-fleet-chip/g)).toHaveLength(40);
    expect(out).toContain('data-fleet-silent="n-lab"');
    expect(out).toContain('laptop');
    expect(out).toContain('build box');
  });
  it('gives state in words, not colour alone, and disables cards of the silent node', () => {
    expect(out).toContain('waiting');
    expect(out).toContain('node silent');
    expect(out.match(/disabled/g)).toHaveLength(10);
  });
  it('has an empty state', () => {
    expect(html({ items: [], nodes: [], now: FLEET_NOW, onopen: () => {} })).toContain('Nothing needs you on any node.');
  });
});
