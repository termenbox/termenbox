import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Home from './Home.svelte';
import type { AgentMap, SessionAgent } from './lib/agents';
import type { HubServer, HubSession } from './lib/hub';
import type { AttentionKind } from './lib/protocol';
import { live } from './lib/healthFixture';

// Two servers, a waiting decision on alpha (the hero) and a failed task on each: the dot follows the server's own verdict.
const NOW = 1_800_000_000_000;
const server = (id: string, health?: ReturnType<typeof live>) => ({ id, state: 'connected', server: { server_name: id }, health }) as unknown as HubServer;
const sess = (serverId: string, name: string): HubSession => ({ id: name, name, agent: null, serverId, serverName: serverId, key: `${serverId}/${name}` });
const att = (kind: AttentionKind, ageMin: number): SessionAgent => ({
  state: 'unknown', seq: 1, stale: false, ts: NOW, serverStatus: true, agent: 'claude',
  attention: { id: `a${ageMin}`, session: 'x', kind, raised_at: NOW - ageMin * 60_000, task: { title: 'A task', started_at: NOW - 3_600_000, source: 'hook' }, prompt: kind === 'needs_input' ? 'Run it?' : undefined },
  ...(kind === 'needs_input' ? { prompt: { id: 'p', kind: 'choice', choices: ['Yes', 'No'], seq: 7, confidence: 'high' }, tool: { name: 'Bash', summary: 'pnpm test', phase: 'started' } } : {}),
});
const sessions = [sess('alpha', 'ask'), sess('alpha', 'broken'), sess('beta', 'broken-too')];
const agents: AgentMap = { 'alpha/ask': att('needs_input', 30), 'alpha/broken': att('failed', 20), 'beta/broken-too': att('failed', 10) };
const html = (servers: HubServer[]) => render(Home, { props: { sessions, agents, servers, now: NOW, lastKey: null, onopen: () => {}, onanswer: () => {}, onreply: () => {}, onservers: () => {}, onnew: () => {} } }).body;
const dots = (out: string) => out.match(/data-server-unhealthy/g)?.length ?? 0;

describe('Home shows the server\'s own verdict as a dot with words', () => {
  it('marks the hero and the rows of an unhealthy server, and only those', () => {
    const out = html([server('alpha', live()), server('beta', live({ healthy: true, unhealthy: [] }))]);
    // The hero (alpha's decision) and alpha's failed row; beta's row has none.
    expect(dots(out)).toBe(2);
    expect(out).toContain('Server unhealthy');
    const betaRow = out.split('broken-too')[1].split('</button>')[0];
    expect(betaRow).not.toContain('data-server-unhealthy');
    const hero = out.split('aria-label="Decision"')[1].split('<h2')[0];
    expect(hero).toContain('data-server-unhealthy');
  });
  it('keeps the dot out of the accessibility tree: the words say it, the dot is decoration', () => {
    const out = html([server('alpha', live()), server('beta')]);
    const dot = out.match(/<small[^>]*data-server-unhealthy[^>]*>.*?<\/small>/)![0];
    expect(dot).toMatch(/<span class="[^"]*bg-bad[^"]*" aria-hidden="true">/);
    expect(dot).not.toContain('aria-label');
    expect(dot).toContain('text-bad'); // a design-system token, not a raw colour
    expect(dot).not.toMatch(/red-\d/);
  });
  it('shows no dot for a healthy server, a server with no verdict yet, or an older server that sends none', () => {
    expect(dots(html([server('alpha', live({ healthy: true, unhealthy: [] })), server('beta')]))).toBe(0);
    expect(dots(html([server('alpha'), server('beta')]))).toBe(0);
  });
});
