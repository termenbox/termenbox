import { describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { agents, applyActivity, applyAttention, applyEvents, applySnapshot, attentionToAck, handleAgentMessage, isStale, resetAgents, runningMs, syncRequest, type AgentMap } from './agents';
import { needsYou } from './tabs';
import type { AgentEvent, ClientMessage, ServerMessage } from './protocol';

const ev = (seq: number, rest: object, workspace = 's'): AgentEvent => ({ workspace: workspace, seq, ts: 1000 + seq, source: 'hook', confidence: 'high', ...rest }) as AgentEvent;

describe('agent events', () => {
  it('follows state, tool, prompt and message events in order', () => {
    const r = applyEvents({}, [
      ev(1, { kind: 'state', state: 'working' }),
      ev(2, { kind: 'tool', name: 'Bash', summary: 'ls', phase: 'started' }),
      ev(3, { kind: 'state', state: 'needs_input' }),
      ev(4, { kind: 'prompt', id: 'p1', prompt_kind: 'yes_no', choices: ['yes', 'no'] }),
    ]);
    expect(r.gap).toBe(false);
    expect(r.map.s.state).toBe('needs_input');
    expect(r.map.s.tool?.name).toBe('Bash');
    expect(r.map.s.prompt?.choices).toEqual(['yes', 'no']);
    const done = applyEvents(r.map, [ev(5, { kind: 'state', state: 'done' }), ev(6, { kind: 'message', summary: 'ok' })]);
    expect(done.map.s.prompt).toBeUndefined();
    expect(done.map.s.message).toBe('ok');
  });

  it('tracks the continuous human wait, clears it on exit, and resets it after server restart', () => {
    let map = applyEvents({}, [ev(1, { kind: 'state', state: 'needs_input', ts: 10 }), ev(2, { kind: 'state', state: 'error', ts: 20 })]).map;
    expect(map.s.waitingSince).toBe(10);
    map = applyEvents(map, [ev(3, { kind: 'state', state: 'working', ts: 30 })]).map;
    expect(map.s.waitingSince).toBeUndefined();
    map = applyEvents(map, [ev(4, { kind: 'state', state: 'needs_input', ts: 40 })]).map;
    expect(map.s.waitingSince).toBe(40);
    expect(applySnapshot(map, [{ workspace: 's', seq: 1, ts: 50, state: 'needs_input', stale: false }]).s.waitingSince).toBe(50);
    expect(applySnapshot(map, [{ workspace: 's', seq: 5, ts: 60, state: 'working', stale: false }]).s.waitingSince).toBeUndefined();
  });
  it('ignores kinds and fields it does not know', () => {
    const r = applyEvents({}, [ev(1, { kind: 'hologram', x: 1 }), ev(2, { kind: 'state', state: 'working', extra: true })]);
    expect(r.map.s.state).toBe('working');
    expect(r.gap).toBe(false); // an unknown kind still uses its seq
  });

  it('skips duplicates and flags a gap in seq', () => {
    const base = applyEvents({}, [ev(1, { kind: 'state', state: 'idle' })]).map;
    expect(applyEvents(base, [ev(1, { kind: 'state', state: 'working' })]).map.s.state).toBe('idle');
    const r = applyEvents(base, [ev(3, { kind: 'state', state: 'working' })]);
    expect(r.gap).toBe(true);
  });

  it('recovers from a gap with a snapshot and asks only for what is newer', () => {
    let map: AgentMap = applyEvents({}, [ev(1, { kind: 'state', state: 'idle' })]).map;
    expect(syncRequest(map)).toEqual({ s: 1 });
    map = applySnapshot(map, [{ workspace: 's', state: 'working', seq: 7, stale: false, ts: 9000 }, { workspace: 't', state: 'idle', seq: 2, stale: false, ts: 1 }]);
    expect(map.s.seq).toBe(7);
    expect(map.s.state).toBe('working');
    expect(applyEvents(map, [ev(8, { kind: 'state', state: 'done' })]).gap).toBe(false);
  });

  it('drops workspaces the snapshot no longer lists and survives a server counter reset', () => {
    const map = applyEvents({}, [ev(1, { kind: 'state', state: 'idle' }), ev(1, { kind: 'state', state: 'idle' }, 'gone')]).map;
    const snap = applySnapshot(map, [{ workspace: 's', state: 'idle', seq: 1, stale: false, ts: 1 }]);
    expect(Object.keys(snap)).toEqual(['s']);
    const restarted = applySnapshot({ s: { ...map.s, seq: 40 } }, [{ workspace: 's', state: 'working', seq: 2, stale: false, ts: 5 }]);
    expect(restarted.s.seq).toBe(2);
    expect(restarted.s.state).toBe('working');
  });

  it('calls working stale after a minute without news', () => {
    const a = { state: 'working' as const, seq: 1, stale: false, ts: 1000 };
    expect(isStale(a, 30_000)).toBe(false);
    expect(isStale(a, 62_000)).toBe(true);
    expect(isStale({ ...a, state: 'idle' }, 999_999)).toBe(false);
  });
});

describe('agent and last response', () => {
  it('takes both from the snapshot and advances the response time on done, message and working going quiet', () => {
    let map = applySnapshot({}, [{ workspace: 's', state: 'working', seq: 3, stale: false, ts: 1000, agent: 'claude', last_response_at: 500 }]);
    expect(map.s.agent).toBe('claude');
    expect(map.s.lastResponseAt).toBe(500);
    map = applyEvents(map, [ev(4, { kind: 'tool', name: 'Bash', summary: 'ls', phase: 'started' })]).map;
    expect(map.s.lastResponseAt).toBe(500);
    map = applyEvents(map, [ev(5, { kind: 'state', state: 'idle' })]).map;
    expect(map.s.lastResponseAt).toBe(1005);
    map = applyEvents(map, [ev(6, { kind: 'message', summary: 'ok' })]).map;
    expect(map.s.lastResponseAt).toBe(1006);
    const none = applySnapshot({}, [{ workspace: 't', state: 'idle', seq: 1, stale: false, ts: 1 }]);
    expect(none.t.agent).toBeUndefined();
    expect(none.t.lastResponseAt).toBeUndefined();
  });
});

describe('the message handler', () => {
  it('feeds the store, requests a sync on a gap and leaves every other message alone', () => {
    resetAgents();
    const sent: ClientMessage[] = [];
    const send = (m: ClientMessage) => sent.push(m);
    expect(handleAgentMessage({ type: 'agent_snapshot', states: [{ workspace: 's', state: 'idle', seq: 4, stale: false, ts: 1 }] }, send)).toBe(true);
    expect(get(agents).s.seq).toBe(4);
    handleAgentMessage({ type: 'agent_events', events: [ev(5, { kind: 'state', state: 'working' })] }, send);
    expect(sent).toEqual([]);
    handleAgentMessage({ type: 'agent_events', events: [ev(9, { kind: 'state', state: 'done' })] }, send);
    expect(sent).toEqual([{ type: 'agent_sync', after: { s: 9 } }]);
    expect(handleAgentMessage({ type: 'pong' } as ServerMessage, send)).toBe(false);
    expect(handleAgentMessage({ type: 'from_the_future' } as unknown as ServerMessage, send)).toBe(false);
  });
});

describe('server status (activity and attention)', () => {
  const att = (kind: 'needs_input' | 'finished_unseen' | 'failed', id = 'a1', workspace = 's') => ({ id, workspace, kind, raised_at: 5000 });
  const base = (): AgentMap => applyActivity({}, [{ workspace: 's', activity: 'working', ts: 10_000, task: { title: 'fix login', started_at: 4_000, source: 'hook' } }], 9_000);

  it('derives the card state from activity and attention, and raw events no longer move it', () => {
    let m = base();
    expect(m.s.state).toBe('working');
    m = applyAttention(m, [att('needs_input')]);
    expect(m.s.state).toBe('needs_input');
    expect(m.s.waitingSince).toBe(5000);
    expect(needsYou(m.s)).toBe(true);
    m = applyEvents(m, [{ workspace: 's', seq: 1, ts: 1, source: 'hook', confidence: 'high', kind: 'state', state: 'idle' } as never]).map;
    expect(m.s.state).toBe('needs_input');
    m = applyAttention(m, []);
    m = applyActivity(m, [{ workspace: 's', activity: 'idle', ts: 11_000 }], 11_000);
    expect(m.s.state).toBe('idle');
    expect(needsYou(m.s)).toBe(false);
  });

  it('counts a finished item as attention and picks the most urgent item of a workspace', () => {
    let m = applyAttention(base(), [att('finished_unseen', 'a1'), att('failed', 'a2')]);
    expect(m.s.attention?.id).toBe('a2');
    m = applyAttention(m, [att('finished_unseen', 'a1')]);
    m = applyActivity(m, [{ workspace: 's', activity: 'idle', ts: 12_000 }], 12_000);
    expect(m.s.state).toBe('done');
    expect(needsYou(m.s)).toBe(true);
  });

  it('shows a silent working workspace as unknown, never as stale', () => {
    const m = applyActivity(base(), [{ workspace: 's', activity: 'unknown', ts: 80_000 }], 80_000);
    expect(m.s.state).toBe('unknown');
    expect(isStale(m.s, 999_999)).toBe(false);
  });

  it('acknowledges a finished item only for a foreground workspace on a visible page, once', () => {
    const m = applyAttention(base(), [att('finished_unseen')]);
    const done = new Set<string>();
    expect(attentionToAck(m.s, false, true, done)).toEqual([]);
    expect(attentionToAck(m.s, true, false, done)).toEqual([]);
    expect(attentionToAck(m.s, true, true, done)).toEqual(['a1']);
    expect(attentionToAck(m.s, true, true, new Set(['a1']))).toEqual([]);
  });

  it('never acknowledges a question by looking at it', () => {
    const m = applyAttention(base(), [att('needs_input')]);
    expect(attentionToAck(m.s, true, true, new Set())).toEqual([]);
  });

  it('computes the running time on the server clock, so a skewed device clock does not matter', () => {
    // The server says it is 10_000 when this device thinks it is 9_000: the device is 1 s behind.
    const m = base();
    expect(m.s.clockOffset).toBe(1_000);
    // The task started at server time 4_000; at device time 19_000 the server clock reads 20_000.
    expect(runningMs(m.s, 19_000)).toBe(16_000);
    expect(runningMs({ ...m.s, task: undefined }, 19_000)).toBeNull();
  });
});
