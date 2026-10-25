// The client's copy of an ACP workspace (T0, decision 0111, U07b): a reducer over the server's `acp_view` messages, kept
// bounded, plus the pure helpers the view uses (search, diffs, copy text). Everything from the agent is untrusted: it
// is cleaned again here, capped, and only ever rendered as text.
import type { AcpDiff, AcpEntryWire, AcpInfo, AcpLocation, AcpViewMessage, ClientMessage } from './protocol';
import { cleanText } from './sessionView';
import { countHits, hitsIn, parseBlocks, partsOf, runsOf, type Block, type Part } from './markdown';

export const MAX_ENTRIES = 400;
export const MAX_TEXT = 16_000;
const LINE = 300;

export interface Option { id: string; kind: string; name: string }
export type Entry =
  | { id: number; rev: number; kind: 'user'; text: string }
  /** `truncated`: the reply was longer than the server keeps and the rest is not shown. */
  | { id: number; rev: number; kind: 'message'; text: string; truncated: boolean }
  | { id: number; rev: number; kind: 'thought'; text: string; truncated: boolean }
  | { id: number; rev: number; kind: 'tool'; toolId: string; title: string; toolKind: string; status: string; diffs: Diff[]; locations: AcpLocation[] }
  | { id: number; rev: number; kind: 'plan'; items: { text: string; status: string }[] }
  | { id: number; rev: number; kind: 'permission'; promptId: string; toolId: string; title: string; toolKind: string; options: Option[]; diffs: Diff[]; plan?: string; state: 'open' | 'answered' | 'cancelled'; chosen?: string }
  | { id: number; rev: number; kind: 'notice'; level: 'info' | 'error'; text: string };

export interface Diff { path: string; oldText?: string; newText: string; truncated: boolean }

export interface Info {
  /** `dir` is for showing (cleaned); `cwd` is the exact directory, to start a shell in, or absent when it cannot be trusted as is. */
  agent: string; dir: string; cwd?: string; title?: string; status: 'running' | 'ended' | 'failed'; statusText?: string; working: boolean;
  mode?: string; modes: { id: string; name: string; description?: string }[];
  commands: { name: string; description?: string; hint?: string }[];
  usage?: { used: number; size: number; cost?: string };
}

export interface StreamState {
  workspace: string;
  epoch: number;
  rev: number;
  first: number;
  entries: Entry[];
  info?: Info;
}

/** Control characters and escape sequences out, lines and indentation kept, zero-width and bidi overrides dropped. */
export function cleanBlock(input: unknown, max = MAX_TEXT): string {
  if (typeof input !== 'string') return '';
  return input
    // eslint-disable-next-line no-control-regex
    .replace(/\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)?/g, '')
    // eslint-disable-next-line no-control-regex
    .replace(/\u001b\[[0-?]*[ -/]*[@-~]?/g, '')
    .replace(/\r\n?/g, '\n')
    .replace(/\t/g, '  ')
    // eslint-disable-next-line no-control-regex
    .replace(/[\u0000-\u0009\u000b-\u001f\u007f-\u009f​-‏‪-‮⁠-⁩﻿]/g, '')
    .slice(0, max);
}

const STATUS = new Set(['running', 'ended', 'failed']);

function cleanDiff(d: AcpDiff): Diff {
  return { path: cleanText(d?.path, 300), oldText: typeof d?.old_text === 'string' ? cleanBlock(d.old_text, 8000) : undefined, newText: cleanBlock(d?.new_text, 8000), truncated: !!d?.truncated };
}

