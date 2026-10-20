import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from 'svelte/server';
import ActivityView from './ActivityView.svelte';
import type { HubServer } from '../lib/hub';
import { ANSWER_TIMEOUT_MS, rangeOf, startOfDay, type HistoryMap } from '../lib/history';
import { answer, NOW, REPORTED, RUNS } from '../lib/historyFixture';
import { healthyHistory, history as healthHistory, live } from '../lib/healthFixture';
import type { HealthMap } from '../lib/health';

const server = (id: string, state = 'connected') => ({ id, state, server: { server_name: id } }) as unknown as HubServer;
const range = rangeOf('day', startOfDay(NOW));
const entry = (sentAt: number, a?: ReturnType<typeof answer>) => ({ ...range, sentAt, answer: a });
const html = (servers: HubServer[], history: HistoryMap, now = NOW) => render(ActivityView, { props: { servers, history, now, onask: () => {} } }).body;

beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(NOW); });
afterEach(() => vi.useRealTimers());

describe('Usage and cost from a fixture', () => {
  let out = '';
  beforeEach(() => { out = html([server('alpha')], { alpha: entry(NOW - 1000, answer([...RUNS, REPORTED])) }); });
  const card = (agent: string) => out.split(`data-usage-agent="${agent}"`)[1].split('</li>')[0];

  it('shows the tokens an agent reported, per agent, with the parts that exist', () => {
    const claude = card('claude');
    expect(claude).toContain('18k tokens');
    expect(claude).toContain('in 13k');
    expect(claude).toContain('out 5.1k');
    expect(claude).toContain('cache read 150k');
    expect(claude).toContain('cache written 25k');
    expect(claude).toContain('input 13,100 tokens, output 5,100 tokens');
    expect(claude).toContain('3 tasks');
  });

  it('shows a cost only where the agent reported one, with whose it is', () => {
    const kilo = card('kilo');
    expect(kilo).toContain('$0.25');
    expect(kilo).toContain('reported by kilo');
    expect(kilo).not.toContain('cache written');
  });

  it('says plainly what is not available, and what would enable it, instead of zeros', () => {
    const claude = card('claude');
    expect(claude).toContain('Cost: not available from this agent');
    expect(claude).toContain('status-line forwarder (roadmap R19, not built yet)');
    expect(claude).not.toContain('Rate-limit window');
    const codex = card('codex');
    expect(codex).toContain('Tokens: not available from this agent');
    expect(codex).toContain('notify hook carries no usage');
    expect(codex).toContain('Cost: not available from this agent');
    expect(codex).not.toMatch(/\b0 tokens|\$0\.00/);
    expect(card('Unnamed agent')).toContain('Tokens: not available from this agent');
  });

  it('counts rate-limit hits where the agent said so, and says no window is known', () => {
    expect(card('claude')).toContain('Rate limited once');
    expect(card('kilo')).not.toContain('Rate limited');
    expect(out).toContain('Rate-limit windows (when a limit resets) are not available');
    expect(out.match(/Rate-limit windows/g)).toHaveLength(1);
    expect(out).toContain('Claude Code counts are a lower bound');
  });

  it('ranks projects, and keeps tasks with no folder in their own row', () => {
    const projects = out.split('data-usage-projects')[1].split('</ul>')[0];
    expect(projects.indexOf('data-usage-project="api"')).toBeLessThan(projects.indexOf('data-usage-project="web"'));
    expect(projects).toContain('data-usage-project="No folder reported"');
    expect(projects).toMatch(/data-usage-project="api"[^]*18k tokens/);
    expect(projects).toMatch(/data-usage-project="web"[^]*1\.5k tokens[^]*\$0\.25/);
  });

  it('draws the token chart with a spoken summary, and puts tokens on the timeline rows', () => {
    expect(out).toContain('Tokens by hour');
    expect(out).toMatch(/aria-label="Tokens by hour: [^"]*15,400 tokens/);
    expect(out).toContain('1.5k tokens');
  });

  it('shows the warm prompt cache as an estimate only when a session has one', () => {
    expect(out).not.toContain('Prompt cache now');
    const warm = render(ActivityView, { props: { servers: [server('alpha')], history: { alpha: entry(NOW, answer(RUNS)) }, now: NOW, onask: () => {}, ttlMs: 300_000, caches: [{ key: 'alpha/api', name: 'api', server: 'alpha', remainingMs: 150_000 }] } }).body;
    expect(warm).toContain('Prompt cache now');
    expect(warm).toContain('about 3 min');
    expect(warm).toContain('5-minute cache time');
  });

  it('shows no usage numbers at all when no agent reported any, only the honest state', () => {
    const bare = html([server('alpha')], { alpha: entry(NOW, answer(RUNS.map(({ usage: _u, ...r }) => r))) });
    expect(bare).not.toContain('Tokens by hour');
    expect(bare).not.toContain('data-usage-tokens');
    expect(bare).not.toContain('data-usage-projects');
    expect(bare).toContain('Tokens: not available from this agent');
  });

  it('reads unexpected values defensively: negative, text and bad currencies are dropped', () => {
    const odd = html([server('alpha')], { alpha: entry(NOW, answer([{ ...RUNS[0], usage: { input_tokens: -5, output_tokens: 'x' as unknown as number, cost_micros: 5, cost_currency: '<b>' } }])) });
    expect(odd).not.toContain('data-usage-tokens');
    expect(odd).not.toContain('data-usage-cost');
    expect(odd).not.toContain('<b>');
  });
});

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

