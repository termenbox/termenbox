import { describe, expect, it } from 'vitest';
import { ANSWER_TIMEOUT_MS } from './history';
import { ageText, answeredHealth, askedHealth, healthStatus, metricView, rttText, scopeNote, sinceText, unhealthyText, verdictOf } from './health';
import { healthyHistory, history, live, samples } from './healthFixture';
import { NOW } from './historyFixture';

describe('asking', () => {
  it('is loading until the answer, unavailable after the timeout, ready after the answer', () => {
    let m = askedHealth({}, 'a', 1000);
    expect(healthStatus(m.a, 1000)).toBe('loading');
    expect(healthStatus(m.a, 1000 + ANSWER_TIMEOUT_MS)).toBe('unavailable');
    m = answeredHealth(m, 'a', history());
    expect(healthStatus(m.a, 1000 + ANSWER_TIMEOUT_MS * 5)).toBe('ready');
  });
  it('keeps the answer on screen and the first clock while it refreshes', () => {
    let m = answeredHealth(askedHealth({}, 'a', 1000), 'a', history());
    m = askedHealth(m, 'a', 40_000);
    expect(m.a.sentAt).toBe(1000);
    expect(m.a.answer).toBeDefined();
  });
  it('starts the clock over on a retry, so the section says loading again', () => {
    let m = askedHealth({}, 'a', 1000);
    expect(healthStatus(m.a, 1000 + ANSWER_TIMEOUT_MS)).toBe('unavailable');
    m = askedHealth(m, 'a', 1000 + ANSWER_TIMEOUT_MS, true);
    expect(healthStatus(m.a, 1000 + ANSWER_TIMEOUT_MS)).toBe('loading');
  });
  it('ignores an answer nobody asked for', () => {
    expect(answeredHealth({}, 'a', history())).toEqual({});
  });
});

describe('the verdict is the server\'s', () => {
  it('uses the live push when it is as new as the last sample, else the answer', () => {
    const h = history();
    expect(verdictOf(live({ healthy: true, unhealthy: [] }), h)?.healthy).toBe(true);
    expect(verdictOf(live({ ts: NOW - 600_000 }), h)).toMatchObject({ healthy: false, unhealthy: ['ram'] });
    expect(verdictOf(undefined, h)?.healthy).toBe(false);
    expect(verdictOf(undefined, undefined)).toBeUndefined();
  });
  it('names what is over its level, in words', () => {
    expect(unhealthyText(['ram'])).toBe('RAM is over its level');
    expect(unhealthyText(['cpu', 'ram', 'disk'])).toBe('CPU, RAM and Disk are over their levels');
    expect(unhealthyText([])).toBe('');
  });
});

describe('a metric from the fixture', () => {
  const h = history();
  it('draws a gap where the server could not read a value, not a zero', () => {
    const cpu = metricView(h, 'cpu');
    expect(cpu.known).toBe(359);
    expect(cpu.segments).toHaveLength(1);
    expect(cpu.segments[0].startsWith('M')).toBe(true);
    const gap = history({ samples: samples().map((p, i) => (i === 100 ? { ...p, cpu_percent: undefined } : p)) });
    expect(metricView(gap, 'cpu').segments).toHaveLength(2);
  });
  it('finds the peak, the time over the level and the current value', () => {
    const cpu = metricView(h, 'cpu');
    expect(cpu.peak?.value).toBe(96);
    expect(cpu.overMs).toBe(5 * 60_000);
    expect(cpu.spoken).toContain('highest 96%');
    expect(cpu.spoken).toContain('at or above its 90% level for about 5 min');
    const ram = metricView(h, 'ram');
    expect(ram.now).toBe(93);
    expect(ram.enter).toBe(90);
    expect(ram.exit).toBe(80);
    expect(metricView(h, 'disk').spoken).toContain('never at its 90% level');
  });
  it('puts the newest sample at the right edge and older ones to its left, on a six hour scale', () => {
    const ram = metricView(h, 'ram');
    expect(ram.segments[0].split(' L').at(-1)).toMatch(/^(L)?100 /);
    expect(ram.segments[0]).toMatch(/^M0\.\d+ /); // the oldest of 360 minutes sits at the left edge
    const short = metricView(history({ samples: samples({ points: 30 }) }), 'ram');
    expect(Number(short.segments[0].match(/^M([\d.]+) /)![1])).toBeGreaterThan(90);
  });
  it('does not call an old reading current: the line stops short of the edge and the age is said', () => {
    const later = NOW + 20 * 60_000;
    const ram = metricView(h, 'ram', later);
    expect(ram.stale).toBe(true);
    expect(ram.ageMs).toBe(20 * 60_000);
    expect(ram.spoken).toContain('last reading 93% 20 min ago');
    expect(ram.spoken).not.toContain('now 93%');
    expect(Number(ram.segments[0].split(' ').at(-2)!.replace(/^L/, ''))).toBeLessThan(95);
    const fresh = metricView(h, 'ram', NOW + 5_000);
    expect(fresh.stale).toBe(false);
    expect(fresh.spoken).toContain('now 93%');
  });
  it('shows no current value when the newest sample could not read it, not the one before', () => {
    const lastUnknown = history({ samples: samples().map((p, i, all) => (i === all.length - 1 ? { ts: p.ts, ram_percent: undefined } : p)) });
    const ram = metricView(lastUnknown, 'ram');
    expect(ram.now).toBeUndefined();
    expect(ram.spoken).toContain('no current reading');
    expect(metricView(lastUnknown, 'cpu').now).toBeUndefined();
  });
  it('says how long ago in minutes, then hours', () => {
    expect([0, 90_000, 119 * 60_000, 180 * 60_000].map(ageText)).toEqual(['1 min', '2 min', '119 min', '3 h']);
  });
  it('says a metric is not available when the server sent none', () => {
    const none = history({ samples: samples().map((p) => ({ ts: p.ts, ram_percent: p.ram_percent })) });
    const disk = metricView(none, 'disk');
    expect(disk.known).toBe(0);
    expect(disk.spoken).toBe('Disk: not available on this server.');
  });
  it('says what a figure is measured against', () => {
    expect(scopeNote('ram', 'container')).toContain("container's memory limit");
    expect(scopeNote('ram', 'host')).toContain('no container limit');
    expect(sinceText(healthyHistory())).toBe('6 hours ago');
    expect(sinceText(history({ samples: samples({ points: 30 }) }))).toMatch(/^since /);
  });
  it('shows the round trip of this device, or that it has none yet', () => {
    expect(rttText(null)).toBe('not measured yet');
    expect(rttText(41.6)).toBe('42 ms');
  });
});
