import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import ActivityView from './ActivityView.svelte';
import type { HubServer } from '../lib/hub';
import { ANSWER_TIMEOUT_MS, rangeOf, startOfDay, type HistoryMap } from '../lib/history';
import { answer, NOW, RUNS } from '../lib/historyFixture';

const server = (id: string, state = 'connected') => ({ id, state, server: { server_name: id } }) as unknown as HubServer;
const range = rangeOf('day', startOfDay(NOW));
const entry = (sentAt: number, a?: ReturnType<typeof answer>) => ({ ...range, sentAt, answer: a });
const html = (servers: HubServer[], history: HistoryMap, now = NOW) => render(ActivityView, { props: { servers, history, now, onask: () => {} } }).body;

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(NOW); });
afterEach(() => vi.useRealTimers());

describe('Activity from a fixture', () => {
  let out = '';
  beforeEach(() => { out = html([server('alpha')], { alpha: entry(NOW - 1000, answer(RUNS)) }); });

  it('shows the daily digest: finished, failed and the time agents waited on the user', () => {
    expect(out).toContain('Daily digest');
    expect(out).toMatch(/data-digest="finished">2</);
    expect(out).toMatch(/data-digest="failed">1</);
    expect(out).toMatch(/data-digest="waited">14m</);
    expect(out).toContain('1 stopped · 1 still running');
    expect(out).toContain('2 finished, 1 failed, 1 stopped, agents waited 14 min on you, 1 still running');
  });

  it('shows the timeline of the day, newest first, every task with its outcome in words', () => {
    const titles = ['Review the deploy script', 'docs', 'Write release notes', 'Migrate the schema', 'Fix the flaky login test'];
    const at = titles.map((t) => out.indexOf(t));
    expect(at.every((i) => i > 0)).toBe(true);
    expect([...at].sort((a, b) => a - b)).toEqual(at);
    for (const w of ['Running', 'Stopped', 'Finished', 'Failed']) expect(out).toContain(w);
    expect(out).toContain('waited 8 min');
    expect(out).toContain('Today');
  });

  it('draws two charts with a spoken summary each, and no colour-only meaning', () => {
    expect(out).toContain('Tasks by hour');
    expect(out).toContain('Waiting on you');
    expect(out).toMatch(/aria-label="Tasks by hour: [^"]*1 finished; [^"]*1 failed/);
    expect(out).toMatch(/aria-label="Time agents waited on you: [^"]*8 min/);
    expect(out).toContain('repeating-linear-gradient');
  });

  it('puts the keyboard and screen-reader basics in place', () => {
    expect(out).toMatch(/<h1[^>]*>Activity<\/h1>/);
    expect(out).toContain('aria-label="Previous day"');
    expect(out).toMatch(/aria-label="Next day"[^>]*disabled/);
    expect(out).toContain('role="radiogroup"');
    expect(out).toContain('role="status"');
  });

  it('says which server a task ran on only when there are several', () => {
    expect(out).not.toContain('alpha ·');
    const two = html([server('alpha'), server('beta')], { alpha: entry(NOW, answer(RUNS)), beta: entry(NOW, answer([])) });
    expect(two).toContain('alpha');
  });

  it('says so when the server cut the answer short, and how long history is kept', () => {
    const cut = html([server('alpha')], { alpha: entry(NOW, answer(RUNS, 'day', { truncated: true })) });
    expect(cut).toContain('showing the newest 5');
    expect(out).toContain('for up to 90 days');
    expect(out).toContain('the first line of your prompt, scrubbed');
    expect(out).toContain('never the rest of a prompt, replies or terminal text');
  });
});

describe('Activity says what is going on', () => {
  it('has nothing to show without a connected server', () => {
    const out = html([server('alpha', 'unreachable')], {});
    expect(out).toContain('No server connected');
    expect(out).not.toContain('Daily digest');
  });
  it('loads, then admits a server that does not answer', () => {
    const asked = { alpha: entry(NOW - 1000) };
    expect(html([server('alpha')], asked)).toContain('Loading activity');
    const late = html([server('alpha')], { alpha: entry(NOW - ANSWER_TIMEOUT_MS - 1) }, NOW);
    expect(late).toContain('Activity is not available');
    expect(late).toContain('alpha did not answer');
    expect(late).toContain('Try again');
  });
  it('keeps what answered and names the server that did not', () => {
    const out = html([server('alpha'), server('beta')], { alpha: entry(NOW, answer(RUNS)), beta: entry(NOW - ANSWER_TIMEOUT_MS - 1) });
    expect(out).toContain('Daily digest');
    expect(out).toContain('beta did not answer, so its activity is not included');
  });
  it('is honest when nothing happened, and says how a task gets recorded', () => {
    const out = html([server('alpha')], { alpha: entry(NOW, answer([])) });
    expect(out).toContain('Nothing happened this day');
    expect(out).toContain('turn on hooks in Settings');
    expect(out).not.toContain('Tasks by hour');
    expect(out).toMatch(/data-digest="finished">0</);
  });
  it('ignores an answer that belongs to another range', () => {
    const week = rangeOf('week', startOfDay(NOW));
    const out = html([server('alpha')], { alpha: { ...week, sentAt: NOW - 1000, answer: answer(RUNS, 'week') } });
    expect(out).toContain('Loading activity');
  });
});
