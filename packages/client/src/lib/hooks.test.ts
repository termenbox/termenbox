import { describe, expect, it } from 'vitest';
import type { AgentMap, WorkspaceAgent } from './agents';
import type { HubServer, HubWorkspace } from './hub';
import { HookOffers } from './hookOffer';
import { applyHookReply, hookQueries, hookRows, markBusy, pendingOffer, waitingOnPrompt, type HookMap } from './hooks';

const server = (id: string, state = 'connected'): HubServer => ({ id, state, server: { server_name: id.toUpperCase() } }) as unknown as HubServer;
const sess = (serverId: string, id: string): HubWorkspace => ({ id, name: id, agent: null, serverId, serverName: serverId, key: `${serverId}/${id}` });
const agentOf = (agent: string, extra: Partial<WorkspaceAgent> = {}): WorkspaceAgent => ({ state: 'idle', seq: 1, stale: false, ts: 1, agent, ...extra });
const memory = () => { const m = new Map<string, string>(); return { getItem: (k: string) => m.get(k) ?? null, setItem: (k: string, v: string) => void m.set(k, v) }; };
const reply = (state: string, extra = {}) => ({ type: 'agent_hooks' as const, agent: 'claude', action: 'preview', state, ...extra });

describe('server hook state', () => {
  it('asks for the state and the exact change when a server connects', () => {
    expect(hookQueries()).toEqual([
      { type: 'agent_hooks', agent: 'claude', action: 'preview' },
      { type: 'agent_hooks', agent: 'opencode', action: 'preview' },
    ]);
  });
  it('keeps the latest answer per server and agent, and reads an unknown state as unavailable', () => {
    let m: HookMap = applyHookReply({}, 's1', reply('not_connected', { preview: '{}' }));
    expect(m.s1.claude).toEqual({ state: 'not_connected', preview: '{}', error: undefined });
    m = applyHookReply(m, 's1', reply('connected', { action: 'apply' }));
    expect(m.s1.claude.state).toBe('connected');
    expect(m.s1.claude.preview).toBeUndefined();
    expect(applyHookReply({}, 's2', reply('from-the-future')).s2.claude.state).toBe('unavailable');
  });
  it('marks a change as busy and clears the busy mark with the answer', () => {
    let m = applyHookReply({}, 's1', reply('not_connected', { preview: 'x' }));
    m = markBusy(m, 's1', 'claude', 'apply');
    expect(m.s1.claude.busy).toBe('apply');
    expect(applyHookReply(m, 's1', reply('connected')).s1.claude.busy).toBeUndefined();
  });
});

describe('the offer on Home', () => {
  const servers = [server('s1')];
  const workspaces = [sess('s1', 'a')];
  const hooks: HookMap = { s1: { claude: { state: 'not_connected', preview: '{}' } } };
  const agents: AgentMap = { 's1/a': agentOf('claude') };

  it('shows on the first detection of Claude Code with hooks off', () => {
    const o = pendingOffer(servers, workspaces, agents, hooks, new HookOffers(memory(), () => 0));
    expect(o?.agent).toBe('claude');
    expect(o?.label).toBe('Claude Code');
    expect(o?.serverName).toBe('S1');
  });
  it('shows OpenCode with its own label', () => {
    const agents: AgentMap = { 's1/a': agentOf('opencode') };
    const hooks: HookMap = { s1: { opencode: { state: 'not_connected', preview: 'plugin' } } };
    const o = pendingOffer(servers, workspaces, agents, hooks, new HookOffers(memory(), () => 0));
    expect(o?.agent).toBe('opencode');
    expect(o?.label).toBe('OpenCode');
  });
  it('does not show without a detected agent, for another agent, with hooks on, or on a server that is not connected', () => {
    const offers = new HookOffers(memory(), () => 0);
    expect(pendingOffer(servers, workspaces, {}, hooks, offers)).toBeUndefined();
    expect(pendingOffer(servers, workspaces, { 's1/a': agentOf('codex') }, hooks, offers)).toBeUndefined();
    expect(pendingOffer(servers, workspaces, agents, { s1: { claude: { state: 'connected' } } }, offers)).toBeUndefined();
    expect(pendingOffer(servers, workspaces, agents, { s1: { claude: { state: 'unsupported' } } }, offers)).toBeUndefined();
    expect(pendingOffer([server('s1', 'reconnecting')], workspaces, agents, hooks, offers)).toBeUndefined();
    expect(pendingOffer(servers, workspaces, agents, {}, offers)).toBeUndefined(); // an older server never answers
  });
  it('stops after Not now and Never, per server', () => {
    const offers = new HookOffers(memory(), () => 0);
    offers.notNow('s1', 'claude');
    expect(pendingOffer(servers, workspaces, agents, hooks, offers)).toBeUndefined();
    offers.manualAnswer('s1', 'claude');
    expect(pendingOffer(servers, workspaces, agents, hooks, offers)).toBeDefined();
    offers.never('s1', 'claude');
    expect(pendingOffer(servers, workspaces, agents, hooks, offers)).toBeUndefined();
  });
  it('treats the agent waiting on a prompt as the user answering by hand', () => {
    expect(waitingOnPrompt(agentOf('claude', { state: 'needs_input' }))).toBe(true);
    expect(waitingOnPrompt(agentOf('claude'))).toBe(false);
    expect(waitingOnPrompt(undefined)).toBe(false);
  });
});

describe('the hook rows in Settings', () => {
  it('lists every connected server with a hook surface, on or off, with the Never mark', () => {
    const offers = new HookOffers(memory(), () => 0);
    offers.never('s2', 'claude');
    const hooks: HookMap = { s1: { claude: { state: 'connected' } }, s2: { claude: { state: 'not_connected' } }, s3: { claude: { state: 'unavailable' } } };
    const rows = hookRows([server('s1'), server('s2'), server('s3'), server('s4')], hooks, offers);
    expect(rows.map((r) => [r.serverId, r.info.state, r.never])).toEqual([['s1', 'connected', false], ['s2', 'not_connected', true]]);
  });
});
