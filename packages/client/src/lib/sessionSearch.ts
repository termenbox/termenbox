// Cross-session search (R78, decision 0156): one query, asked of every connected server, answered as a stream of hits
// read from the agents' own conversation files. This module keeps the state of one search: it numbers the search, drops
// answers of an older one, groups hits (by agent, workspace or not at all) and says why a search stopped early. The query goes to the servers
// and nowhere else: it is not written to storage, the breadcrumbs or any log. Texts from a server are shown as text only.
import type { ClientMessage, SearchDoneMessage, SearchHitMessage, ServerMessage, SessionResumedMessage, TabResumed } from './protocol';

export const MAX_QUERY = 512;
export const DEBOUNCE_MS = 250;
/** More than a server's own cap of 50; a stuck or hostile server cannot grow the list without end. */
export const MAX_HITS_PER_SERVER = 100;

export const NOTICE = 'What you type here is sent to your server, which reads the agents’ own conversation files to answer. It is not kept or written to any log.';

export interface Hit {
  serverId: string;
  serverName: string;
  workspace: string;
  tab: string;
  agent: string;
  /** False: the workspace is dormant and the hit offers Resume. */
  up: boolean;
  turn: string;
  kind: string;
  snippet: string;
  /** Where the session ran, from its file (decision 0167); empty from an older server. */
  dir: string;
  /** The conversation's id, for `resume_session`; empty from an older server. */
  session: string;
  /** [start, end) in characters of `snippet`, already clamped. */
  highlight: [number, number];
  at?: number;
}
export interface Group {
  key: string; serverId: string; serverName: string; workspace: string; name: string; agent: string; up: boolean; hits: Hit[];
  /** The full directory of a group named after one, shown as a tooltip. */
  title?: string;
}

export type Status = 'idle' | 'searching' | 'done';
export interface SearchState {
  query: string;
  status: Status;
  hits: Hit[];
  /** Servers that have not finished yet. */
  pending: string[];
  /** Why a server stopped early, once per distinct reason. */
  cuts: string[];
  /** Agents whose search is off, with the server's fixed reason. */
  off: { agent: string; reason: string }[];
  /** Servers that went away before they finished. */
  lost: number;
  /** Conversations the time window kept from being searched, summed over the servers. */
  older: number;
  /** The time window this search used. */
  range: RangeId;
}
export const IDLE: SearchState = { query: '', status: 'idle', hits: [], pending: [], cuts: [], off: [], lost: 0, older: 0, range: '30d' };

export type RangeId = '24h' | '7d' | '30d' | 'any';
export const RANGES: { id: RangeId; label: string; ms: number | null }[] = [
  { id: '24h', label: '24 h', ms: 86_400_000 }, { id: '7d', label: '7 d', ms: 7 * 86_400_000 },
  { id: '30d', label: '30 d', ms: 30 * 86_400_000 }, { id: 'any', label: 'Any', ms: null },
];
/** The `since` a search of `range` sends, or undefined for any age. */
export const sinceOf = (range: RangeId, now: number): number | undefined => { const ms = RANGES.find((r) => r.id === range)?.ms; return ms == null ? undefined : now - ms; };
const RANGE_KEY = 'termenbox_search_range';
/** The window this viewer chose last; 30 days when nothing usable is stored. */
export function loadRange(store: Pick<Storage, 'getItem'> | undefined = groupStore()): RangeId {
  try {
    const v = store?.getItem(RANGE_KEY);
    return RANGES.some((r) => r.id === v) ? (v as RangeId) : '30d';
  } catch { return '30d'; }
}
export function saveRange(range: RangeId, store: Pick<Storage, 'setItem'> | undefined = groupStore()) {
  try { store?.setItem(RANGE_KEY, range); } catch { /* Private mode: the choice lasts until the sheet closes. */ }
}
export function olderText(n: number): string { return `${n} older ${n === 1 ? 'conversation' : 'conversations'} not searched.`; }

export interface SearchDeps {
  /** The servers to ask: the connected ones. */
  servers: () => { id: string; name: string }[];
  send: (serverId: string, message: ClientMessage) => void;
  onchange: (state: SearchState) => void;
  /** The clock, for the window's start. */
  now?: () => number;
  /** Where the window is remembered; defaults to the browser's local storage. */
  store?: Pick<Storage, 'getItem' | 'setItem'>;
}

