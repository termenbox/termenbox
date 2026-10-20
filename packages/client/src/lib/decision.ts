// The decision card's data (decisions 0103, 0104): one model for Home, the inbox and the session view, built from
// what the server already sends. Everything here that came from an agent is untrusted: cleaned and capped, and only
// ever rendered as text. No standing rules and no batch approve (0104).
import { cleanText, type Tier } from './sessionView';
import { bannerChoices } from './notify';
import { isStale, type SessionAgent } from './agents';
import { clean as cleanLine } from './snippets';

export const STEP_NAME_CHARS = 40;
export const STEP_SUMMARY_CHARS = 200;
/** How long an answer may wait for the server to clear Attention before the card says so and lets the user try again. */
export const ANSWER_WAIT_MS = 8000;

export interface Step { name: string; summary: string }

export interface Decision {
  key: string;
  name: string;
  agent?: string;
  /** The task it belongs to (untrusted text). */
  task?: string;
  /** When the wait began (ms). */
  since?: number;
  question: string;
  tier: Tier;
  /** The tool step the agent's hook reported, at T1 only (untrusted text). */
  step?: Step;
  /** The labels of the agent's own options, in order. */
  options: string[];
  /** Which prompt this is, for a surface that answers by index (the session view). */
  promptId?: string;
  /** The prompt a one-tap answer is attached to, as it was when the card was built. */
  prompt?: NonNullable<SessionAgent['prompt']>;
}

/** The command, file or diff summary of the step the agent is about to take, from its own hook: the latest `tool`
 *  event, only while it has started and not finished. */
export function stepOf(a: SessionAgent | undefined): Step | undefined {
  const t = a?.tool;
  if (!t || t.phase !== 'started') return undefined;
  const name = cleanText(t.name, STEP_NAME_CHARS);
  const summary = cleanText(t.summary, STEP_SUMMARY_CHARS);
  return name || summary ? { name, summary } : undefined;
}

/** The card for a session that needs you. Options come only from a prompt the agent's own hook confirmed (the
 *  banner's rule), so the tier with options is T1 (T0 when the session is an ACP session); without one the card has the question and the reply field only. */
export function decisionFor(s: { key: string; name: string; since?: number; question?: string; title?: string }, a: SessionAgent | undefined, now: number): Decision {
  const options = a ? bannerChoices(a, now) : [];
  const structured = options.length > 0 && !!a && !isStale(a, now);
  return {
    key: s.key,
    name: s.name,
    agent: a?.agent,
    task: s.title ? cleanText(s.title, 120) : undefined,
    since: s.since,
    question: cleanText(s.question, 200) || 'Needs your answer',
    tier: structured ? (a!.prompt?.source === 'acp' ? 't0' : 't1') : 't3',
    step: structured ? stepOf(a) : undefined,
    options: options.map((o) => cleanText(o, 40)),
    prompt: structured ? a!.prompt : undefined,
  };
}

/** The line a typed reply sends: one line, no control characters, capped. The server types it, then Enter. */
export const replyLine = (text: string): string => cleanLine(text);
