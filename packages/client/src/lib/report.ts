// "Report a problem" (decision 0074): builds a scrubbed diagnostics text and a prefilled GitHub issue URL.
// The user reads and submits it; nothing is sent by the app. Guardrail diagnostics-never-include-secrets:
// no certificates, tokens, claim secrets, addresses, terminal output or workspace names.
import { writable } from 'svelte/store';
import { APP_VERSION } from './appVersion';

const REPO = 'termenbox/termenbox';
/** GitHub rejects very long query strings; stay well under it. */
export const MAX_URL_TEXT = 6000;
const MAX_ERRORS = 10;

export interface ErrorNote {
  at: number;
  text: string;
}

export const recentErrors = writable<ErrorNote[]>([]);

export function noteError(text: string, now = Date.now()): void {
  recentErrors.update((l) => [...l, { at: now, text: scrub(text).slice(0, 300) }].slice(-MAX_ERRORS));
}

/** Removes anything that identifies a user, a server or a login. */
export function scrub(text: string): string {
  return (
    text
      // device certificates and similar base64url.base64url pairs
      .replace(/[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}/g, '[cert]')
      // GitHub tokens and long secrets (claim secrets are 43 base64url characters)
      .replace(/\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,})\b/g, '[token]')
      .replace(/\b[A-Za-z0-9_-]{32,}\b/g, '[secret]')
      // IPv6 and IPv4 addresses, with an optional :port
      .replace(/\b(?:[0-9a-f]{1,4}:){2,7}[0-9a-f]{1,4}\b/gi, '[ip]')
      .replace(/\b\d{1,3}(?:\.\d{1,3}){3}(?::\d{1,5})?\b/g, '[ip]')
      // host:port pairs
      .replace(/\b[\w.-]+\.[a-z]{2,}:\d{2,5}\b/gi, '[host]')
  );
}

export interface Env {
  version: string | null;
  userAgent: string;
  inner: string;
  visual: string;
  connection: string;
  /** Where the terminal sits: left edge, width and scroll of each box around it. */
  layout?: string;
  /** What the app did just before (lib/breadcrumbs), oldest first. */
  steps?: string[];
  /** `version` from the server's auth_ok, so a stale image is visible. */
  server?: string;
  errors: ErrorNote[];
}

/** The text shown to the user and placed in the issue. */
export function buildReport(env: Env): string {
  const lines = [
    `app: ${APP_VERSION} (${env.version ?? 'dev'})`,
    `browser: ${env.userAgent}`,
    `window: ${env.inner}`,
    `visual viewport: ${env.visual}`,
    `connection: ${env.connection}`,
    ...(env.layout ? [`layout: ${env.layout}`] : []),
    `server: ${env.server ?? 'unknown'}`,
    'steps before this report:',
    ...(env.steps?.length ? env.steps : ['(none)']),
    'recent errors:',
    ...(env.errors.length ? env.errors.map((e) => `- ${new Date(e.at).toISOString()} ${e.text}`) : ['- none']),
  ];
  return scrub(lines.join('\n'));
}

/** What the person tells us before GitHub opens, so they need not look back at the app while writing the issue. */
export interface Description {
  steps: string;
  actual: string;
  expected: string;
}

export function describeProblem(d: Description): string {
  const part = (title: string, text: string) => `${title}\n${text.trim() || '-'}`;
  if (!d.steps.trim() && !d.actual.trim() && !d.expected.trim()) return '';
  return scrub([part('Steps to reproduce:', d.steps), part('Actual result:', d.actual), part('Expected result:', d.expected)].join('\n\n'));
}

export interface Plan {
  url: string;
  /** Set when the text was too long for the URL: copy it, the issue opens with a paste note. */
  copy?: string;
}

export function reportPlan(text: string, what = ''): Plan {
  const base = `https://github.com/${REPO}/issues/new?template=problem.yml`;
  const head = what ? `&what=${encodeURIComponent(what.slice(0, 1500))}` : '';
  if (text.length <= MAX_URL_TEXT) return { url: `${base}${head}&diagnostics=${encodeURIComponent(text)}` };
  return { url: `${base}${head}&diagnostics=${encodeURIComponent('(paste the copied diagnostics here)')}`, copy: text };
}
