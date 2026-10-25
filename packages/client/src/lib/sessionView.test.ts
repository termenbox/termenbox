import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import SessionView from '../SessionView.svelte';
import { cleanText, viewFrom, merge, answerFor, requestView, TAIL_LINES, LINE_CHARS } from './sessionView';
import type { SessionViewMessage } from './protocol';

const msg = (over: Partial<SessionViewMessage> = {}): SessionViewMessage => ({ type: 'session_view', session: 's', tier: 't3', tail: ['$ make', 'done'], hash: 'h1', ...over });
const html = (props: Record<string, unknown>) => (render as unknown as (c: unknown, o: { props: Record<string, unknown> }) => { body: string })(SessionView, { props }).body;

describe('untrusted terminal text', () => {
  it('drops escape sequences, controls, bidi and zero-width characters', () => {
    expect(cleanText('\u001b[31mred\u001b[0m ok')).toBe('red ok');
    expect(cleanText('a\u001b]0;title\u0007b')).toBe('ab');
    expect(cleanText('x\u0000y\u0008z\u007f')).toBe('xyz');
    expect(cleanText('‮evil⁦​')).toBe('evil');
    expect(cleanText(42)).toBe('');
  });
  it('caps lines and the tail', () => {
    expect(cleanText('x'.repeat(500)).length).toBe(LINE_CHARS);
    const v = viewFrom(msg({ tail: Array.from({ length: 100 }, (_, i) => `l${i}`) }))!;
    expect(v.tail).toHaveLength(TAIL_LINES);
    expect(v.tail.at(-1)).toBe('l99');
  });
  it('renders markup in the tail as text', () => {
    const out = html({ name: 'api', view: viewFrom(msg({ tail: ['<img src=x onerror=alert(1)>'] })) });
    expect(out).not.toContain('<img');
    expect(out).toContain('&lt;img');
  });
});

describe('wire compatibility', () => {
  it('reads an unknown tier as t3 and an unknown prompt kind as a choice', () => {
    expect(viewFrom(msg({ tier: 't9' }))!.tier).toBe('t3');
    const v = viewFrom(msg({ tier: 't2', prompt: { id: 'p', question: 'Go?', kind: 'future', options: [{ label: 'A' }, { label: 'B' }] } }))!;
    expect(v.tier).toBe('t2');
    expect(v.prompt!.kind).toBe('choice');
  });
  it('ignores extra fields and refuses a message that is not a view', () => {
    expect(viewFrom({ ...msg(), extra: 1 } as SessionViewMessage)).not.toBeNull();
    expect(viewFrom({ type: 'session_view' } as unknown as SessionViewMessage)).toBeNull();
    expect(viewFrom(null)).toBeNull();
  });
  it('keeps the held view when the server says it is unchanged, and asks with its hash', () => {
    const held = viewFrom(msg())!;
    expect(merge(held, msg({ tail: [], unchanged: true }))).toBe(held);
    expect(requestView('s', held)).toEqual({ type: 'session_view', session: 's', hash: 'h1' });
    expect(requestView('other', held)).toEqual({ type: 'session_view', session: 'other' });
  });
  it('a prompt with fewer than two options is not offered', () => {
    expect(viewFrom(msg({ prompt: { id: 'p', question: 'q', kind: 'choice', options: [{ label: 'only' }] } }))!.prompt).toBeUndefined();
  });
});

describe('answers send only what the view lists', () => {
  const v = viewFrom(msg({ tier: 't2', prompt: { id: 'p1', question: 'Proceed?', kind: 'choice', options: [{ label: 'Yes' }, { label: 'No' }] } }))!;
  it('an option index becomes a session_answer with no keys', () => {
    expect(answerFor(v, 1)).toEqual({ type: 'session_answer', session: 's', prompt_id: 'p1', option: 1 });
  });
  it('an index outside the listed options sends nothing', () => {
    for (const i of [-1, 2, 99, 1.5, NaN]) expect(answerFor(v, i)).toBeNull();
    expect(answerFor(viewFrom(msg())!, 0)).toBeNull();
  });
});

