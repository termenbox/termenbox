import { describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { render } from 'svelte/server';
import AcpView from './AcpView.svelte';
import AcpEntryView from './AcpEntryView.svelte';
import AcpMarkdown from './AcpMarkdown.svelte';
import NewSessionSheet from './NewSessionSheet.svelte';
import { Composer } from './lib/ds';
import { ServerHub, type HubConnection, type HubServer } from './lib/hub';
import type { ClientMessage, ServerListItem, ServerMessage } from './lib/protocol';
import type { ConnectionOptions } from './lib/connection';
import { decisionFor } from './lib/decision';
import type { SessionAgent } from './lib/agents';
import { applyEvents } from './lib/agents';
import { applyView, emptyStream, entryFrom, type Entry, type StreamState } from './lib/acpStream';
import { parseBlocks } from './lib/markdown';
import type { AcpEntryWire, AcpViewMessage } from './lib/protocol';

const html = (c: unknown, props: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(c, { props }).body;
const wire = (e: Record<string, unknown>) => entryFrom(e as AcpEntryWire) as Entry;
const base = { opened: new Set<string>(), ontoggle: () => {}, onanswer: () => {}, sessionKey: 'srv/acp:work', name: 'work', agent: 'claude', now: 0 };

describe('entries are drawn as text', () => {
  it('hostile agent text never becomes markup, in any kind of entry', () => {
    const evil = '<img src=x onerror=alert(1)><script>alert(2)</script>';
    for (const e of [
      wire({ id: 1, rev: 1, kind: 'user', text: evil }),
      wire({ id: 2, rev: 2, kind: 'thought', text: evil }),
      wire({ id: 3, rev: 3, kind: 'tool', tool_id: 't', title: evil, tool_kind: 'edit', status: 'pending', locations: [{ path: `/w/${evil}` }] }),
      wire({ id: 4, rev: 4, kind: 'plan', items: [{ text: evil, status: 'pending' }] }),
      wire({ id: 5, rev: 5, kind: 'notice', level: 'error', text: evil }),
      wire({ id: 6, rev: 6, kind: 'permission', prompt_id: 'p', tool_id: 't', title: evil, tool_kind: 'edit', options: [], state: 'answered' }),
    ]) {
      const out = html(AcpEntryView, { ...base, entry: e });
      expect(out, e.kind).not.toContain('<img');
      expect(out, e.kind).not.toContain('<script>alert');
      expect(out, e.kind).toContain('&lt;');
    }
    const md = html(AcpMarkdown, { blocks: parseBlocks(`# ${evil}\n\n- ${evil}\n\n[x](javascript:alert(1)) <b onclick=x>b</b>\n\n\`\`\`\n${evil}\n\`\`\``) });
    expect(md).not.toContain('<img');
    expect(md).not.toContain('<script>alert');
    expect(md).not.toContain('href="javascript');
    expect(md).not.toContain('<b onclick');
  });

  it('a tool says its kind and status in words, and shows a diff only behind a tap', () => {
    const e = wire({ id: 1, rev: 1, kind: 'tool', tool_id: 't', title: 'Write a.txt', tool_kind: 'edit', status: 'completed', diffs: [{ path: '/w/a.txt', new_text: 'hi' }], locations: [{ path: '/w/a.txt', line: 3 }] });
    const out = html(AcpEntryView, { ...base, entry: e });
    for (const t of ['Write a.txt', 'edit', 'done', '>a.txt<', ':3<', 'aria-expanded="false"']) expect(out).toContain(t);
    expect(out).not.toContain('Changes in');
    expect(html(AcpEntryView, { ...base, entry: e, opened: new Set(['1:0']) })).toContain('Changes in /w/a.txt');
    expect(html(AcpEntryView, { ...base, entry: e, asking: true })).not.toContain('data-diff');
  });

  it('an open permission is the decision card with the agent’s own one-time options and the diff; an answered one is a log line', () => {
    const open = wire({ id: 1, rev: 1, kind: 'permission', prompt_id: 'acp-1', tool_id: 't', title: 'Write a.txt', tool_kind: 'edit', state: 'open',
      options: [{ id: 'allow-once', kind: 'allow_once', name: 'Yes' }, { id: 'allow-always', kind: 'allow_always', name: 'Always' }, { id: 'reject', kind: 'reject_once', name: 'No' }],
      diffs: [{ path: '/w/a.txt', new_text: 'hi' }] });
    const out = html(AcpEntryView, { ...base, entry: open });
    for (const t of ['aria-label="Decision"', 'Write a.txt', 'Wants to change a file', 'T0 · ACP', 'Yes', 'No', 'data-hero-options', 'Changes in /w/a.txt']) expect(out).toContain(t);
    expect(out).not.toContain('Always');
    expect(out).not.toContain('Write a reply');
    const done = wire({ ...(open as unknown as Record<string, unknown>), kind: 'permission', prompt_id: 'acp-1', tool_id: 't', tool_kind: 'edit', state: 'answered', chosen: 'reject',
      options: [{ id: 'reject', kind: 'reject_once', name: 'No' }], diffs: [] });
    const line = html(AcpEntryView, { ...base, entry: done });
    expect(line).toContain('Rejected · No');
    expect(line).not.toContain('Decision');
  });

  it('a reply that was cut says so, in the reply and in a thought', () => {
    const cut = (kind: string, truncated: boolean) => wire({ id: 1, rev: 1, kind, text: 'The first part', truncated });
    for (const kind of ['message', 'thought']) {
      expect(html(AcpEntryView, { ...base, entry: cut(kind, true) }), kind).toContain('The reply is longer; the rest is not shown.');
      expect(html(AcpEntryView, { ...base, entry: cut(kind, false) }), kind).not.toContain('is longer');
    }
  });

  it('an error line is an alert only when it is new, not when it is history', () => {
    const err = wire({ id: 1, rev: 1, kind: 'notice', level: 'error', text: 'It failed' });
    expect(html(AcpEntryView, { ...base, entry: err })).not.toContain('role="alert"');
    expect(html(AcpEntryView, { ...base, entry: err, announce: true })).toContain('role="alert"');
    expect(html(AcpEntryView, { ...base, entry: wire({ id: 2, rev: 2, kind: 'notice', level: 'info', text: 'ok' }), announce: true })).not.toContain('role="alert"');
  });

  it('search marks the hits in the text it draws', () => {
    const e = wire({ id: 1, rev: 1, kind: 'message', text: 'The **parser** and the lexer' });
    const out = html(AcpEntryView, { ...base, entry: e, query: 'er', current: 1 });
    expect((out.match(/<mark/g) ?? []).length).toBe(2); // pars-er and lex-er; the second is the current one
    expect((out.match(/data-current/g) ?? []).length).toBe(1);
  });
});

describe('the session view', () => {
  it('a history with an old error announces nothing when it is opened', () => {
    const v = { type: 'acp_view' as const, session: 'acp:work', epoch: 1, rev: 2, reset: true, first: 1, entries: [{ id: 1, rev: 2, kind: 'notice', level: 'error', text: 'Old failure' }] as AcpEntryWire[],
      info: { agent: 'claude', dir: '/w', status: 'running', working: false } };
    const out = html(AcpView, { sessionKey: 'srv/acp:work', name: 'work', agent: 'claude', stream: applyView(emptyStream('acp:work'), v), onprompt: () => {}, oninterrupt: () => {}, onanswer: () => {}, onmode: () => {}, onshell: () => {} });
    expect(out).toContain('Old failure');
    expect(out).not.toContain('role="alert"');
  });

  const view = (over: Partial<AcpViewMessage> = {}): AcpViewMessage => ({ type: 'acp_view', session: 'acp:work', epoch: 1, rev: 3, reset: true, first: 1, entries: [{ id: 1, rev: 1, kind: 'user', text: 'hello' }, { id: 2, rev: 3, kind: 'message', text: 'Hi **there**' }],
    info: { agent: 'claude', dir: '/w', status: 'running', working: false, mode: 'ask', modes: [{ id: 'ask', name: 'Ask' }, { id: 'plan', name: 'Plan' }], commands: [{ name: 'compact' }], usage: { used: 250, size: 1000 } }, ...over });
  const props = (stream?: StreamState, extra: Record<string, unknown> = {}) => ({ sessionKey: 'srv/acp:work', name: 'work', agent: 'claude', stream, onprompt: () => {}, oninterrupt: () => {}, onanswer: () => {}, onmode: () => {}, onshell: () => {}, ...extra });

  it('labels the tier, lists the stream, the agent’s modes, the context use and the prompt line', () => {
    const out = html(AcpView, props(applyView(emptyStream('acp:work'), view())));
    for (const t of ['T0 · ACP', 'work', 'hello', 'Hi', 'there', 'Ask', 'Plan', '25%', 'Search this session', 'Copy the whole session', 'Shell', 'aria-label="Prompt"', 'Prompt keys', 'role="log"']) expect(out).toContain(t);
    expect(out).not.toContain('Stop the agent');
  });
  it('shows a Stop key while the agent works, and closes the prompt line when it has ended', () => {
    const working = html(AcpView, props(applyView(emptyStream('s'), view({ info: { ...view().info!, working: true } }))));
    expect(working).toContain('Stop the agent');
    const ended = html(AcpView, props(applyView(emptyStream('s'), view({ info: { ...view().info!, status: 'ended' } }))));
    expect(ended).toContain('This session has ended.');
    expect(ended).toMatch(/<textarea[^>]*disabled/);
  });
  it('says what it is waiting for: opening, an empty session, a lost connection', () => {
    expect(html(AcpView, props(undefined))).toContain('Opening the session');
    expect(html(AcpView, props(undefined, { connState: 'reconnecting' }))).toContain('Waiting for the connection');
    expect(html(AcpView, props(applyView(emptyStream('s'), view({ entries: [] }))))).toContain('Say what you want done');
    const lost = html(AcpView, props(applyView(emptyStream('s'), view()), { connState: 'reconnecting' }));
    expect(lost).toContain('Reconnecting. What you see is kept.');
    expect(lost).toContain('Reconnecting. Draft kept.');
    expect(lost).toContain('hello');
  });
  it('the Shell key is there but off when the server runs no tmux', () => {
    const out = html(AcpView, props(applyView(emptyStream('s'), view()), { canShell: false }));
    expect(out).toMatch(/data-testid="shell"[^>]*disabled|disabled[^>]*data-testid="shell"/);
    expect(html(AcpView, props(applyView(emptyStream('s'), view()), { canShell: true }))).not.toMatch(/disabled[^>]*data-testid="shell"/);
  });
});

describe('the prompt line is the shared composer', () => {
  it('multi-line: a textarea that sends on Enter, with the same 16 px type and named keys', () => {
    const out = html(Composer, { multiline: true, label: 'Prompt', value: 'hi' });
    expect(out).toContain('<textarea');
    expect(out).toContain('text-input');
    expect(out).toContain('aria-label="Send"');
    expect(html(Composer, { sendDisabled: true, value: 'x' })).toMatch(/<button[^>]*type="submit"[^>]*disabled|disabled[^>]*type="submit"/);
  });
});

describe('T0 in the shared pieces', () => {
  const a = (source?: string): SessionAgent => ({ state: 'needs_input', seq: 4, stale: false, ts: 10, agent: 'claude', serverStatus: true,
    attention: { id: 'a1', session: 's', kind: 'needs_input', raised_at: 5, prompt: 'Write a.txt' },
    prompt: { id: 'acp-1', kind: 'permission', choices: ['Yes', 'No'], seq: 4, confidence: 'high', source } });
  it('Home’s card says T0 for an ACP prompt and T1 for a hook one', () => {
    const s = { key: 'k/s', name: 's', since: 5, question: 'Write a.txt' };
    expect(decisionFor(s, a('acp'), 20).tier).toBe('t0');
    expect(decisionFor(s, a('hook'), 20).tier).toBe('t1');
    expect(decisionFor(s, a('acp'), 20).options).toEqual(['Yes', 'No']);
  });
  it('a prompt event keeps where it came from, so the card can say so', () => {
    const r = applyEvents({}, [{ session: 's', seq: 1, ts: 1, source: 'acp', confidence: 'high', kind: 'prompt', id: 'acp-1', prompt_kind: 'permission', choices: ['Yes'] } as never]);
    expect(r.map.s.prompt?.source).toBe('acp');
  });
});

describe('starting an ACP session', () => {
  const server = (backends: string[], acpAgents: string[]): HubServer => ({ id: 'a', state: 'connected', reason: '', version: '0.1.34+dev', backends, acpAgents, controlMode: true, tmuxVersion: '3.6', prefix: 'C-b',
    server: { server_url: '', server_id: 'a', server_name: 'alpha', registered_at: 0 } as HubServer['server'] });
  const sheet = (s: HubServer) => html(NewSessionSheet, { servers: [s], initialServerId: 'a', locked: true, sessions: [], windows: {}, oncreate: () => {}, onclose: () => {} });
  it('offers the owner’s agents under the ACP manager, and the tmux ones otherwise', () => {
    const both = sheet(server(['tmux', 'acp'], ['claude', 'kilo']));
    expect(both).toContain('Agent session (ACP)');
    expect(both).toContain('Codex'); // tmux first: the owner's call whether new sessions default to ACP is not made
    const acpOnly = sheet(server(['acp'], ['claude', 'kilo']));
    expect(acpOnly).toContain('Claude Code');
    expect(acpOnly).toContain('Kilo');
    expect(acpOnly).not.toContain('Codex');
    expect(acpOnly).toContain('Choose the directory the agent works in');
    expect(sheet(server(['acp'], []))).toContain('This server allows no agent sessions yet.');
  });
  it('the hub keeps the agent list the server announced', () => {
    const connections: { opts: ConnectionOptions }[] = [];
    const hub = new ServerHub({ credentials: async () => ({ cert: 'c', sign: async () => 's' }), factory: (opts) => {
      connections.push({ opts });
      return { start() { opts.onState('connecting'); }, close() {}, send(_m: ClientMessage) {}, subscribe(_f: (m: ServerMessage) => void) { return () => {}; } } as HubConnection;
    } });
    hub.start([{ server_id: 'a', server_url: '', server_name: 'a', registered_at: 0 } as ServerListItem]);
    connections[0].opts.onState('connected');
    connections[0].opts.onReady({ type: 'auth_ok', user: 'u', server_name: 'a', backends: ['tmux', 'acp'], acp_agents: ['claude'] }, false);
    expect(get(hub.servers)[0].acpAgents).toEqual(['claude']);
  });
});
