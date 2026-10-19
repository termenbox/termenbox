// Activity (U12, decision 0105): the tasks each server kept, as a timeline per day and a digest. A server answers a range
// of time; this module asks for it, turns the answers into rows, counts and chart buckets, and tracks deletion. Texts from
// the server (task titles, session names) are shown as text only.
import type { HistoryMessage, HistoryRun } from './protocol';

/** A server that has not answered by then is older than this feature (it ignores a message it does not know). */
export const ANSWER_TIMEOUT_MS = 8000;
export const DELETE_TIMEOUT_MS = 8000;

export type Mode = 'day' | 'week';
export interface Range { from: number; to: number }

// ---- Days. The client picks the range, so a day is the user's own local day.

export function startOfDay(ms: number): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}
export function addDays(dayStart: number, n: number): number {
  const d = new Date(dayStart);
  d.setDate(d.getDate() + n);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}
/** `anchor` is the start of the last day shown: a day, or the 7 days ending on it. */
export function rangeOf(mode: Mode, anchor: number): Range {
  return { from: mode === 'day' ? anchor : addDays(anchor, -6), to: addDays(anchor, 1) };
}
/** Steps the anchor by a day or a week; it never passes today (there is no future to show). */
export function step(mode: Mode, anchor: number, dir: 1 | -1, now: number): number {
  return Math.min(addDays(anchor, dir * (mode === 'day' ? 1 : 7)), startOfDay(now));
}
export const daysOf = (r: Range): number[] => {
  const out: number[] = [];
  for (let d = r.from; d < r.to; d = addDays(d, 1)) out.push(d);
  return out;
};

const fmt = (ms: number, o: Intl.DateTimeFormatOptions, locale?: string) => new Date(ms).toLocaleDateString(locale, o);
export const clock = (ms: number, locale?: string) => new Date(ms).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' });
const dayName = (ms: number, locale?: string) => fmt(ms, { weekday: 'short', day: 'numeric', month: 'short' }, locale);

export function dayLabel(dayStart: number, now: number, locale?: string): string {
  const today = startOfDay(now);
  if (dayStart === today) return 'Today';
  if (dayStart === addDays(today, -1)) return 'Yesterday';
  return dayName(dayStart, locale);
}
export function rangeLabel(mode: Mode, r: Range, now: number, locale?: string): string {
  if (mode === 'day') return dayLabel(r.from, now, locale);
  const last = addDays(r.to, -1);
  const [a, b] = [fmt(r.from, { day: 'numeric', month: 'short' }, locale), fmt(last, { day: 'numeric', month: 'short' }, locale)];
  return `${a} to ${b}`;
}

export function duration(ms: number): string {
  const min = Math.round(ms / 60_000);
  if (ms < 30_000) return ms < 1000 ? '0 min' : 'under 1 min';
  if (min < 60) return `${Math.max(min, 1)} min`;
  const h = Math.floor(min / 60);
  return min % 60 ? `${h} h ${min % 60} min` : `${h} h`;
}

/** A duration that fits a tile or a bar label: `12m`, `1h 5m`. */
export function compact(ms: number): string {
  const min = Math.round(ms / 60_000);
  if (min < 1) return ms > 0 ? '<1m' : '0m';
  if (min < 60) return `${min}m`;
  return min % 60 ? `${Math.floor(min / 60)}h ${min % 60}m` : `${Math.floor(min / 60)}h`;
}

// ---- What the servers said.

export type Outcome = 'finished' | 'failed' | 'stopped' | 'running';
export interface Row {
  serverId: string;
  serverName: string;
  session: string;
  agent?: string;
  task?: string;
  startedAt: number;
  endedAt?: number;
  outcome: Outcome;
  blockedMs: number;
  durationMs: number;
}
export interface Source { serverId: string; serverName: string; runs: HistoryRun[] }

const nonNegative = (n: unknown) => (typeof n === 'number' && Number.isFinite(n) && n > 0 ? n : 0);

