import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  addDays, answered, asked, ANSWER_TIMEOUT_MS, bucketsOf, compact, DeleteBook, digestOf, digestSentence, duration, groupByDay, overall, rangeLabel, rangeOf, rowsOf,
  startOfDay, statusOf, step, taskBars, waitBars, type Source,
} from './history';
import { answer, at, EARLIER, NOW, RUNS } from './historyFixture';

const src = (runs = RUNS): Source[] => [{ serverId: 'alpha', serverName: 'alpha', runs }];

describe('ranges are the user\'s local days', () => {
  it('a day is midnight to midnight, a week is the 7 days ending on the anchor', () => {
    const today = startOfDay(NOW);
    expect(rangeOf('day', today)).toEqual({ from: at(14, 0), to: at(15, 0) });
    expect(rangeOf('week', today)).toEqual({ from: at(8, 0), to: at(15, 0) });
  });
  it('stepping moves a day or a week and never passes today', () => {
    const today = startOfDay(NOW);
    expect(step('day', today, -1, NOW)).toBe(at(13, 0));
    expect(step('week', today, -1, NOW)).toBe(at(7, 0));
    expect(step('day', today, 1, NOW)).toBe(today);
    expect(step('week', at(10, 0), 1, NOW)).toBe(today);
  });
  it('labels say Today, Yesterday and the week', () => {
    expect(rangeLabel('day', rangeOf('day', startOfDay(NOW)), NOW, 'en-GB')).toBe('Today');
    expect(rangeLabel('day', rangeOf('day', at(13, 0)), NOW, 'en-GB')).toBe('Yesterday');
    expect(rangeLabel('week', rangeOf('week', startOfDay(NOW)), NOW, 'en-GB')).toBe('8 Oct to 14 Oct');
  });
  it('addDays stays on local midnight', () => {
    expect(new Date(addDays(at(31, 0), 1)).getHours()).toBe(0);
  });
});

describe('durations', () => {
  it('say minutes and hours in words and compactly', () => {
    expect(duration(0)).toBe('0 min');
    expect(duration(10_000)).toBe('under 1 min');
    expect(duration(4 * 60_000)).toBe('4 min');
    expect(duration(65 * 60_000)).toBe('1 h 5 min');
    expect(duration(120 * 60_000)).toBe('2 h');
    expect([compact(0), compact(20_000), compact(14 * 60_000), compact(65 * 60_000), compact(60 * 60_000)]).toEqual(['0m', '<1m', '14m', '1h 5m', '1h']);
  });
});

describe('the digest and the timeline', () => {
  const rows = rowsOf(src(), NOW);
  it('counts what finished, failed, stopped and runs, and sums what agents waited on the user', () => {
    expect(digestOf(rows)).toEqual({ finished: 2, failed: 1, stopped: 1, running: 1, blockedMs: 14 * 60_000 });
    expect(digestSentence(digestOf(rows))).toBe('2 finished, 1 failed, 1 stopped, agents waited 14 min on you, 1 still running');
  });
  it('lists the running task first, then the newest ended', () => {
    expect(rows.map((r) => r.outcome)).toEqual(['running', 'stopped', 'finished', 'failed', 'finished']);
    expect(rows[0].durationMs).toBe(30 * 60_000);
  });
  it('reads an unknown outcome as stopped and ignores nonsense numbers', () => {
    const [r] = rowsOf(src([{ session: 's', started_at: 5, ended_at: 3, outcome: 'exploded', blocked_ms: -4 }]), NOW);
    expect([r.outcome, r.durationMs, r.blockedMs]).toEqual(['stopped', 0, 0]);
  });
  it('groups by the day a task ended, newest day first', () => {
    const week = rowsOf(src([...RUNS, ...EARLIER]), NOW);
    const g = groupByDay(week, rangeOf('week', startOfDay(NOW)));
    expect(g.map((x) => [x.day, x.rows.length])).toEqual([[at(14, 0), 5], [at(13, 0), 1]]);
  });
  it('puts a task that started before the range on the first day shown', () => {
    const r = rangeOf('day', startOfDay(NOW));
    const rows = rowsOf(src([{ session: 's', started_at: at(13, 22), blocked_ms: 0 }]), NOW);
    expect(groupByDay(rows, r).map((g) => g.day)).toEqual([r.from]);
  });
});

