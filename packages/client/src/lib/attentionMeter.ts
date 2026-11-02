import type { AttentionDay } from './protocol';

/** What the meter line says (decision 0169): numbers only, never agent text. */
export interface MeterReading {
  /** Human seconds per agent-hour over the window; null while no agent has worked. */
  secPerAgentHour: number | null;
  /** Share of resolved asks answered without the human, 0 to 100; null while none was resolved. */
  autoPercent: number | null;
  /** Estimated US dollars per task a verifier passed (plan step 1.4); null without cost data or without a pass. */
  dollarsPerVerified: number | null;
  /** Turns whose model had no price, so the cost above leaves them out (plan step 1.4b); 0 when none. */
  unpricedTurns: number;
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
    m.cost_micros = (m.cost_micros ?? 0) + (d.cost_micros ?? 0);
    for (const [k, v] of Object.entries(d.verified ?? {})) (m.verified ??= {})[k] = (m.verified[k] ?? 0) + v;
    for (const [k, v] of Object.entries(d.by_model ?? {})) {
      const u = ((m.by_model ??= {})[k] ??= { turns: 0, input: 0, output: 0, cache_read: 0, cache_write: 0, cost_micros: 0 });
      u.turns += v.turns; u.input += v.input; u.output += v.output; u.cache_read += v.cache_read; u.cache_write += v.cache_write; u.cost_micros += v.cost_micros; u.unpriced = (u.unpriced ?? 0) + (v.unpriced ?? 0);
    }
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
  const costMicros = inWindow.reduce((n, d) => n + (d.cost_micros ?? 0), 0);
  const passed = inWindow.reduce((n, d) => n + (d.verified?.pass ?? 0), 0);
  const unpricedTurns = inWindow.reduce((n, d) => n + Object.values(d.by_model ?? {}).reduce((k, u) => k + (u.unpriced ?? 0), 0), 0);
  return {
    unpricedTurns,
    dollarsPerVerified: costMicros > 0 && passed > 0 ? costMicros / 1e6 / passed : null,
    secPerAgentHour: agentMs > 0 ? Math.round(humanMs / 1000 / (agentMs / 3_600_000)) : null,
    autoPercent: resolved > 0 ? Math.round(((resolved - human) / resolved) * 100) : null,
  };
}