/** Control characters out, the server's cap on length applied, outer space trimmed. */
export function cleanQuery(text: string): string {
  // eslint-disable-next-line no-control-regex
  let q = text.replace(/[\u0000-\u001f\u007f]/g, ' ').trim();
  while (new TextEncoder().encode(q).length > MAX_QUERY) q = q.slice(0, -1);
  return q;
}

function clamp(h: [number, number] | undefined, length: number): [number, number] {
  const a = Array.isArray(h) ? h : [0, 0];
  const start = Math.min(Math.max(0, Number.isFinite(a[0]) ? Math.trunc(a[0]) : 0), length);
  const end = Math.min(Math.max(start, Number.isFinite(a[1]) ? Math.trunc(a[1]) : start), length);
  return [start, end];
}
function time(at: string | undefined): number | undefined {
  if (typeof at !== 'string') return undefined;
  const t = Date.parse(at);
  return Number.isFinite(t) ? t : undefined;
}

/** The sentence for a stop reason; `cancelled` is the user's own doing and says nothing. */
export function cutText(cut: string): string | null {
  if (cut === 'cancelled' || cut === 'range') return null;
  if (cut === 'hits') return 'Search was cut: it stopped at the first matches. Narrow the words to see others.';
  if (cut === 'time' || cut === 'bytes') return 'Search was cut: it stopped early to stay fast, so some older conversations were not searched.';
  return 'Search was cut before it looked everywhere.';
}
export function offText(o: { agent: string; reason: string }): string {
  return `Search is off for ${o.agent}: ${o.reason}`;
}

export class SessionSearch {
  private id = 0;
  private state: SearchState = IDLE;
  private names = new Map<string, string>();
  private range: RangeId;
  constructor(private deps: SearchDeps) { this.range = loadRange(deps.store); this.state = { ...IDLE, range: this.range }; }

  get current(): SearchState { return this.state; }

  private set(s: SearchState) { this.state = { ...s, range: this.range }; this.deps.onchange(this.state); }

  /** Changes the time window, remembers it, and asks again if a query is showing. */
  setRange(range: RangeId) {
    if (range === this.range) return;
    const query = this.state.query;
    this.range = range;
    saveRange(range, this.deps.store);
    if (query) this.run(query); else this.set(this.state);
  }

  /** Stops what is running (the servers are told) and starts a search for `text`, or goes idle when it is empty. */
  run(text: string) {
    this.stop();
    const query = cleanQuery(text);
    if (!query) return this.set(IDLE);
    const servers = this.deps.servers();
    this.id++;
    this.names = new Map(servers.map((s) => [s.id, s.name]));
    this.set({ ...IDLE, query, status: servers.length ? 'searching' : 'done', pending: servers.map((s) => s.id) });
    const since = sinceOf(this.range, (this.deps.now ?? Date.now)());
    for (const s of servers) this.deps.send(s.id, { type: 'search', id: this.id, query, ...(since === undefined ? {} : { since }) });
  }

  /** Tells the servers that still work to stop. The hits so far stay. */
  stop() {
    if (this.state.status !== 'searching') return;
    for (const sid of this.state.pending) this.deps.send(sid, { type: 'search', id: this.id, query: '', cancel: true });
    this.set({ ...this.state, status: 'done', pending: [] });
  }

  /** Closing the sheet: stop, and forget the query and every hit. */
  close() {
    this.stop();
    this.id++;
    this.set(IDLE);
  }

  /** A server that dropped its connection will not finish. */
  lost(serverId: string) {
    if (!this.state.pending.includes(serverId)) return;
    this.finish(serverId, (s) => ({ ...s, lost: s.lost + 1 }));
  }

  handle(serverId: string, msg: ServerMessage) {
    if (msg.type === 'search_hit') this.hit(serverId, msg);
    else if (msg.type === 'search_done') this.done(serverId, msg);
  }

  private hit(serverId: string, m: SearchHitMessage) {
    if (m.id !== this.id || this.state.status !== 'searching' || !this.state.pending.includes(serverId)) return;
    if (this.state.hits.filter((h) => h.serverId === serverId).length >= MAX_HITS_PER_SERVER) return;
    const snippet = typeof m.snippet === 'string' ? m.snippet : '';
    const hit: Hit = {
      serverId, serverName: this.names.get(serverId) ?? 'Server', workspace: m.workspace, tab: m.tab ?? '', agent: m.agent, up: !!m.up,
      turn: m.turn, kind: m.kind, snippet, dir: typeof m.dir === 'string' ? m.dir : '', session: typeof m.session === 'string' ? m.session : '', highlight: clamp(m.highlight, snippet.length), at: time(m.at),
    };
    this.set({ ...this.state, hits: [...this.state.hits, hit] });
  }

