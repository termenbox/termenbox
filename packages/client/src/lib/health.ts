// Server health (U14, decision 0105): what a server says about its own CPU, RAM and disk, for the dot on Home and the health
// section of Activity. The server decides `healthy` (a level held for several samples, with a lower level to recover); this
// module never recomputes it, it only lays out what the server sent. Numbers and names from the server are shown as text.
import { ANSWER_TIMEOUT_MS, clock, type Status } from './history';
import type { HealthHistoryMessage, HealthPoint, ServerHealth } from './protocol';

export type MetricId = 'cpu' | 'ram' | 'disk';
const KEY = { cpu: 'cpu_percent', ram: 'ram_percent', disk: 'disk_percent' } as const;
const NAME: Record<MetricId, string> = { cpu: 'CPU', ram: 'RAM', disk: 'Disk' };
export const METRICS: MetricId[] = ['cpu', 'ram', 'disk'];
const HOUR = 3_600_000;
/** The chart always spans six hours, the length of the server's ring: a server that started later shows a shorter line, not a stretched one. */
export const SPAN_MS = 6 * HOUR;

// ---- Asking. A server older than this feature never answers `health_history`.

export interface HealthEntry { sentAt: number; answer?: HealthHistoryMessage }
export type HealthMap = Record<string, HealthEntry>;

/** Notes that a server was asked. An answer already held stays on screen while it refreshes (its age is shown); the clock starts at the first ask. */
export function askedHealth(map: HealthMap, serverId: string, now: number, restart = false): HealthMap {
  const e = map[serverId];
  // `restart` is a "Try again": the clock starts over, so the section says loading again until an answer or the timeout.
  return { ...map, [serverId]: { sentAt: restart ? now : e?.sentAt ?? now, answer: e?.answer } };
}
export function answeredHealth(map: HealthMap, serverId: string, msg: HealthHistoryMessage): HealthMap {
  const e = map[serverId];
  return e ? { ...map, [serverId]: { ...e, answer: msg } } : map;
}
export function healthStatus(e: HealthEntry | undefined, now: number): Status {
  if (e?.answer) return 'ready';
  return e && now - e.sentAt >= ANSWER_TIMEOUT_MS ? 'unavailable' : 'loading';
}

// ---- What to show.

export interface Verdict { healthy: boolean; unhealthy: string[]; scope: string; point?: HealthPoint }

/** The newest verdict: the live push if it is at least as new as the last sample of the answer, else the answer's. */
export function verdictOf(live: ServerHealth | undefined, answer: HealthHistoryMessage | undefined): Verdict | undefined {
  const last = answer?.samples.at(-1);
  if (live && (!last || live.ts >= last.ts)) return { healthy: live.healthy, unhealthy: live.unhealthy, scope: live.scope, point: live };
  return answer ? { healthy: answer.healthy, unhealthy: answer.unhealthy, scope: answer.scope, point: last } : undefined;
}

/** "RAM is over its level", "CPU and disk are over their levels". */
export function unhealthyText(unhealthy: string[]): string {
  const names = unhealthy.map((m) => NAME[m as MetricId] ?? m);
  if (!names.length) return '';
  const list = names.length > 1 ? `${names.slice(0, -1).join(', ')} and ${names.at(-1)}` : names[0];
  return `${list} ${names.length > 1 ? 'are' : 'is'} over ${names.length > 1 ? 'their levels' : 'its level'}`;
}

/** What a metric measures, in words (the scope is what makes the RAM figure mean something). */
export function scopeNote(id: MetricId, scope: string): string {
  if (id === 'ram') return scope === 'container' ? "of the container's memory limit" : "of the machine's memory (no container limit is set)";
  if (id === 'cpu') return scope === 'container' ? 'of the CPU the container may use' : 'of the CPU';
  return 'of the disk holding the server data';
}