export function entryFrom(e: AcpEntryWire): Entry | null {
  if (!e || !Number.isInteger(e.id)) return null;
  const base = { id: e.id, rev: Number(e.rev) || 0 };
  const w = e as Record<string, unknown>;
  switch (e.kind) {
    case 'user':
      return { ...base, kind: 'user', text: cleanBlock(w.text) };
    case 'message':
    case 'thought':
      return { ...base, kind: e.kind, text: cleanBlock(w.text), truncated: w.truncated === true };
    case 'tool':
      return { ...base, kind: 'tool', toolId: cleanText(w.tool_id, 120), title: cleanText(w.title, LINE), toolKind: cleanText(w.tool_kind, 20) || 'other', status: cleanText(w.status, 20) || 'pending',
        diffs: (Array.isArray(w.diffs) ? (w.diffs as AcpDiff[]) : []).slice(0, 3).map(cleanDiff),
        locations: (Array.isArray(w.locations) ? (w.locations as AcpLocation[]) : []).slice(0, 5).map((l) => ({ path: cleanText(l?.path, 300), line: Number.isInteger(l?.line) ? l.line : undefined })) };
    case 'plan':
      return { ...base, kind: 'plan', items: (Array.isArray(w.items) ? (w.items as { text: string; status: string }[]) : []).slice(0, 20).map((i) => ({ text: cleanText(i?.text, 200), status: cleanText(i?.status, 20) })).filter((i) => i.text) };
    case 'permission': {
      const state = w.state === 'answered' || w.state === 'cancelled' ? w.state : 'open';
      return { ...base, kind: 'permission', promptId: cleanText(w.prompt_id, 40), toolId: cleanText(w.tool_id, 120), title: cleanText(w.title, LINE), toolKind: cleanText(w.tool_kind, 20) || 'other',
        // Only a one-time option is ever offered, whatever the server sent (decision 0104).
        options: (Array.isArray(w.options) ? (w.options as Option[]) : []).filter((o) => o?.kind === 'allow_once' || o?.kind === 'reject_once').slice(0, 4)
          .map((o) => ({ id: typeof o.id === 'string' ? o.id : '', kind: o.kind, name: cleanText(o.name, 40) })).filter((o) => o.id),
        diffs: (Array.isArray(w.diffs) ? (w.diffs as AcpDiff[]) : []).slice(0, 3).map(cleanDiff),
        plan: typeof w.plan === 'string' ? cleanBlock(w.plan, 8000) : undefined, state, chosen: typeof w.chosen === 'string' ? w.chosen : undefined };
    }
    case 'notice':
      return { ...base, kind: 'notice', level: w.level === 'error' ? 'error' : 'info', text: cleanText(w.text, LINE) };
    default:
      return null; // a kind from a newer server
  }
}

/** The directory exactly as the server sent it, or undefined when it is not a plain path (cleaning would change it). */
// eslint-disable-next-line no-control-regex
const exactDir = (d: unknown): string | undefined => (typeof d === 'string' && d.length > 0 && d.length <= 512 && !/[\u0000-\u001f\u007f-\u009f]/.test(d) ? d : undefined);

export function infoFrom(i: AcpInfo | undefined): Info | undefined {
  if (!i || typeof i !== 'object') return undefined;
  return {
    agent: cleanText(i.agent, 40), dir: cleanText(i.dir, 512), cwd: exactDir(i.dir), title: i.title ? cleanText(i.title, 200) : undefined,
    status: STATUS.has(i.status) ? (i.status as Info['status']) : 'running', statusText: i.status_text ? cleanText(i.status_text, 120) : undefined, working: !!i.working,
    mode: i.mode ? cleanText(i.mode, 80) : undefined,
    modes: (Array.isArray(i.modes) ? i.modes : []).slice(0, 12).map((m) => ({ id: String(m?.id ?? ''), name: cleanText(m?.name, 40), description: m?.description ? cleanText(m.description, 160) : undefined })).filter((m) => m.id),
    commands: (Array.isArray(i.commands) ? i.commands : []).slice(0, 120).map((c) => ({ name: cleanText(c?.name, 64), description: c?.description ? cleanText(c.description, 160) : undefined, hint: c?.hint ? cleanText(c.hint, 80) : undefined })).filter((c) => c.name),
    usage: i.usage && Number.isFinite(i.usage.used) && Number.isFinite(i.usage.size) ? { used: i.usage.used, size: i.usage.size, cost: i.usage.cost ? cleanText(i.usage.cost, 40) : undefined } : undefined,
  };
}

export const emptyStream = (workspace: string): StreamState => ({ workspace, epoch: 0, rev: 0, first: 0, entries: [] });

/** Applies one `acp_view`: a reset replaces everything, otherwise changed entries replace the ones with the same id and
 *  new ones are added at the end. Entries older than the server keeps, and beyond the cap, are dropped. */
export function applyView(state: StreamState, msg: AcpViewMessage): StreamState {
  const reset = msg.reset || msg.epoch !== state.epoch;
  const byId = new Map<number, Entry>(reset ? [] : state.entries.map((e) => [e.id, e]));
  for (const w of Array.isArray(msg.entries) ? msg.entries : []) {
    const e = entryFrom(w);
    if (e) byId.set(e.id, e);
  }
  let entries = [...byId.values()].filter((e) => e.id >= (msg.first || 0)).sort((a, b) => a.id - b.id);
  if (entries.length > MAX_ENTRIES) entries = entries.slice(-MAX_ENTRIES);
  return { workspace: state.workspace, epoch: msg.epoch, rev: msg.rev, first: msg.first, entries, info: infoFrom(msg.info) ?? (reset ? undefined : state.info) };
}

/** How long a workspace may take to answer `acp_open` before the view says the server may be too old for it. */
export const OPEN_WAIT_MS = 8000;

/** What the view says before the first answer arrives. A server older than the ACP view ignores `acp_open`. */
export function openingText(connected: boolean, silent: boolean): string {
  if (!connected) return 'Waiting for the connection…';
  return silent ? 'The server has not answered. If it is an older version, update it to see agent workspaces.' : 'Opening the workspace…';
}