  private done(serverId: string, m: SearchDoneMessage) {
    if (m.id !== this.id || !this.state.pending.includes(serverId)) return;
    this.finish(serverId, (s) => {
      const cut = m.cut ? cutText(m.cut) : null;
      const off = (Array.isArray(m.off) ? m.off : []).filter((o) => !s.off.some((x) => x.agent === o.agent && x.reason === o.reason));
      const older = s.older + (Number.isFinite(m.older) && m.older! > 0 ? Math.trunc(m.older!) : 0);
      return { ...s, older, cuts: cut && !s.cuts.includes(cut) ? [...s.cuts, cut] : s.cuts, off: [...s.off, ...off] };
    });
  }

  private finish(serverId: string, edit: (s: SearchState) => SearchState) {
    const next = edit({ ...this.state, pending: this.state.pending.filter((p) => p !== serverId) });
    this.set({ ...next, status: next.pending.length ? 'searching' : 'done' });
  }
}

/** The last two segments of a directory, for a heading; the full path goes in a tooltip. */
export function dirLabel(dir: string): string {
  const segs = dir.split('/').filter(Boolean);
  return segs.length ? segs.slice(-2).join('/') : 'Unknown folder';
}

export type GroupMode = 'agent' | 'workspace' | 'none';
export const GROUP_MODES: { mode: GroupMode; label: string }[] = [{ mode: 'agent', label: 'Agent' }, { mode: 'workspace', label: 'Workspace' }, { mode: 'none', label: 'None' }];
const GROUP_KEY = 'termenbox_search_grouping';
const groupStore = (): Storage | undefined => {
  try { return typeof localStorage === 'undefined' ? undefined : localStorage; } catch { return undefined; }
};
/** The grouping this viewer chose last; workspace when nothing usable is stored. */
export function loadGroupMode(store: Pick<Storage, 'getItem'> | undefined = groupStore()): GroupMode {
  try {
    const v = store?.getItem(GROUP_KEY);
    return GROUP_MODES.some((m) => m.mode === v) ? (v as GroupMode) : 'workspace';
  } catch { return 'workspace'; }
}
export function saveGroupMode(mode: GroupMode, store: Pick<Storage, 'setItem'> | undefined = groupStore()) {
  try { store?.setItem(GROUP_KEY, mode); } catch { /* Private mode: the choice lasts until the sheet closes. */ }
}

/**
 * Hits grouped by `mode`, the group with the newest hit first, the newest hit first inside it. `workspace` groups by
 * workspace (`nameOf` gives its current name), `agent` by agent per server, `none` is one flat list with no header.
 */
export function groupHits(hits: Hit[], mode: GroupMode, nameOf: (serverId: string, workspace: string) => string | undefined): Group[] {
  const by = new Map<string, Group>();
  for (const h of hits) {
    // A conversation no workspace holds is grouped under the directory it ran in.
    const folder = mode === 'workspace' && !h.workspace;
    const key = folder ? `${h.serverId}/dir:${h.dir}` : mode === 'workspace' ? `${h.serverId}/${h.workspace}` : mode === 'agent' ? `${h.serverId}/agent:${h.agent}` : 'all';
    let g = by.get(key);
    if (!g) {
      const name = folder ? dirLabel(h.dir) : mode === 'workspace' ? nameOf(h.serverId, h.workspace) ?? h.workspace : mode === 'agent' ? h.agent : '';
      g = { key, serverId: h.serverId, serverName: h.serverName, workspace: mode === 'workspace' ? h.workspace : '', name, agent: h.agent, up: h.up, hits: [], ...(folder && h.dir ? { title: h.dir } : {}) };
      by.set(key, g);
    }
    g.hits.push(h);
  }
  const newest = (g: Group) => g.hits.reduce((m, h) => Math.max(m, h.at ?? 0), 0);
  for (const g of by.values()) g.hits.sort((a, b) => (b.at ?? 0) - (a.at ?? 0));
  return [...by.values()].sort((a, b) => newest(b) - newest(a));
}

