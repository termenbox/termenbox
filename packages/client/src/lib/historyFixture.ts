// A Wednesday on one server: the same fixture feeds the unit tests, the rendered-view test and the browser probe, so they
// agree on what the view must show.
import type { HistoryMessage, HistoryRun } from './protocol';
import { rangeOf, startOfDay } from './history';

/** Local time, October 2026. */
export const at = (day: number, h: number, m = 0) => new Date(2026, 9, day, h, m).getTime();
export const NOW = at(14, 18, 0);
const min = 60_000;

export const RUNS: HistoryRun[] = [
  { workspace: 'api', agent: 'claude', task: 'Fix the flaky login test', started_at: at(14, 9, 10), ended_at: at(14, 9, 42), outcome: 'finished', blocked_ms: 4 * min,
    project: 'api', usage: { input_tokens: 12000, output_tokens: 3400, cache_read_tokens: 90000, cache_write_tokens: 20000 } },
  { workspace: 'web', agent: 'codex', task: 'Migrate the schema', started_at: at(14, 11, 0), ended_at: at(14, 11, 20), outcome: 'failed', blocked_ms: 0, project: 'web' },
  { workspace: 'api', agent: 'claude', task: 'Write release notes', started_at: at(14, 14, 5), ended_at: at(14, 14, 30), outcome: 'finished', blocked_ms: 8 * min,
    project: 'api', usage: { input_tokens: 800, output_tokens: 1500, cache_read_tokens: 60000, cache_write_tokens: 5000 } },
  { workspace: 'docs', started_at: at(14, 15, 0), ended_at: at(14, 15, 5), outcome: 'stopped', blocked_ms: 0 },
  { workspace: 'api', agent: 'claude', task: 'Review the deploy script', started_at: at(14, 17, 30), blocked_ms: 2 * min,
    project: 'api', usage: { input_tokens: 300, output_tokens: 200, rate_limited: 1 } },
];
/** The day before, for the week view. */
export const EARLIER: HistoryRun[] = [
  { workspace: 'api', agent: 'claude', task: 'Bump dependencies', started_at: at(13, 10, 0), ended_at: at(13, 10, 40), outcome: 'finished', blocked_ms: 0,
    project: 'api', usage: { input_tokens: 2000, output_tokens: 600 } },
];

export function answer(runs: HistoryRun[], mode: 'day' | 'week' = 'day', extra: Partial<HistoryMessage> = {}): HistoryMessage {
  const r = rangeOf(mode, startOfDay(NOW));
  return { type: 'history', from: r.from, to: r.to, runs, truncated: false, keep_days: 90, max_bytes: 20_000_000, ...extra };
}

/** An ACP agent that reported its own cost and tokens (no recorded agent does yet: this is the shape the protocol allows). */
export const REPORTED: HistoryRun = {
  workspace: 'acp', agent: 'kilo', task: 'Sort the imports', started_at: at(14, 12, 0), ended_at: at(14, 12, 10), outcome: 'finished', blocked_ms: 0,
  project: 'web', usage: { input_tokens: 1472, output_tokens: 35, cache_read_tokens: 13008, cost_micros: 250_000, cost_currency: 'USD' },
};