export interface MetricView {
  id: MetricId;
  label: string;
  /** The newest sample's value, in percent; absent when the newest sample could not read it. */
  now?: number;
  /** How old the newest sample is, and whether that is too old to call it current. */
  ageMs: number;
  stale: boolean;
  peak?: { value: number; ts: number };
  /** About how long the metric was at or above its level in the window. */
  overMs: number;
  enter: number;
  exit: number;
  /** SVG path data (viewBox 0 0 100 40): one subpath per run of known samples, so a gap is a gap and not a zero. */
  segments: string[];
  /** Where the dashed level line sits (y, in the same box). */
  levelY: number;
  known: number;
  /** The sentence a screen reader gets for the chart. */
  spoken: string;
}

export const CHART_H = 40;
const yOf = (v: number) => +(CHART_H - (Math.min(100, Math.max(0, v)) / 100) * CHART_H).toFixed(2);

/** `at` is the time the chart's right edge stands for (the present); a server whose samples stopped shows a line that stops short of it. */
export function metricView(msg: HealthHistoryMessage, id: MetricId, at = msg.samples.at(-1)?.ts ?? 0): MetricView {
  const key = KEY[id];
  const level = msg.levels.find((l) => l.metric === id);
  const enter = level?.enter ?? 90;
  const exit = level?.exit ?? 80;
  const segments: string[] = [];
  let run: string[] = [];
  let peak: MetricView['peak'];
  let over = 0;
  let known = 0;
  for (const p of msg.samples) {
    const v = p[key];
    if (typeof v !== 'number') {
      if (run.length) segments.push(run.join(' '));
      run = [];
      continue;
    }
    known++;
    if (!peak || v > peak.value) peak = { value: v, ts: p.ts };
    if (v >= enter) over += msg.step_s * 1000;
    const x = +(100 - ((at - p.ts) / SPAN_MS) * 100).toFixed(2);
    run.push(`${run.length ? 'L' : 'M'}${Math.max(0, x)} ${yOf(v)}`);
  }
  if (run.length) segments.push(run.join(' '));
  const last = msg.samples.at(-1);
  const nowValue = typeof last?.[key] === 'number' ? last[key] : undefined;
  const ageMs = last ? Math.max(0, at - last.ts) : 0;
  // Three missed samples, with a little for the network.
  const stale = ageMs > 3 * msg.interval_s * 1000 + 5000;
  const label = NAME[id];
  let spoken = `${label}: not available on this server.`;
  if (known) {
    const mins = Math.round(over / 60_000);
    const current = nowValue === undefined ? 'no current reading' : stale ? `last reading ${nowValue}% ${ageText(ageMs)} ago` : `now ${nowValue}%`;
    spoken = `${label} over the last hours: ${current}, highest ${peak!.value}% at ${clock(peak!.ts)}, ${over ? `at or above its ${enter}% level for about ${mins < 1 ? 'a minute' : `${mins} min`}` : `never at its ${enter}% level`}.`;
  }
  return { id, label, now: nowValue, ageMs, stale, peak, overMs: over, enter, exit, segments, levelY: yOf(enter), known, spoken };
}

/** "3 min", "2 h": how long ago, for a reading that is no longer current. */
export function ageText(ms: number): string {
  const m = Math.max(1, Math.round(ms / 60_000));
  return m < 120 ? `${m} min` : `${Math.round(m / 60)} h`;
}

/** The coverage of the answer: how long the server has been sampling, to say "since 14:02" instead of implying six hours. */
export function coverageMs(msg: HealthHistoryMessage): number {
  const [a, b] = [msg.samples[0]?.ts, msg.samples.at(-1)?.ts];
  return a === undefined || b === undefined ? 0 : b - a;
}
export const sinceText = (msg: HealthHistoryMessage): string => (coverageMs(msg) >= SPAN_MS - msg.step_s * 2000 ? '6 hours ago' : `since ${clock(msg.samples[0]?.ts ?? 0)}`);

/** This device's round trip, as text; the server cannot know it. */
export const rttText = (ms: number | null): string => (ms === null ? 'not measured yet' : `${Math.round(ms)} ms`);