/** An entry the reader has not been given before: newer than the stream when the view had its first full picture
 *  (`base`, or undefined while that picture is still arriving). Only these are announced to a screen reader. */
export const isNewEntry = (e: { rev: number }, base: number | undefined): boolean => base !== undefined && e.rev > base;

/** The name of the plain tmux workspace beside an ACP workspace (decision 0109). A long name is shortened with a hash of
 *  the whole, so two long names that share a start do not share a shell. */
export function shellName(name: string): string {
  if (name.length <= 58) return `${name}-shell`;
  let h = 2166136261;
  for (const c of name) h = Math.imul(h ^ c.codePointAt(0)!, 16777619) >>> 0;
  return `${name.slice(0, 49)}-${h.toString(36).slice(0, 6)}-shell`;
}

/** What to send to show a workspace: the revision held (if any) so only the difference comes back. */
export const openMessage = (workspace: string, held?: StreamState): ClientMessage =>
  held && held.epoch ? { type: 'acp_open', session: workspace, after: held.rev, epoch: held.epoch } : { type: 'acp_open', session: workspace };

// ---- What the view derives

/** Tools whose permission is open: their diff is shown once, in the permission. */
export function toolsAsking(entries: Entry[]): Set<string> {
  return new Set(entries.filter((e): e is Extract<Entry, { kind: 'permission' }> => e.kind === 'permission' && e.state === 'open').map((e) => e.toolId));
}

export function planProgress(items: { status: string }[]): { done: number; total: number } {
  return { done: items.filter((i) => i.status === 'completed').length, total: items.length };
}

/** Context use as a whole percentage, from `used / size` (every agent that sends usage means it so). */
export function contextPercent(u: Info['usage']): number | null {
  return u && u.size > 0 ? Math.min(100, Math.round((u.used / u.size) * 100)) : null;
}

export const baseName = (path: string) => path.split('/').filter(Boolean).pop() ?? path;

// ---- Search and copy

const blocksCache = new Map<string, Block[]>();
/** The Markdown tree of a reply, remembered by its text: a streaming reply is parsed again only when it grows. */
export function blocksOf(text: string): Block[] {
  let b = blocksCache.get(text);
  if (!b) {
    b = parseBlocks(text);
    if (blocksCache.size > 64) blocksCache.delete(blocksCache.keys().next().value as string);
    blocksCache.set(text, b);
  }
  return b;
}

/** The text runs of an entry, in drawing order: what search looks through and counts. `asking` is a tool whose
 *  permission is open: its diff is drawn in the permission, not here. A permission that is open is a card of
 *  buttons, not log text; once answered it is one line. */
export function entryRuns(e: Entry, asking = false): string[] {
  switch (e.kind) {
    case 'message': return runsOf(blocksOf(e.text));
    case 'user': case 'thought': case 'notice': return [e.text];
    case 'tool': return [e.title, ...e.locations.map((l) => baseName(l.path)), ...(asking ? [] : e.diffs).flatMap((d) => [d.path, ...diffRows(d).filter((r) => r.t !== 'gap').map((r) => (r as { text: string }).text)])];
    case 'plan': return e.items.map((i) => i.text);
    case 'permission': return e.state === 'open' ? [] : [e.title];
  }
}

export interface Match { entry: number; index: number }
/** Every hit of `query` in the stream, in order. `index` counts hits inside the entry. */
export function search(entries: Entry[], query: string): Match[] {
  const q = query.trim();
  if (!q) return [];
  const out: Match[] = [];
  const asking = toolsAsking(entries);
  for (const e of entries) {
    const n = e.kind === 'message' ? countHits(blocksOf(e.text), q) : entryRuns(e, e.kind === 'tool' && asking.has(e.toolId)).reduce((a, r) => a + hitsIn(r, q).length, 0);
    for (let i = 0; i < n && out.length < 2000; i++) out.push({ entry: e.id, index: i });
  }
  return out;
}

/** The search marks of an entry's text runs (the same runs `entryRuns` lists, in the same order); `current` is the index
 *  of the hit to stand out among the entry's hits, or -1. */
export function entryMarks(e: Entry, query: string, current: number, asking = false): (Part[] | undefined)[] {
  let n = 0;
  return entryRuns(e, asking).map((run) => {
    const marks = partsOf(run, query, n, current);
    n += hitsIn(run, query).length;
    return marks;
  });
}

/** How many runs the diff contributes to `entryRuns`: its path, then each shown line. */
export const diffRunCount = (d: Pick<Diff, 'oldText' | 'newText'>): number => 1 + diffRows(d).filter((r) => r.t !== 'gap').length;

/** What the view says under a reply that was cut, and what Copy appends so the clipboard is not taken for the whole reply. */
export const TRUNCATED_NOTE = 'The reply is longer; the rest is not shown.';

