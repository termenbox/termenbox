// Activity (U12, decision 0105): the tasks each server kept, as a timeline per day and a digest. A server answers a range
// of time; this module asks for it, turns the answers into rows, counts and chart buckets, and tracks deletion. Texts from
// the server (task titles, workspace names) are shown as text only.
import type { HistoryMessage, HistoryRun, RunUsage } from './protocol';

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
/** The start of local hour `h` of the day that starts at `dayStart`. A day of 23 or 25 hours is not 24 x 3600 s long. */
function hourOf(dayStart: number, h: number): number {
  const d = new Date(dayStart);
  d.setHours(h, 0, 0, 0);
  return d.getTime();
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
  workspace: string;
  agent?: string;
  task?: string;
  startedAt: number;
  endedAt?: number;
  outcome: Outcome;
  blockedMs: number;
  durationMs: number;
  project?: string;
  usage?: Usage;
}
export interface Source { serverId: string; serverName: string; runs: HistoryRun[] }

const nonNegative = (n: unknown) => (typeof n === 'number' && Number.isFinite(n) && n > 0 ? n : 0);

export function rowOf(s: Pick<Source, 'serverId' | 'serverName'>, r: HistoryRun, now: number): Row {
  const endedAt = typeof r.ended_at === 'number' ? r.ended_at : undefined;
  const outcome: Outcome = endedAt === undefined ? 'running' : r.outcome === 'finished' || r.outcome === 'failed' ? r.outcome : 'stopped';
  return {
    serverId: s.serverId, serverName: s.serverName, workspace: r.workspace, agent: r.agent, task: r.task,
    startedAt: r.started_at, endedAt, outcome, blockedMs: nonNegative(r.blocked_ms),
    durationMs: nonNegative((endedAt ?? now) - r.started_at),
    project: r.project || undefined, usage: usageOfRun(r.usage),
  };
}

/** Newest first; a task still running leads. */
export function rowsOf(sources: Source[], now: number): Row[] {
  const key = (r: Row) => r.endedAt ?? Number.POSITIVE_INFINITY;
  return sources.flatMap((s) => s.runs.map((r) => rowOf(s, r, now))).sort((a, b) => key(b) - key(a) || b.startedAt - a.startedAt);
}

// ---- Usage and cost (U13). Counts the agents exposed, and nothing worked out from them.

/** What one task exposed. A field that is `undefined` was not reported; it is never shown as zero. */
export interface Usage { input?: number; output?: number; cacheRead?: number; cacheWrite?: number; costMicros?: number; currency?: string; rateLimited: number }
const count = (n: unknown) => (typeof n === 'number' && Number.isSafeInteger(n) && n >= 0 ? n : undefined);
export function usageOfRun(u: RunUsage | undefined): Usage | undefined {
  if (!u || typeof u !== 'object') return undefined;
  const cost = count(u.cost_micros);
  const currency = typeof u.cost_currency === 'string' && /^[A-Za-z]{1,5}$/.test(u.cost_currency) ? u.cost_currency.toUpperCase() : undefined;
  const out: Usage = {
    input: count(u.input_tokens), output: count(u.output_tokens), cacheRead: count(u.cache_read_tokens), cacheWrite: count(u.cache_write_tokens),
    costMicros: cost !== undefined && currency ? cost : undefined, currency: cost !== undefined ? currency : undefined, rateLimited: count(u.rate_limited) ?? 0,
  };
  const any = [out.input, out.output, out.cacheRead, out.cacheWrite, out.costMicros].some((n) => n !== undefined) || out.rateLimited > 0;
  return any ? out : undefined;
}
const plus = (a: number | undefined, b: number | undefined) => (b === undefined ? a : (a ?? 0) + b);

export interface UsageTotals {
  /** Tasks, and how many of them carried counts. */
  tasks: number; withCounts: number;
  input?: number; output?: number; cacheRead?: number; cacheWrite?: number;
  /** Cost the agents reported, per currency (millionths). Absent when none did. */
  cost?: Record<string, number>;
  /** Tasks in which the agent reported tokens / a cost. */
  withTokens: number; withCost: number;
  rateLimited: number;
}
export const emptyTotals = (): UsageTotals => ({ tasks: 0, withCounts: 0, withTokens: 0, withCost: 0, rateLimited: 0 });
export function addUsage(t: UsageTotals, u: Usage | undefined): UsageTotals {
  const n = { ...t, tasks: t.tasks + 1 };
  if (!u) return n;
  n.withCounts++;
  n.input = plus(n.input, u.input); n.output = plus(n.output, u.output);
  n.cacheRead = plus(n.cacheRead, u.cacheRead); n.cacheWrite = plus(n.cacheWrite, u.cacheWrite);
  if ([u.input, u.output, u.cacheRead, u.cacheWrite].some((v) => v !== undefined)) n.withTokens++;
  if (u.costMicros !== undefined && u.currency) { n.cost = { ...n.cost, [u.currency]: (n.cost?.[u.currency] ?? 0) + u.costMicros }; n.withCost++; }
  n.rateLimited += u.rateLimited;
  return n;
}
/** Input plus output tokens: what a chart and a ranking use. Cache reads and writes are shown apart, but a figure that
 * only has cache counts is still a reported figure, not a missing one. */
