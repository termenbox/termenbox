import type { ClientMessage, SessionViewMessage } from './protocol';

/** The workspace view (decision 0103): what the client does with a `session_view` message. Everything in it is
 *  untrusted text from a terminal: cleaned again here, capped, and only ever rendered as text. */

export type Tier = 't0' | 't1' | 't2' | 't3';
export const TAIL_LINES = 30;
export const LINE_CHARS = 200;
export const OPTION_CHARS = 40;
export const QUESTION_CHARS = 200;
export const MAX_OPTIONS = 9;

export interface ViewOption { label: string }
export interface ViewPrompt { id: string; question: string; kind: 'choice' | 'yes_no'; options: ViewOption[] }
export const TIMELINE_ENTRIES = 30;
export const TIMELINE_TEXT_CHARS = 200;
export const TIMELINE_FILES = 20;
export const TIMELINE_PLAN = 20;
export interface TimelineEntryView { kind: 'prompt' | 'tool' | 'message'; name?: string; text: string }
export interface TimelineView { entries: TimelineEntryView[]; plan: { text: string; status: string }[]; files: string[]; lastMessage?: string }
export interface ViewModel { workspace: string; tier: Tier; agent?: string; task?: string; tail: string[]; hash: string; prompt?: ViewPrompt; timeline?: TimelineView }

/** The transcript timeline, cleaned and capped again here (it is untrusted text); null when there is nothing to show. */
export function timelineFrom(t: SessionViewMessage['timeline']): TimelineView | undefined {
  if (!t || !Array.isArray(t.entries)) return undefined;
  const entries = t.entries.slice(-TIMELINE_ENTRIES).map((e): TimelineEntryView => ({
    kind: e?.kind === 'prompt' || e?.kind === 'tool' ? e.kind : 'message',
    name: e?.name ? cleanText(e.name, 40) : undefined,
    text: cleanText(e?.text, TIMELINE_TEXT_CHARS),
  })).filter((e) => e.text || e.name);
  const plan = (Array.isArray(t.plan) ? t.plan : []).slice(0, TIMELINE_PLAN)
    .map((p) => ({ text: cleanText(p?.text, TIMELINE_TEXT_CHARS), status: cleanText(p?.status, 20) })).filter((p) => p.text);
  const files = (Array.isArray(t.files) ? t.files : []).slice(-TIMELINE_FILES).map((f) => cleanText(f, 80)).filter(Boolean);
  const lastMessage = t.last_message ? cleanText(t.last_message, 400) : undefined;
  return entries.length || plan.length || lastMessage ? { entries, plan, files, lastMessage } : undefined;
}

/** Escape sequences, control characters, bidi overrides and zero-width characters out; the rest is kept. */
export function cleanText(input: unknown, max = LINE_CHARS): string {
  if (typeof input !== 'string') return '';
  return input
    // eslint-disable-next-line no-control-regex
    .replace(/\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)?/g, '')
    // eslint-disable-next-line no-control-regex
    .replace(/\u001b\[[0-?]*[ -/]*[@-~]?/g, '')
    .replace(/\t/g, '  ')
    // eslint-disable-next-line no-control-regex
    .replace(/[\u0000-\u001f\u007f-\u009f​-‏‪-‮⁠-⁩﻿]/g, '')
    .trimEnd()
    .slice(0, max);
}

export function tierOf(value: unknown): Tier {
  return value === 't1' || value === 't2' ? value : 't3';
}

export const TIER_LABEL: Record<Tier, { name: string; detail: string }> = {
  t0: { name: 'T0 · ACP', detail: 'The agent asks over its own protocol' },
  t1: { name: 'Structured', detail: 'From the agent’s hooks' },
  t2: { name: 'Profile', detail: 'Parsed from the screen with this agent’s profile' },
  t3: { name: 'Generic', detail: 'Read from the screen; check before you trust an option' },
};

/** The view model from a wire message, or null when it is not a usable view. */
export function viewFrom(msg: SessionViewMessage | null | undefined): ViewModel | null {
  if (!msg || typeof msg.session !== 'string' || !Array.isArray(msg.tail)) return null;
  const tail = msg.tail.slice(-TAIL_LINES).map((l) => cleanText(l));
  const p = msg.prompt;
  let prompt: ViewPrompt | undefined;
  if (p && typeof p.id === 'string' && Array.isArray(p.options)) {
    const options = p.options.slice(0, MAX_OPTIONS).map((o) => ({ label: cleanText(o?.label, OPTION_CHARS) })).filter((o) => o.label);
    if (options.length >= 2) {
      prompt = { id: cleanText(p.id, 64), question: cleanText(p.question, QUESTION_CHARS), kind: p.kind === 'yes_no' ? 'yes_no' : 'choice', options };
    }
  }
  return {
    workspace: msg.session,
    tier: tierOf(msg.tier),
    agent: msg.agent ? cleanText(msg.agent, 32) : undefined,
    task: msg.task ? cleanText(msg.task, 120) : undefined,
    tail,
    hash: typeof msg.hash === 'string' ? msg.hash : '',
    prompt,
    timeline: timelineFrom(msg.timeline),
  };
}

/** Keeps the held view when the server says nothing changed. */
export function merge(held: ViewModel | null, msg: SessionViewMessage): ViewModel | null {
  if (msg.unchanged && held && held.workspace === msg.session) return held;
  return viewFrom(msg);
}

/** The answer for a tap on option `index`. Only an option the view lists can be sent, and only as its index:
 *  the keys stay on the server. */
export function answerFor(view: ViewModel, index: number): ClientMessage | null {
  const p = view.prompt;
  if (!p || !Number.isInteger(index) || index < 0 || index >= p.options.length) return null;
  return { type: 'session_answer', session: view.workspace, prompt_id: p.id, option: index };
}

export const requestView = (workspace: string, held?: ViewModel | null): ClientMessage =>
  held && held.workspace === workspace && held.hash ? { type: 'session_view', session: workspace, hash: held.hash } : { type: 'session_view', session: workspace };
