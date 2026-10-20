import { derived, writable } from 'svelte/store';
import { Connection, type ConnectionOptions, type ConnState } from './connection';
import { applyActivity, applyAttention, applyEvents, applySnapshot, syncRequest, type AgentMap } from './agents';
import type { ClientMessage, ServerListItem, ServerMessage, SessionInfo } from './protocol';
import type { Target } from './transport';
import { applyWindowList, forgetWindows, type WindowMap } from './windows';

export type HubConnection = Pick<Connection, 'start' | 'close' | 'subscribe' | 'send'> & Partial<Pick<Connection, 'kind' | 'rtt' | 'drops' | 'retry' | 'retryNow'>>;
export type ServerState = ConnState | 'paused';
export interface HubServer {
  id: string;
  server: ServerListItem;
  state: ServerState;
  reason: string;
  version: string;
  backends: string[];
  /** The agents the owner allows as ACP sessions (T0); empty when the server runs none. */
  acpAgents: string[];
  /** Live window updates run (tmux 3.2+); false shows a notice in the server's status. */
  controlMode: boolean;
  tmuxVersion: string;
  /** tmux's prefix key (`C-b`); empty without tmux. */
  prefix: string;
}
export interface HubSession extends SessionInfo { serverId: string; key: string; serverName: string }
interface Entry extends HubServer {
  connection: HubConnection | null;
  unsubscribe?: () => void;
  sessions: SessionInfo[];
  agents: AgentMap;
  windows: WindowMap;
  refresh: boolean;
  everReady: boolean;
}
interface Options {
  credentials: ConnectionOptions['credentials'];
  factory?: (opts: ConnectionOptions) => HubConnection;
  target?: (server: ServerListItem, refresh: boolean) => Promise<Target>;
  onSaved?: (server: ServerListItem) => void;
  onReady?: (id: string, msg: Extract<ServerMessage, { type: 'auth_ok' }>, reconnect: boolean) => void;
  onMessage?: (id: string, msg: ServerMessage) => void;
  onDrop?: (id: string, reason: string, upMs: number) => void;
  onFatal?: (id: string, reason: string) => void;
}

export const serverKey = (server: ServerListItem) => server.server_id || server.server_url;
export const sessionKey = (serverId: string, sessionId: string) => `${serverId}/${sessionId}`;

/** A usable selection wins; otherwise the owner chooses among available peers. */
export function sessionDestinations(servers: HubServer[], selected?: HubServer): HubServer[] {
  return selected?.state === 'connected' ? [selected] : servers.filter(s => s.state === 'connected');
}

/**
 * Where a new session can go: every connected server, with the one to preselect. `preferredId` is the server whose own
 * "+ New" was tapped, or else the open one; with several servers the sheet asks, so a plain "+ New" never picks silently.
 */
export function newSessionTargets(servers: HubServer[], preferredId?: string): { servers: HubServer[]; serverId: string } {
  const connected = servers.filter(s => s.state === 'connected');
  return { servers: connected, serverId: (connected.find(s => s.id === preferredId) ?? connected[0])?.id ?? '' };
}

/** Owns connections for the device's saved servers. Only user actions pause/remove them; no cap drops a peer. */
export class ServerHub {
  private entries = new Map<string, Entry>();
  readonly servers = writable<HubServer[]>([]);
  readonly allSessions = writable<HubSession[]>([]);
  readonly agents = writable<AgentMap>({});
  readonly windows = writable<WindowMap>({});
  readonly connectedCount = derived(this.servers, servers => servers.filter(s => s.state === 'connected').length);
  readonly overLimit = derived(this.connectedCount, count => count > 6);

  constructor(private opts: Options) {}

  start(servers: ServerListItem[]) { for (const server of servers) this.add(server); }
  add(server: ServerListItem) {
    const id = serverKey(server);
    const existing = this.entries.get(id);
    if (existing) {
      const changed = existing.server.claim !== server.claim || existing.server.server_url !== server.server_url;
      existing.server = server;
      if (changed && existing.state !== 'paused') { this.disconnect(existing); this.connect(existing); }
      else this.publish();
      return;
    }
    const entry: Entry = { id, server, state: 'connecting', reason: '', version: '', backends: [], acpAgents: [], controlMode: false, tmuxVersion: '', prefix: '', connection: null,
      sessions: [], agents: {}, windows: {}, refresh: !server.record && !!server.server_id, everReady: false };
    this.entries.set(id, entry);
    this.connect(entry);
  }
  connection(id: string) { return this.entries.get(id)?.connection ?? null; }
  send(id: string, message: ClientMessage) { this.connection(id)?.send(message); }
  /** Global keys stay in the UI; the wire always carries the original server-local session id. */
  sendToSession(key: string, message: ClientMessage) {
    const session = this.findSession(key);
    if (!session) return;
    const wire = 'session' in message ? { ...message, session: session.id }
      : 'session_id' in message ? { ...message, session_id: session.id } : message;
    this.send(session.serverId, wire);
  }
  findSession(key: string): HubSession | undefined {
    for (const e of this.entries.values()) {
      if (e.state === 'paused') continue;
      const s = e.sessions.find(s => sessionKey(e.id, s.id) === key);
      if (s) return { ...s, serverId: e.id, key, serverName: e.server.server_name || 'Server' };
    }
  }
  pause(id: string) {
    const e = this.entries.get(id); if (!e || e.state === 'paused') return;
    this.disconnect(e); e.state = 'paused'; e.reason = 'Paused by you'; this.publish();
  }
  resume(id: string) { const e = this.entries.get(id); if (e?.state === 'paused') this.connect(e); }
  retry(id: string) { const e = this.entries.get(id); if (e && e.state !== 'paused') { this.disconnect(e); this.connect(e); } }
  remove(id: string) {
    const e = this.entries.get(id); if (!e) return;
    this.disconnect(e); this.entries.delete(id); this.publish();
  }
  close() { for (const id of [...this.entries.keys()]) this.remove(id); }