describe('workspace view for an agent with no profile (T3)', () => {
  it('shows header, tail, composer, tier label and the terminal button', () => {
    const out = html({ name: 'api', agent: 'amp', fleet: 'working', task: 'Fix the build', startedAt: Date.now() - 120_000, view: viewFrom(msg()) });
    for (const t of ['api', 'Fix the build', '2 min', 'Working', '$ make', 'Generic view', 'Open full terminal', 'aria-label="Send a line"']) expect(out).toContain(t);
    expect(out).not.toContain('aria-label="Decision"');
  });
  it('is never empty before the first view arrives', () => {
    const out = html({ name: 'api', agent: 'amp' });
    expect(out).toContain('Reading the screen');
    expect(out).toContain('Open full terminal');
    expect(out).toContain('Generic view');
    expect(out).toContain('aria-label="Send a line"');
  });
  it('shows the parsed decision with numbered options when the generic detector found one', () => {
    const out = html({ name: 'api', fleet: 'needs-you', view: viewFrom(msg({ prompt: { id: 'p', question: 'Overwrite file?', kind: 'yes_no', options: [{ label: 'Yes' }, { label: 'No' }] } })) });
    for (const t of ['Overwrite file?', 'Yes', 'No', 'aria-label="Decision"']) expect(out).toContain(t);
  });
  it('names the profile tier when a profile parsed the screen', () => {
    expect(html({ name: 'api', view: viewFrom(msg({ tier: 't2' })) })).toContain('Profile view');
  });
});

describe('the T1 timeline from a recorded transcript', () => {
  // The file the server's transcript test writes from the recorded Claude Code transcript fixture.
  const recorded = JSON.parse(readFileSync(new URL('../../../server/testdata/transcripts/claude-session.view.json', import.meta.url), 'utf8')) as SessionViewMessage;

  it('renders the steps, plan progress, changed files and last message', () => {
    const out = html({ name: 'api', agent: 'claude', view: viewFrom(recorded) });
    for (const t of ['data-testid="timeline"', 'Fix the failing parser test', 'cargo test -p parser', 'Plan · 1 of 3 done', 'Fixing the parser', 'lib.rs, main.rs', 'all tests pass', 'Structured view']) expect(out).toContain(t);
    expect(out.indexOf('data-testid="timeline"')).toBeLessThan(out.indexOf('aria-label="Send a line"'));
  });
  it('shows no timeline below T1 or when the server sends none (no hooks: no regression)', () => {
    expect(html({ name: 'api', view: viewFrom({ ...recorded, tier: 't2' }) })).not.toContain('data-testid="timeline"');
    expect(html({ name: 'api', view: viewFrom({ ...recorded, timeline: undefined }) })).not.toContain('data-testid="timeline"');
    expect(html({ name: 'api', view: viewFrom({ ...recorded, tier: 't3', timeline: undefined }) })).toContain('Generic view');
  });
  it('cleans and caps transcript text again and renders markup as text', () => {
    const v = viewFrom({ ...recorded, timeline: { entries: [{ kind: 'prompt', text: '\u001b[31m<img src=x onerror=alert(1)>' + 'x'.repeat(500) }, ...Array.from({ length: 60 }, () => ({ kind: 'tool', name: 'Bash', text: 'ls' }))], files: Array.from({ length: 50 }, (_, i) => `f${i}`) } })!;
    expect(v.timeline!.entries.length).toBe(30);
    expect(v.timeline!.files.length).toBe(20);
    const out = html({ name: 'api', view: viewFrom({ ...recorded, timeline: { entries: [{ kind: 'prompt', text: '<img src=x onerror=alert(1)>' }] } }) });
    expect(out).not.toContain('<img');
  });
  it('an old message without a timeline field still parses', () => {
    expect(viewFrom({ type: 'session_view', session: 's', tier: 't1', tail: [], hash: 'h' })!.timeline).toBeUndefined();
  });
});
