import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import DecisionPanel from '../DecisionPanel.svelte';
import type { AskKind } from './protocol';
import { ASK_FALLBACK, askFallback, answerIndex, decisionFor, nextNeedsYou, replyLine, stepOf, STEP_SUMMARY_CHARS } from './decision';
import type { WorkspaceAgent } from './agents';

const waiting = (extra: Partial<WorkspaceAgent> = {}): WorkspaceAgent => ({
  state: 'needs_input', seq: 4, stale: false, ts: 10, agent: 'claude', serverStatus: true,
  attention: { id: 'a1', workspace: 's', kind: 'needs_input', raised_at: 5, prompt: 'Allow the edit?', task: { title: 'Fix login', started_at: 0, source: 'hook' } },
  prompt: { id: 'p1', kind: 'choice', choices: ['Yes', 'Yes, always', 'No'], seq: 4, confidence: 'high' },
  tool: { name: 'Edit', summary: 'src/auth.ts +12 -3', phase: 'started' },
  ...extra,
});
const sess = { key: 'k/s', name: 's', since: 5, question: 'Allow the edit?', title: 'Fix login' };

const html = (p: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(DecisionPanel, { props: p }).body;

describe('the decision a card shows', () => {
  it('is T1 with the hook step when the agent confirmed the prompt', () => {
    const d = decisionFor(sess, waiting(), 20);
    expect(d).toMatchObject({ tier: 't1', options: ['Yes', 'Yes, always', 'No'], step: { name: 'Edit', summary: 'src/auth.ts +12 -3' }, task: 'Fix login', since: 5, question: 'Allow the edit?' });
    expect(d.prompt?.id).toBe('p1');
  });
  it('has no options, no step and no prompt without hook confirmation', () => {
    const d = decisionFor(sess, waiting({ prompt: { id: 'p1', kind: 'choice', choices: ['Yes', 'No'], seq: 4, confidence: 'low' } }), 20);
    expect(d).toMatchObject({ tier: 't3', options: [], step: undefined, prompt: undefined });
  });
  it('offers no standing rule: only the agent’s own options, none added', () => {
    expect(decisionFor(sess, waiting(), 20).options).toEqual(['Yes', 'Yes, always', 'No']);
    expect(decisionFor(sess, waiting({ prompt: { id: 'p', kind: 'choice', choices: ['1', '2', '3', '4'], seq: 4, confidence: 'high' } }), 20).options).toEqual([]);
  });
  it('falls back to a plain question', () => {
    expect(decisionFor({ ...sess, question: undefined }, undefined, 20)).toMatchObject({ question: 'Needs your answer', content: false });
  });
  it('says what each kind of ask is until its content arrives, and never shows an empty card', () => {
    for (const kind of Object.keys(ASK_FALLBACK) as AskKind[]) {
      const a = waiting({ attention: { ...waiting().attention!, ask: kind, prompt: undefined } });
      const bare = decisionFor({ ...sess, question: undefined }, a, 20);
      expect(bare).toMatchObject({ ask: kind, content: false, question: ASK_FALLBACK[kind] });
      expect(bare.question.length).toBeGreaterThan(0);
      const out = html({ decision: bare, now: 65_000, onpick: () => {}, onreply: () => {}, onopen: () => {} });
      expect(out).toContain(ASK_FALLBACK[kind]);
      expect(out).toContain('Open terminal');
      expect(decisionFor(sess, a, 20)).toMatchObject({ ask: kind, content: true, question: 'Allow the edit?' });
    }
  });
  it('treats a kind this client does not know as unknown', () => {
    expect(askFallback('future' as AskKind)).toBe(ASK_FALLBACK.unknown);
    expect(askFallback(undefined)).toBe('Needs your answer');
  });
  it('offers one-tap options only from structured content', () => {
    const a = waiting({ prompt: undefined });
    expect(decisionFor(sess, a, 20).options).toEqual([]);
  });
});

describe('hook step as untrusted text', () => {
  it('is cleaned and capped, and only while the tool has started', () => {
    const s = stepOf(waiting({ tool: { name: 'Bash', summary: `\u001b[31mrm\u001b[0m ‮-rf ${'x'.repeat(500)}`, phase: 'started' } }))!;
    expect(s.summary.startsWith('rm -rf')).toBe(true);
    expect(s.summary).not.toMatch(/[\u001b‮]/);
    expect(s.summary.length).toBe(STEP_SUMMARY_CHARS);
    expect(stepOf(waiting({ tool: { name: 'Edit', summary: 'a', phase: 'finished' } }))).toBeUndefined();
    expect(stepOf(undefined)).toBeUndefined();
  });
});

describe('typed reply', () => {
  it('is one line without control characters', () => {
    expect(replyLine('  run\nthe\u001b[0m tests  ')).toBe('run the [0m tests');
    expect(replyLine('   ')).toBe('');
  });
});

describe('the decision panel', () => {
  const props = (over = {}) => ({ decision: decisionFor(sess, waiting(), 20), now: 65_000, onpick: () => {}, onreply: () => {}, ...over });
  it('shows the question, task, wait, tier label, hook step, options and a reply field', () => {
    const out = html(props());
    for (const t of ['Allow the edit?', 'Fix login', 'Waiting 1 min', 'Structured', 'The agent is about to use Edit', 'src/auth.ts +12 -3', 'Yes, always', 'aria-label="Write a reply"']) expect(out).toContain(t);
  });
  it('renders agent text as text, never as markup', () => {
    const d = { ...decisionFor(sess, waiting(), 20), question: '<img src=x onerror=alert(1)>', step: { name: 'Bash', summary: '<script>alert(1)</script>' } };
    const out = html(props({ decision: d }));
    expect(out).not.toContain('<img');
    expect(out).not.toContain('<script>alert');
    expect(out).toContain('&lt;img');
  });
  it('leaves the reply field out when the surface has its own composer', () => {
    expect(html(props({ composer: false }))).not.toContain('Write a reply');
  });
});

describe('answerIndex (the Yes and No shortcuts)', () => {
  const claude = ['Yes', 'Yes, and don’t ask again for npm test commands in /repo', 'No, and tell Claude what to do differently'];
  it('picks the plain Yes and the explicit No, never the standing permission', () => {
    expect(answerIndex(claude, 'yes')).toBe(0);
    expect(answerIndex(claude, 'no')).toBe(2);
  });
  it('is not fooled by the second option being a grant', () => {
    expect(answerIndex(['Yes', 'Yes, and don\'t ask again', 'No'], 'no')).toBe(2);
    expect(answerIndex(['Yes, and don\'t ask again', 'No'], 'yes')).toBeNull();
    expect(answerIndex(['Allow always', 'Deny'], 'yes')).toBeNull();
    expect(answerIndex(['Allow for this workspace', 'Allow once', 'Deny'], 'yes')).toBe(1);
  });
  it('does nothing when the options carry no plain answer', () => {
    expect(answerIndex(['Option A', 'Option B'], 'yes')).toBeNull();
    expect(answerIndex(['Option A', 'Option B'], 'no')).toBeNull();
    expect(answerIndex([], 'yes')).toBeNull();
  });
});

describe('nextNeedsYou', () => {
  const list = ['a', 'b', 'c', 'd'].map((key) => ({ key }));
  const waiting = new Set(['b', 'c', 'd']);
  const since = (k: string) => ({ b: 30, c: 10, d: 20 } as Record<string, number>)[k] ?? 0;
  it('goes round the waiting workspaces, longest wait first', () => {
    expect(nextNeedsYou(list, (k) => waiting.has(k), since, null)?.key).toBe('c');
    expect(nextNeedsYou(list, (k) => waiting.has(k), since, 'c')?.key).toBe('d');
    expect(nextNeedsYou(list, (k) => waiting.has(k), since, 'b')?.key).toBe('c');
  });
  it('stays put when the current workspace is the only one', () => {
    expect(nextNeedsYou(list, (k) => k === 'b', since, 'b')).toBeNull();
    expect(nextNeedsYou(list, () => false, since, null)).toBeNull();
  });
});