  private disconnect(e: Entry) {
    const c = e.connection;
    e.connection = null; // invalidate callbacks before close(), including late async auth
    e.unsubscribe?.(); e.unsubscribe = undefined;
    c?.close();
  }
  private connect(e: Entry) {
    e.state = 'connecting'; e.reason = '';
    const current = () => e.connection === connection;
    const connection = (this.opts.factory ?? (opts => new Connection(opts)))({
      target: () => this.opts.target ? this.opts.target(e.server, e.refresh)
        : Promise.resolve({ record: e.server.record, wsUrl: e.server.server_url || undefined }),
      credentials: this.opts.credentials, claim: e.server.claim, serverId: e.server.server_id,
      onState: (state, detail) => {
        if (!current()) return;
        e.state = state;
        if (detail !== undefined) e.reason = detail;
        if (state === 'connected') e.reason = '';
        if (state === 'connected') e.refresh = false;
        else if (state === 'unreachable' || state === 'reconnecting') e.refresh = true;
        this.publish();
      },
      onReady: (msg, reconnect) => {
        if (!current()) return;
        const restoring = reconnect || e.everReady;
        e.everReady = true;
        const { claim, ...saved } = e.server;
        e.server = { ...saved, server_name: saved.server_name || msg.server_name };
        e.version = msg.version ?? ''; e.backends = msg.backends ?? []; e.acpAgents = msg.acp_agents ?? [];
        e.controlMode = !!msg.control_mode; e.tmuxVersion = msg.tmux_version ?? ''; e.prefix = msg.prefix ?? '';
        this.publish(); this.opts.onSaved?.(e.server);
        connection.send({ type: 'list_sessions' });
        connection.send({ type: 'agent_sync', after: syncRequest(e.agents) });
        this.opts.onReady?.(e.id, msg, restoring);
      },
      onDrop: (reason, upMs) => {
        if (!current()) return;
        e.reason = reason; this.publish(); this.opts.onDrop?.(e.id, reason, upMs);
      },
      onFatal: reason => { if (current()) { e.state = 'closed'; e.reason = reason; this.publish(); this.opts.onFatal?.(e.id, reason); } },
    });
    e.connection = connection;
    e.unsubscribe = connection.subscribe(msg => {
      if (!current()) return;
      const changed = msg.type === 'sessions' || msg.type === 'agent_snapshot' || msg.type === 'agent_events' || msg.type === 'window_list' || msg.type === 'activity' || msg.type === 'attention';
      if (msg.type === 'sessions') { e.sessions = msg.sessions; e.windows = forgetWindows(e.windows, msg.sessions.map(s => s.id)); }
      if (msg.type === 'window_list') e.windows = applyWindowList(e.windows, msg);
      if (msg.type === 'agent_snapshot') e.agents = applySnapshot(e.agents, msg.states);
      if (msg.type === 'activity') e.agents = applyActivity(e.agents, msg.sessions, Date.now());
      if (msg.type === 'attention') e.agents = applyAttention(e.agents, msg.items);
      if (msg.type === 'agent_events') {
        const result = applyEvents(e.agents, msg.events); e.agents = result.map;
        if (result.gap) connection.send({ type: 'agent_sync', after: syncRequest(e.agents) });
      }
      if (changed) this.publish();
      this.opts.onMessage?.(e.id, msg);
    });
    this.publish(); connection.start();
  }
  private publish() {
    const entries = [...this.entries.values()];
    this.servers.set(entries.map(({ id, server, state, reason, version, backends, acpAgents, controlMode, tmuxVersion, prefix }) => ({ id, server, state, reason, version, backends, acpAgents, controlMode, tmuxVersion, prefix })));
    this.allSessions.set(entries.filter(e => e.state !== 'paused').flatMap(e => e.sessions.map(s => ({ ...s,
      serverId: e.id, serverName: e.server.server_name || 'Server', key: sessionKey(e.id, s.id) }))));
    this.agents.set(Object.fromEntries(entries.filter(e => e.state !== 'paused').flatMap(e => Object.entries(e.agents).map(([id, a]) => [sessionKey(e.id, id), a]))));
    this.windows.set(Object.fromEntries(entries.filter(e => e.state !== 'paused').flatMap(e => Object.entries(e.windows).map(([id, w]) => [sessionKey(e.id, id), w]))));
  }
}