/** The text a tap on Copy puts on the clipboard for one entry. */
export function copyText(e: Entry): string {
  switch (e.kind) {
    case 'message': case 'thought': return e.truncated ? `${e.text}\n[${TRUNCATED_NOTE}]` : e.text;
    case 'user': case 'notice': return e.text;
    case 'tool': return [e.title, ...e.diffs.map(diffText)].filter(Boolean).join('\n');
    case 'plan': return e.items.map((i) => `${i.status === 'completed' ? '[x]' : '[ ]'} ${i.text}`).join('\n');
    case 'permission': return [e.title, ...e.diffs.map(diffText), e.plan ?? ''].filter(Boolean).join('\n');
  }
}

/** The whole conversation as text, for Copy all. */
export function transcriptText(entries: Entry[]): string {
  return entries.filter((e) => e.kind !== 'thought').map((e) => (e.kind === 'user' ? `> ${e.text.replace(/\n/g, '\n> ')}` : copyText(e))).join('\n\n');
}

// ---- Diffs

export type DiffRow = { t: '+' | '-' | ' '; text: string } | { t: 'gap'; count: number };
const CONTEXT = 2;
const DIFF_LINES = 200;

/** A line diff of the two texts (the longest common subsequence of lines), unchanged runs folded to a gap row. A new
 *  file is all additions. Bounded: both sides are cut to 200 lines (the most the server keeps), so the table is at most 40 000 cells. */
export function diffRows(d: Pick<Diff, 'oldText' | 'newText'>): DiffRow[] {
  const hit = rowsCache.get(d);
  if (hit) return hit;
  const rows = computeRows(d);
  rowsCache.set(d, rows);
  return rows;
}
const rowsCache = new WeakMap<object, DiffRow[]>();

function computeRows(d: Pick<Diff, 'oldText' | 'newText'>): DiffRow[] {
  const a = d.oldText === undefined ? [] : d.oldText.split('\n');
  const b = d.newText.split('\n');
  if (d.oldText === undefined) return b.map((text) => ({ t: '+', text }));
  const n = Math.min(a.length, DIFF_LINES), m = Math.min(b.length, DIFF_LINES);
  const dp = Array.from({ length: n + 1 }, () => new Uint16Array(m + 1));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  const rows: { t: '+' | '-' | ' '; text: string }[] = [];
  let i = 0, j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) { rows.push({ t: ' ', text: a[i] }); i++; j++; }
    else if (dp[i + 1][j] >= dp[i][j + 1]) rows.push({ t: '-', text: a[i++] });
    else rows.push({ t: '+', text: b[j++] });
  }
  while (i < n) rows.push({ t: '-', text: a[i++] });
  while (j < m) rows.push({ t: '+', text: b[j++] });
  // Fold long unchanged runs.
  const out: DiffRow[] = [];
  for (let k = 0; k < rows.length; ) {
    if (rows[k].t !== ' ') { out.push(rows[k++]); continue; }
    let e = k;
    while (e < rows.length && rows[e].t === ' ') e++;
    const run = rows.slice(k, e);
    const head = k === 0 ? 0 : CONTEXT, tail = e === rows.length ? 0 : CONTEXT;
    if (run.length <= head + tail + 1) out.push(...run);
    else { out.push(...run.slice(0, head), { t: 'gap', count: run.length - head - tail }, ...run.slice(run.length - tail)); }
    k = e;
  }
  return out;
}

export function diffStat(d: Pick<Diff, 'oldText' | 'newText'>): { added: number; removed: number } {
  const rows = diffRows(d);
  return { added: rows.filter((r) => r.t === '+').length, removed: rows.filter((r) => r.t === '-').length };
}

export function diffText(d: Diff): string {
  return [`--- ${d.path}`, ...diffRows(d).map((r) => (r.t === 'gap' ? `@@ ${r.count} unchanged @@` : `${r.t}${r.text}`))].join('\n');
}

// ---- Status words

export const TOOL_STATUS: Record<string, { glyph: string; word: string }> = {
  pending: { glyph: '○', word: 'waiting' },
  in_progress: { glyph: '◐', word: 'running' },
  completed: { glyph: '✓', word: 'done' },
  failed: { glyph: '✕', word: 'failed' },
};
export const toolStatus = (s: string) => TOOL_STATUS[s] ?? TOOL_STATUS.pending;

/** How a finished permission reads in the log: what the owner chose, in the agent's words and in ours. */
export function permissionOutcome(e: Extract<Entry, { kind: 'permission' }>): string {
  if (e.state === 'cancelled') return 'Cancelled';
  const o = e.options.find((x) => x.id === e.chosen);
  if (!o) return 'Answered';
  return o.kind === 'allow_once' ? `Allowed once · ${o.name}` : `Rejected · ${o.name}`;
}