export function rowOf(s: Pick<Source, 'serverId' | 'serverName'>, r: HistoryRun, now: number): Row {
  const endedAt = typeof r.ended_at === 'number' ? r.ended_at : undefined;
  const outcome: Outcome = endedAt === undefined ? 'running' : r.outcome === 'finished' || r.outcome === 'failed' ? r.outcome : 'stopped';
  return {
    serverId: s.serverId, serverName: s.serverName, session: r.session, agent: r.agent, task: r.task,
    startedAt: r.started_at, endedAt, outcome, blockedMs: nonNegative(r.blocked_ms),
    durationMs: nonNegative((endedAt ?? now) - r.started_at),
  };
}

/** Newest first; a task still running leads. */
export function rowsOf(sources: Source[], now: number): Row[] {
  const key = (r: Row) => r.endedAt ?? Number.POSITIVE_INFINITY;
  return sources.flatMap((s) => s.runs.map((r) => rowOf(s, r, now))).sort((a, b) => key(b) - key(a) || b.startedAt - a.startedAt);
}

export interface Digest { finished: number; failed: number; stopped: number; running: number; blockedMs: number }
export function digestOf(rows: Row[]): Digest {
  const d: Digest = { finished: 0, failed: 0, stopped: 0, running: 0, blockedMs: 0 };
  for (const r of rows) { d[r.outcome]++; d.blockedMs += r.blockedMs; }
  return d;
}
/** The digest in one sentence, for a screen reader and for the empty case. */
export function digestSentence(d: Digest): string {
  const parts = [`${d.finished} finished`, `${d.failed} failed`, `agents waited ${duration(d.blockedMs)} on you`];
  if (d.stopped) parts.splice(2, 0, `${d.stopped} stopped`);
  if (d.running) parts.push(`${d.running} still running`);
  return parts.join(', ');
}

export interface DayGroup { day: number; rows: Row[] }
/** Rows grouped by the day they ended on (a running task: the day it started, or the first day shown), newest day first. */
export function groupByDay(rows: Row[], r: Range): DayGroup[] {
  const groups = new Map<number, Row[]>();
  for (const row of rows) {
    const day = Math.max(startOfDay(row.endedAt ?? row.startedAt), r.from);
    groups.set(day, [...(groups.get(day) ?? []), row]);
  }
  return [...groups.entries()].sort((a, b) => b[0] - a[0]).map(([day, rows]) => ({ day, rows }));
}

export interface Bucket { label: string; finished: number; failed: number; blockedMs: number; /** Start of the hour or day. */ start: number }
/** Hours of the day in the day view, days in the week view. */
export function bucketsOf(rows: Row[], mode: Mode, r: Range, locale?: string): Bucket[] {
  const days = daysOf(r);
  const buckets: Bucket[] = mode === 'day'
    ? Array.from({ length: 24 }, (_, h) => ({ label: h % 6 === 0 ? String(h).padStart(2, '0') : '', finished: 0, failed: 0, blockedMs: 0, start: r.from + h * 3_600_000 }))
    : days.map((d) => ({ label: fmt(d, { weekday: 'short' }, locale), finished: 0, failed: 0, blockedMs: 0, start: d }));
  const at = (t: number): Bucket | undefined =>
    t < r.from || t >= r.to ? undefined : mode === 'day' ? buckets[new Date(t).getHours()] : buckets[days.indexOf(startOfDay(t))];
  for (const row of rows) {
    // A task still running counts where it began, or on the first day shown when it began before.
    const b = at(row.endedAt ?? Math.max(row.startedAt, r.from));
    if (!b) continue;
    if (row.outcome === 'finished') b.finished++;
    else if (row.outcome === 'failed') b.failed++;
    b.blockedMs += row.blockedMs;
  }
  return buckets;
}