/** The snippet cut into the text before, the match and the text after, for a highlight that needs no HTML. */
export function parts(h: Pick<Hit, 'snippet' | 'highlight'>): { before: string; match: string; after: string } {
  const [a, b] = h.highlight;
  return { before: h.snippet.slice(0, a), match: h.snippet.slice(a, b), after: h.snippet.slice(b) };
}

export function whenText(at: number | undefined, now: number, locale?: string): string {
  if (at === undefined) return '';
  const days = Math.floor((now - at) / 86_400_000);
  if (days <= 0) return new Date(at).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' });
  if (days === 1) return 'Yesterday';
  return new Date(at).toLocaleDateString(locale, { day: 'numeric', month: 'short' });
}

/** Runs `fn` with the last value once typing has paused for `ms`; `flush` runs it now (Enter), `cancel` drops it. */
export function debounced(fn: (text: string) => void, ms = DEBOUNCE_MS) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let last = '';
  return {
    push(text: string) { last = text; clearTimeout(timer); timer = setTimeout(() => { timer = undefined; fn(last); }, ms); },
    flush() { if (timer === undefined) return; clearTimeout(timer); timer = undefined; fn(last); },
    cancel() { clearTimeout(timer); timer = undefined; },
  };
}

// ---- What a tap on a hit does (decision 0156). The workspace is up: a terminal tab is switched to and nothing more (the
// agent's own screen cannot be scrolled from outside, fact 0065), an ACP workspace is switched to and scrolled to the
// turn. The workspace is down: Resume is a separate, explicit step, and nothing is typed into an agent by the client.
export type Landing = 'switch' | 'jump' | 'resume';
export const landingOf = (h: Pick<Hit, 'up' | 'tab'>): Landing => (!h.up ? 'resume' : h.tab ? 'switch' : 'jump');

/** What to tell after a Resume, and whether the view should now go to the turn. A tab that came back as a shell is said
 *  to be one, and the hit is not claimed to be shown: the match lives in the agent's file, not on that screen. */
export function resumeOutcome(h: Pick<Hit, 'tab' | 'agent'>, said: TabResumed[] | undefined): { message: string; jump: boolean; ok: boolean } {
  if (!h.tab) return { message: 'Workspace reopened.', jump: true, ok: true };
  const r = said?.find((x) => x.tab === h.tab);
  if (r?.outcome === 'resumed') return { message: `Resuming ${h.agent} in this tab.`, jump: false, ok: true };
  if (r) return { message: `Could not resume ${h.agent}: ${r.reason ?? 'no reason given'}. The tab is a shell again.`, jump: false, ok: false };
  return { message: 'Workspace reopened. This server did not say whether the conversation was resumed.', jump: false, ok: false };
}

/** True when no workspace holds the hit's conversation: Resume creates one in the hit's directory (decision 0167). */
export const isUnheld = (h: Pick<Hit, 'workspace'>): boolean => !h.workspace;

/** What to tell after `resume_session`, and whether the view should now go to the workspace the server named. */
export function sessionResumedOutcome(h: Pick<Hit, 'agent'>, r: Pick<SessionResumedMessage, 'outcome' | 'reason' | 'workspace'>): { message: string; go: boolean; ok: boolean } {
  if (r.outcome === 'missing') return { message: 'The folder or the conversation is gone, so it cannot be resumed.', go: false, ok: false };
  if (!r.workspace) return { message: 'This server did not say where the conversation went.', go: false, ok: false };
  if (r.outcome === 'switched') return { message: 'Switched to the workspace that holds this conversation.', go: true, ok: true };
  if (r.outcome === 'resumed') return { message: `Resuming ${h.agent} in a new workspace.`, go: true, ok: true };
  if (r.outcome === 'shell') return { message: `Could not resume ${h.agent}: ${r.reason ?? 'no reason given'}. The new workspace is a shell.`, go: true, ok: false };
  return { message: 'This server answered in a way this app does not know.', go: false, ok: false };
}

/** What the Resume confirmation says: a dormant workspace is reopened, a conversation no workspace holds gets a new one. */
export function resumeAskText(h: Pick<Hit, 'workspace' | 'agent' | 'dir'>): string {
  return isUnheld(h)
    ? `No workspace holds this conversation. Resume opens a new workspace in ${h.dir ? dirLabel(h.dir) : 'its folder'} and starts ${h.agent} again with it.`
    : `This workspace is not running. Resume opens it and starts ${h.agent} again with this conversation.`;
}