export const tokensOf = (t: { input?: number; output?: number; cacheRead?: number; cacheWrite?: number }) =>
  [t.input, t.output, t.cacheRead, t.cacheWrite].every((v) => v === undefined) ? undefined : (t.input ?? 0) + (t.output ?? 0);

export interface AgentUsage { agent: string; totals: UsageTotals }
export interface ProjectUsage { project: string; totals: UsageTotals }
export const NO_AGENT = 'Unnamed agent';
export const NO_PROJECT = 'No folder reported';

function groupUsage(rows: Row[], key: (r: Row) => string): [string, UsageTotals][] {
  const map = new Map<string, UsageTotals>();
  for (const r of rows) map.set(key(r), addUsage(map.get(key(r)) ?? emptyTotals(), r.usage));
  return [...map.entries()];
}
/** Per agent, the one with the most tokens first (agents that reported none last, by name). */
export function usageByAgent(rows: Row[]): AgentUsage[] {
  return groupUsage(rows, (r) => r.agent || NO_AGENT).map(([agent, totals]) => ({ agent, totals }))
    .sort((a, b) => (tokensOf(b.totals) ?? -1) - (tokensOf(a.totals) ?? -1) || a.agent.localeCompare(b.agent));
}
/** Per project (folder name), tokens first. */
export function usageByProject(rows: Row[]): ProjectUsage[] {
  return groupUsage(rows, (r) => r.project || NO_PROJECT).map(([project, totals]) => ({ project, totals }))
    .sort((a, b) => (tokensOf(b.totals) ?? -1) - (tokensOf(a.totals) ?? -1) || a.project.localeCompare(b.project));
}

/** 1234 becomes `1.2k`, 3400000 `3.4M`. The exact number goes in the accessible text. */
export function tokenText(n: number): string {
  if (n < 1000) return String(n);
  const r = (v: number) => (v >= 10 ? Math.round(v) : Math.round(v * 10) / 10);
  // 999_999 rounds to 1000k, which is 1M.
  const k = r(n / 1000);
  return n < 1_000_000 && k < 1000 ? `${k}k` : `${r(n / 1_000_000)}M`;
}
export const exact = (n: number) => n.toLocaleString('en');
/** An amount the agent reported, in its own currency. */
export function costText(micros: number, currency: string, locale?: string): string {
  const v = micros / 1e6;
  try {
    // A currency without minor units (JPY, KRW) keeps none.
    const digits = new Intl.NumberFormat(locale, { style: 'currency', currency }).resolvedOptions().maximumFractionDigits ?? 2;
    return new Intl.NumberFormat(locale, { style: 'currency', currency, maximumFractionDigits: digits === 0 ? 0 : v < 1 ? 4 : 2, minimumFractionDigits: digits === 0 ? 0 : 2 }).format(v);
  } catch { return `${v.toFixed(2)} ${currency}`; }
}
export const costsText = (cost: Record<string, number>, locale?: string) => Object.entries(cost).map(([c, m]) => costText(m, c, locale)).join(' + ');

/** What the agent would have to do for a figure to appear. Says only what the recordings and docs of the repo back (fact 0029). */
export type Missing = 'tokens' | 'cost';
export function whyMissing(agent: string, what: Missing): string {
  const a = agent === NO_AGENT ? 'this agent' : agent;
  if (agent === 'claude') {
    if (what === 'tokens') return 'Claude Code reports no token counts in its hooks. Termenbox reads them from Claude Code\'s own transcript when hooks are on (Settings, Agent hooks) and a task runs.';
    return 'Claude Code gives hooks no cost. The status-line forwarder (roadmap R19, not built yet) would report it.';
  }
  if (agent === 'codex') {
    if (what === 'tokens') return 'Codex\'s notify hook carries no usage. A Codex workspace started from Termenbox over ACP reports tokens for each turn.';
    return 'Codex reports no cost to Termenbox.';
  }
  if (what === 'tokens') return `${a} reported no token counts. Only hooks of Claude Code and ACP agents that report usage supply them.`;
  return `${a} reported no cost to Termenbox.`;
}
/** The window indicator is a count of rate-limit hits: no agent gives Termenbox a window's length or reset time today. */
export const WINDOW_NOTE = 'Rate-limit windows (when a limit resets) are not available for any agent: none passes them to Termenbox yet.';

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

