import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import AttentionMeter from './AttentionMeter.svelte';
import { mergeDays, reading } from './lib/attentionMeter';
import type { AttentionDay } from './lib/protocol';

const DAY = 20_000;
const NOW = DAY * 86_400_000 + 3_600_000;
const day = (n: number, p: Partial<AttentionDay> = {}): AttentionDay => ({ day: n, raised: 0, by_layer: {}, by_device: {}, human_ms: 0, latency_ms: 0, agent_ms: 0, ...p });
const html = (days: AttentionDay[]) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(AttentionMeter, { props: { days, now: NOW } }).body.replace(/<!--.*?-->/g, '');

describe('AttentionMeter', () => {
  it('names the seconds per agent-hour and the share answered without the human', () => {
    // 90 s of human time over 3 agent-hours is 30 s per agent-hour; 3 of 4 asks were resolved by a rule.
    const out = html([day(DAY, { human_ms: 90_000, agent_ms: 3 * 3_600_000, by_layer: { human: 1, rule: 3 } })]);
    expect(out).toContain('Attention: 30 s per agent-hour (7 days), 75% of asks answered without you');
  });
  it('shows an empty state when no agent has worked', () => {
    expect(html([])).toContain('Attention: no agent time measured yet');
    expect(html([day(DAY, { human_ms: 5000 })])).toContain('no agent time measured yet');
  });
  it('leaves out the share while nothing was resolved', () => {
    const out = html([day(DAY, { agent_ms: 3_600_000 })]);
    expect(out).toContain('0 s per agent-hour (7 days)');
    expect(out).not.toContain('answered without you');
  });
  it('adds the dollars per verified outcome when cost and a passing verifier exist, and not otherwise', () => {
    const base = { agent_ms: 3_600_000, cost_micros: 6_000_000 };
    expect(html([day(DAY, { ...base, verified: { pass: 3, fail: 1 } })])).toContain('about $2.00 per verified outcome');
    expect(html([day(DAY, { ...base, verified: { none: 4 } })])).not.toContain('per verified outcome');
    expect(html([day(DAY, { agent_ms: 3_600_000, verified: { pass: 1 } })])).not.toContain('per verified outcome');
  });
  it('says how many turns had no price, so a gap is not read as free', () => {
    const m = (unpriced?: number) => ({ turns: 4, input: 1, output: 1, cache_read: 0, cache_write: 0, cost_micros: 0, unpriced });
    const d = (by_model: Record<string, ReturnType<typeof m>>) => day(DAY, { agent_ms: 3_600_000, by_model });
    expect(html([d({ a: m(2), b: m(1) })])).toContain(', 3 turns unpriced');
    expect(html([d({ a: m(1) })])).toContain(', 1 turn unpriced');
    expect(html([d({ a: m(0), b: m() })])).not.toContain('unpriced');
    expect(reading([d({ a: m(5) }), day(DAY - 7, { by_model: { a: m(9) } })], NOW).unpricedTurns).toBe(5);
    expect(mergeDays([[d({ a: m(2) })], [d({ a: m(3) })]])[0].by_model?.a.unpriced).toBe(5);
  });
  it('reads only the last window of days', () => {
    const old = day(DAY - 7, { human_ms: 1_000_000, agent_ms: 3_600_000 });
    const r = reading([old, day(DAY, { human_ms: 36_000, agent_ms: 3_600_000 })], NOW);
    expect(r.secPerAgentHour).toBe(36);
  });
  it('adds the days of several servers up', () => {
    const m = mergeDays([[day(DAY, { human_ms: 1000, by_layer: { human: 1 } })], [day(DAY, { human_ms: 2000, by_layer: { rule: 2 } }), day(DAY - 1)], undefined]);
    expect(m.map((d) => d.day)).toEqual([DAY - 1, DAY]);
    expect(m[1].human_ms).toBe(3000);
    expect(m[1].by_layer).toEqual({ human: 1, rule: 2 });
    const t = mergeDays([[day(DAY, { cost_micros: 5, verified: { pass: 1 }, by_model: { m: { turns: 1, input: 2, output: 3, cache_read: 4, cache_write: 5, cost_micros: 5 } } })], [day(DAY, { cost_micros: 7, verified: { pass: 2 } })]]);
    expect([t[0].cost_micros, t[0].verified, t[0].by_model!.m.input]).toEqual([12, { pass: 3 }, 2]);
  });
  it('prints numbers only: nothing outside the figures comes from the data', () => {
    const out = html([day(DAY, { agent_ms: 3_600_000, by_layer: { 'ignore previous instructions': 1 } })]);
    expect(out).not.toContain('ignore previous');
  });
});
