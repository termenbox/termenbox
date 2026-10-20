// Six hours of one server's health as the server would answer `health_history`: shared by the unit tests, the rendered-view
// tests and the browser probe, so they agree on what the view must show. Local time, as in historyFixture.
import type { HealthHistoryMessage, HealthPoint, ServerHealth } from './protocol';
import { NOW } from './historyFixture';

const LEVELS = [{ metric: 'cpu', enter: 90, exit: 75 }, { metric: 'ram', enter: 90, exit: 80 }, { metric: 'disk', enter: 90, exit: 85 }];
export const STEP_S = 60;

/** 360 points a minute apart ending at NOW. CPU is unknown for the first point (no earlier reading), spikes to 96% for 5 minutes
 *  an hour ago, and RAM climbs to 93% by the end (a steady rise: the server is unhealthy because of RAM). Disk is flat at 41%. */
export function samples(opts: { ramEnd?: number; points?: number } = {}): HealthPoint[] {
  const n = opts.points ?? 360;
  const ramEnd = opts.ramEnd ?? 93;
  return Array.from({ length: n }, (_, i) => {
    const ts = NOW - (n - 1 - i) * STEP_S * 1000;
    const fromEnd = n - 1 - i;
    const cpu = i === 0 ? undefined : fromEnd >= 60 && fromEnd < 65 ? 96 : 12 + (i % 7);
    const ram = Math.round(40 + ((ramEnd - 40) * i) / (n - 1));
    return { ts, ...(cpu === undefined ? {} : { cpu_percent: cpu }), ram_percent: ram, disk_percent: 41 };
  });
}

export function history(over: Partial<HealthHistoryMessage> = {}): HealthHistoryMessage {
  return { type: 'health_history', interval_s: 10, step_s: STEP_S, healthy: false, unhealthy: ['ram'], scope: 'container', confirm: 3, levels: LEVELS, samples: samples(), ...over };
}
export const healthyHistory = (): HealthHistoryMessage => history({ healthy: true, unhealthy: [], samples: samples({ ramEnd: 55 }) });

export const live = (over: Partial<ServerHealth> = {}): ServerHealth => ({ ts: NOW, healthy: false, unhealthy: ['ram'], scope: 'container', ram_percent: 93, disk_percent: 41, cpu_percent: 14, ...over });