/** One bar of a chart: its value as text, its parts, and what a screen reader says for it (empty: nothing to say). */
export interface Bar { label: string; value: string; parts: { n: number; tone: 'ok' | 'bad' | 'warn' }[]; spoken: string }
const at = (b: Bucket, mode: Mode, locale?: string) =>
  mode === 'day' ? `${new Date(b.start).toLocaleTimeString(locale, { hour: 'numeric' })}` : fmt(b.start, { weekday: 'long' }, locale);

export function taskBars(buckets: Bucket[], mode: Mode, locale?: string): Bar[] {
  return buckets.map((b) => ({
    label: b.label, value: b.finished + b.failed ? String(b.finished + b.failed) : '',
    parts: [{ n: b.finished, tone: 'ok' }, { n: b.failed, tone: 'bad' }],
    spoken: b.finished + b.failed ? `${at(b, mode, locale)} ${b.finished} finished${b.failed ? `, ${b.failed} failed` : ''}` : '',
  }));
}
export function waitBars(buckets: Bucket[], mode: Mode, locale?: string): Bar[] {
  return buckets.map((b) => ({
    label: b.label, value: b.blockedMs ? compact(b.blockedMs) : '', parts: [{ n: b.blockedMs, tone: 'warn' }],
    spoken: b.blockedMs ? `${at(b, mode, locale)} ${duration(b.blockedMs)}` : '',
  }));
}

// ---- Asking.

export interface HistoryEntry extends Range { sentAt: number; answer?: HistoryMessage }
export type HistoryMap = Record<string, HistoryEntry>;
export type Status = 'loading' | 'ready' | 'unavailable';

/** Notes that a server was asked for a range. The answer to the same range stays on screen while it refreshes. */
export function asked(map: HistoryMap, serverId: string, r: Range, now: number): HistoryMap {
  const same = map[serverId]?.from === r.from && map[serverId]?.to === r.to;
  return { ...map, [serverId]: { ...r, sentAt: same ? map[serverId].sentAt : now, answer: same ? map[serverId].answer : undefined } };
}
/** An answer counts only for the range last asked for; a late one for an earlier range is dropped. */
export function answered(map: HistoryMap, serverId: string, msg: HistoryMessage): HistoryMap {
  const e = map[serverId];
  return e && e.from === msg.from && e.to === msg.to ? { ...map, [serverId]: { ...e, answer: msg } } : map;
}
export function statusOf(e: HistoryEntry | undefined, now: number): Status {
  if (e?.answer) return 'ready';
  return e && now - e.sentAt >= ANSWER_TIMEOUT_MS ? 'unavailable' : 'loading';
}
/** The overall state of the view: the first answer is enough to show something; it is loading while nothing has answered yet. */
export function overall(statuses: Status[]): Status {
  if (statuses.includes('ready')) return 'ready';
  return statuses.length && statuses.every((s) => s === 'unavailable') ? 'unavailable' : 'loading';
}

// ---- Deleting.

export interface DeleteResult { removed: number; /** Servers that did not confirm in time. */ failed: string[] }
/** What the user is told: how many servers answered, how many tasks went, and the names of those that did not confirm. */
export interface HistoryDeletion { servers: number; removed: number; failedNames: string[] }
/** Asks each server to forget its history and waits for every `history_deleted`, up to a limit. */
export class DeleteBook {
  private waiting = new Map<string, (removed: number) => void>();
  run(ids: string[], send: (id: string) => void, timeoutMs = DELETE_TIMEOUT_MS): Promise<DeleteResult> {
    return new Promise((resolve) => {
      let removed = 0;
      const pending = new Set(ids);
      const finish = () => { clearTimeout(timer); for (const id of ids) this.waiting.delete(id); resolve({ removed, failed: [...pending] }); };
      const timer = setTimeout(finish, timeoutMs);
      if (!ids.length) return finish();
      for (const id of ids) {
        this.waiting.set(id, (n) => { removed += n; pending.delete(id); if (!pending.size) finish(); });
        send(id);
      }
    });
  }
  reply(serverId: string, removed: number) {
    const done = this.waiting.get(serverId);
    this.waiting.delete(serverId);
    done?.(removed);
  }
}