describe('chart buckets', () => {
  const rows = rowsOf(src([...RUNS, ...EARLIER]), NOW);
  it('the day view has 24 hours and counts a task in the hour it ended', () => {
    const b = bucketsOf(rows, 'day', rangeOf('day', startOfDay(NOW)));
    expect(b).toHaveLength(24);
    expect(b.filter((x) => x.finished + x.failed > 0).map((x) => [new Date(x.start).getHours(), x.finished, x.failed])).toEqual([[9, 1, 0], [11, 0, 1], [14, 1, 0]]);
    expect(b.map((x) => x.label).filter(Boolean)).toEqual(['00', '06', '12', '18']);
  });
  it('the week view has 7 days with their counts and waiting time', () => {
    const b = bucketsOf(rows, 'week', rangeOf('week', startOfDay(NOW)), 'en-GB');
    expect(b).toHaveLength(7);
    expect(b.map((x) => [x.finished, x.failed, x.blockedMs / 60_000])).toEqual([[0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [1, 0, 0], [2, 1, 14]]);
    expect(b[6].label).toBe('Wed');
  });
  it('bars say their value in text and for a screen reader; an empty bar says nothing', () => {
    const b = bucketsOf(rows, 'week', rangeOf('week', startOfDay(NOW)), 'en-GB');
    const tasks = taskBars(b, 'week', 'en-GB');
    expect(tasks[6]).toMatchObject({ value: '3', spoken: 'Wednesday 2 finished, 1 failed' });
    expect(tasks[0]).toMatchObject({ value: '', spoken: '' });
    expect(waitBars(b, 'week', 'en-GB')[6]).toMatchObject({ value: '14m', spoken: 'Wednesday 14 min' });
  });
});

describe('asking servers', () => {
  const r = rangeOf('day', startOfDay(NOW));
  it('a server is loading until it answers, and unavailable if it never does', () => {
    const m = asked({}, 'alpha', r, 1000);
    expect(statusOf(m.alpha, 1000 + ANSWER_TIMEOUT_MS - 1)).toBe('loading');
    expect(statusOf(m.alpha, 1000 + ANSWER_TIMEOUT_MS)).toBe('unavailable');
    expect(statusOf(undefined, 1000)).toBe('loading');
  });
  it('an answer for the range asked is kept; a late one for another range is dropped', () => {
    const m = asked({}, 'alpha', r, 1000);
    expect(statusOf(answered(m, 'alpha', answer(RUNS)).alpha, 5000)).toBe('ready');
    const week = answer(RUNS, 'week');
    expect(answered(m, 'alpha', week)).toBe(m);
    expect(answered({}, 'alpha', answer(RUNS))).toEqual({});
  });
  it('asking again for the same range keeps the answer and the clock; a new range starts over', () => {
    let m = answered(asked({}, 'alpha', r, 1000), 'alpha', answer(RUNS));
    m = asked(m, 'alpha', r, 9000);
    expect(m.alpha.answer).toBeDefined();
    expect(m.alpha.sentAt).toBe(1000);
    m = asked(m, 'alpha', rangeOf('week', startOfDay(NOW)), 9000);
    expect(m.alpha.answer).toBeUndefined();
    expect(m.alpha.sentAt).toBe(9000);
  });
  it('the view is ready as soon as one server answered, and unavailable only when every one failed', () => {
    expect(overall(['ready', 'loading', 'unavailable'])).toBe('ready');
    expect(overall(['loading', 'unavailable'])).toBe('loading');
    expect(overall(['unavailable', 'unavailable'])).toBe('unavailable');
    expect(overall([])).toBe('loading');
  });
});

describe('deleting history', () => {
  afterEach(() => vi.useRealTimers());
  it('resolves when every server confirmed, with the total removed', async () => {
    const book = new DeleteBook();
    const sent: string[] = [];
    const p = book.run(['a', 'b'], (id) => sent.push(id));
    expect(sent).toEqual(['a', 'b']);
    book.reply('a', 3);
    book.reply('b', 4);
    expect(await p).toEqual({ removed: 7, failed: [] });
  });
  it('names the servers that did not confirm in time', async () => {
    vi.useFakeTimers();
    const book = new DeleteBook();
    const p = book.run(['a', 'b'], () => {}, 1000);
    book.reply('a', 2);
    vi.advanceTimersByTime(1000);
    expect(await p).toEqual({ removed: 2, failed: ['b'] });
    book.reply('b', 5); // too late: nothing happens
  });
  it('resolves at once when no server is connected', async () => {
    expect(await new DeleteBook().run([], () => {})).toEqual({ removed: 0, failed: [] });
  });
});
