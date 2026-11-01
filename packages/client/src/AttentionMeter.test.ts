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
  });
  it('prints numbers only: nothing outside the figures comes from the data', () => {
    const out = html([day(DAY, { agent_ms: 3_600_000, by_layer: { 'ignore previous instructions': 1 } })]);
    expect(out).not.toContain('ignore previous');
  });
});