export interface Bucket { label: string; finished: number; failed: number; blockedMs: number; /** Input plus output tokens of tasks that reported them. */ tokens: number; /** Start of the hour or day. */ start: number }
/** Hours of the day in the day view, days in the week view. */
export function bucketsOf(rows: Row[], mode: Mode, r: Range, locale?: string): Bucket[] {
  const days = daysOf(r);
  const buckets: Bucket[] = mode === 'day'
    ? Array.from({ length: 24 }, (_, h) => ({ label: h % 6 === 0 ? String(h).padStart(2, '0') : '', finished: 0, failed: 0, blockedMs: 0, tokens: 0, start: hourOf(r.from, h) }))
    : days.map((d) => ({ label: fmt(d, { weekday: 'short' }, locale), finished: 0, failed: 0, blockedMs: 0, tokens: 0, start: d }));
  const at = (t: number): Bucket | undefined =>
    t < r.from || t >= r.to ? undefined : mode === 'day' ? buckets[new Date(t).getHours()] : buckets[days.indexOf(startOfDay(t))];
  for (const row of rows) {
    // A task still running counts where it began, or on the first day shown when it began before.
    const b = at(row.endedAt ?? Math.max(row.startedAt, r.from));
    if (!b) continue;
    if (row.outcome === 'finished') b.finished++;
    else if (row.outcome === 'failed') b.failed++;
    b.blockedMs += row.blockedMs;
    b.tokens += tokensOf(row.usage ?? {}) ?? 0;
  }
  return buckets;
}

/** One bar of a chart: its value as text, its parts, and what a screen reader says for it (empty: nothing to say). */
export interface Bar { label: string; value: string; parts: { n: number; tone: 'ok' | 'bad' | 'warn' | 'accent' }[]; spoken: string }
const at = (b: Bucket, mode: Mode, locale?: string) =>
  mode === 'day' ? `${new Date(b.start).toLocaleTimeString(locale, { hour: 'numeric' })}` : fmt(b.start, { weekday: 'long' }, locale);

/** Which bars write their value above them. Bars that would not leave room for the text of their neighbours give way:
 *  a value stays only when no taller bar (or an earlier one as tall) with a value is within `radius` places. */
export function shownValues(bars: Bar[], radius: number): boolean[] {
  const total = (b: Bar) => b.parts.reduce((t, p) => t + p.n, 0);
  return bars.map((b, i) => !!b.value && !bars.some((o, j) => j !== i && o.value && Math.abs(j - i) <= radius && (total(o) > total(b) || (total(o) === total(b) && j < i))));
}

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

export function tokenBars(buckets: Bucket[], mode: Mode, locale?: string): Bar[] {
  return buckets.map((b) => ({
    label: b.label, value: b.tokens ? tokenText(b.tokens) : '', parts: [{ n: b.tokens, tone: 'accent' }],
    spoken: b.tokens ? `${at(b, mode, locale)} ${exact(b.tokens)} tokens` : '',
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
export const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
/** What the user is told after a deletion: what went where, then which servers did not confirm (an unconfirmed server may still hold its history). */
export function deletionSummary(d: HistoryDeletion): string {
  const gone = d.servers ? `Removed ${plural(d.removed, 'task', 'tasks')} from ${plural(d.servers, 'server', 'servers')}.` : '';
  if (!d.failedNames.length) return gone ? `Done. ${gone}` : 'Done.';
  const miss = `${d.failedNames.join(', ')} did not confirm in time, so history may remain there. A server older than this feature keeps none and never answers.`;
  return gone ? `${gone} ${miss}` : miss;
}
/** Asks each server to forget its history and waits for every `history_deleted`, up to a limit. */
export class DeleteBook {
  private waiting = new Map<string, (removed: number) => void>();
  private inFlight?: Promise<DeleteResult>;
  /** A second call while one is waiting joins it: the servers are asked once and both callers get the one answer. */
  run(ids: string[], send: (id: string) => void, timeoutMs = DELETE_TIMEOUT_MS): Promise<DeleteResult> {
    if (this.inFlight) return this.inFlight;
    const p = new Promise<DeleteResult>((resolve) => {
      let removed = 0;
      const pending = new Set(ids);
      const finish = () => { clearTimeout(timer); for (const id of ids) this.waiting.delete(id); this.inFlight = undefined; resolve({ removed, failed: [...pending] }); };
      const timer = setTimeout(finish, timeoutMs);
      if (!ids.length) return finish();
      for (const id of ids) {
        this.waiting.set(id, (n) => { removed += n; pending.delete(id); if (!pending.size) finish(); });
        send(id);
      }
    });
    // An empty list finishes before `inFlight` is set below; only a still-waiting call is remembered.
    if (this.waiting.size) this.inFlight = p;
    return p;
  }
  reply(serverId: string, removed: number) {
    const done = this.waiting.get(serverId);
    this.waiting.delete(serverId);
    done?.(removed);
  }
}