describe('Server health from a fixture (U14)', () => {
  const srv = (id = 'alpha', h?: ReturnType<typeof live>) => ({ ...server(id), health: h }) as unknown as HubServer;
  const view = (servers: HubServer[], health: HealthMap, now = NOW, rtt: number | null = 41) =>
    render(ActivityView, { props: { servers, history: {}, now, onask: () => {}, health, rttOf: () => rtt } }).body;

  it('says an unhealthy server is unhealthy in words, names the metric and explains the levels', () => {
    const out = view([srv('alpha', live())], { alpha: { sentAt: NOW, answer: healthHistory() } });
    expect(out).toContain('data-activity-health');
    expect(out).toContain('data-health-verdict="unhealthy"');
    expect(out).toContain('RAM is over its level, held for 30 seconds');
    expect(out).toContain('Level 90%, healthy again at 80% or less');
    expect(out).toContain("of the container's memory limit");
  });
  it('draws three charts, each with a spoken summary and a dashed level line, not colour alone', () => {
    const out = view([srv()], { alpha: { sentAt: NOW, answer: healthHistory() } });
    expect(out.match(/data-health-chart/g)).toHaveLength(3);
    const labels = [...out.matchAll(/role="img" aria-label="([^"]+)"/g)].map((m) => m[1]);
    expect(labels).toHaveLength(3);
    expect(labels[0]).toMatch(/^CPU over the last hours: now \d+%, highest 96% at \d\d:\d\d( [AP]M)?, at or above its 90% level for about 5 min/);
    expect(labels[2]).toContain('never at its 90% level');
    expect(out).toContain('stroke-dasharray');
    expect(view([srv()], { alpha: { sentAt: NOW, answer: healthyHistory() } })).toContain('data-health-verdict="healthy"');
  });
  it('shows this device\'s round trip, and that it is not measured yet', () => {
    const h = { alpha: { sentAt: NOW, answer: healthyHistory() } };
    expect(view([srv()], h, NOW, 41)).toContain('41 ms');
    expect(view([srv()], h, NOW, null)).toContain('not measured yet');
  });
  it('is honest while loading, for an older server that never answers, and for a server that just started', () => {
    expect(view([srv()], { alpha: { sentAt: NOW } })).toContain('Loading health');
    const old = view([srv()], { alpha: { sentAt: NOW - ANSWER_TIMEOUT_MS - 1 } });
    expect(old).toContain('data-health-state="unavailable"');
    expect(old).toContain('did not report its health');
    expect(old).toContain('Try again');
    const fresh = view([srv()], { alpha: { sentAt: NOW, answer: healthHistory({ samples: [] }) } });
    expect(fresh).toContain('data-health-state="empty"');
    expect(fresh).toContain('has just started');
  });
  it('says plainly when a metric could not be read', () => {
    const samples = healthyHistory().samples.map((p) => ({ ts: p.ts, ram_percent: p.ram_percent }));
    const out = view([srv()], { alpha: { sentAt: NOW, answer: healthHistory({ healthy: true, unhealthy: [], samples }) } });
    expect(out).toContain('not available');
    expect(out.match(/data-health-chart/g)).toHaveLength(1);
  });
  it('shows the health of every connected server, each under its own name', () => {
    const out = view([srv('alpha'), srv('beta')], { alpha: { sentAt: NOW, answer: healthyHistory() }, beta: { sentAt: NOW, answer: healthHistory() } });
    expect(out.match(/data-health-server=/g)).toHaveLength(2);
    expect(out.split('data-health-server="beta"')[1]).toContain('data-health-verdict="unhealthy"');
  });
});
