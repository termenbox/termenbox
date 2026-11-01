import type { AttentionDay } from './protocol';

/** What the meter line says (decision 0169): numbers only, never agent text. */
export interface MeterReading {
  /** Human seconds per agent-hour over the window; null while no agent has worked. */
  secPerAgentHour: number | null;
  /** Share of resolved asks answered without the human, 0 to 100; null while none was resolved. */
  autoPercent: number | null;
}

const DAY_MS = 86_400_000;

/** The daily buckets of every server added up by day (the baseline is the owner's whole fleet), oldest first. */
export function mergeDays(perServer: (AttentionDay[] | undefined)[]): AttentionDay[] {
  const byDay = new Map<number, AttentionDay>();
  for (const days of perServer) for (const d of days ?? []) {
    const m = byDay.get(d.day) ?? { day: d.day, raised: 0, by_layer: {}, by_device: {}, human_ms: 0, latency_ms: 0, agent_ms: 0 };
    m.raised += d.raised; m.human_ms += d.human_ms; m.latency_ms += d.latency_ms; m.agent_ms += d.agent_ms;
    for (const [k, v] of Object.entries(d.by_layer)) m.by_layer[k] = (m.by_layer[k] ?? 0) + v;
    for (const [k, v] of Object.entries(d.by_device)) m.by_device[k] = (m.by_device[k] ?? 0) + v;
    byDay.set(d.day, m);
  }
  return [...byDay.values()].sort((a, b) => a.day - b.day);
}

/** The reading over the last `window` UTC days up to and including the day of `now` (ms). */
export function reading(days: AttentionDay[], now: number, window = 7): MeterReading {
  const today = Math.floor(now / DAY_MS);
  const inWindow = days.filter((d) => d.day > today - window && d.day <= today);
  const humanMs = inWindow.reduce((n, d) => n + d.human_ms, 0);
  const agentMs = inWindow.reduce((n, d) => n + d.agent_ms, 0);
  let resolved = 0; let human = 0;
  for (const d of inWindow) for (const [layer, n] of Object.entries(d.by_layer)) { resolved += n; if (layer === 'human') human += n; }
  return {
    secPerAgentHour: agentMs > 0 ? Math.round(humanMs / 1000 / (agentMs / 3_600_000)) : null,
    autoPercent: resolved > 0 ? Math.round(((resolved - human) / resolved) * 100) : null,
  };
}
