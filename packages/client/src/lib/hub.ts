import { derived, writable } from 'svelte/store';
import { Connection, type ConnectionOptions, type ConnState } from './connection';
import { applyActivity, applyAttention, applyEvents, applySnapshot, syncRequest, type AgentMap } from './agents';
import type { AttentionDay, ClientMessage, ServerHealth, ServerListItem, ServerMessage, TabAgentState, UpdateInfo, WorkspaceInfo } from './protocol';
import { CHECK_WAIT_MS, checkAnswered, type UpdateAction } from './updateState';
import type { Target } from './transport';
import { storedBundle, type PathFailure } from './paths';
import { BundleRejected, decodeBundle, verifyBundle } from './bundle';
import { orderVersion, orderWorkspaces } from './order';
import { applyWindowList, forgetWindows, type WindowMap } from './windows';

export type HubConnection = Pick<Connection, 'start' | 'close' | 'subscribe' | 'send'> & Partial<Pick<Connection, 'kind' | 'pathLabel' | 'rtt' | 'drops' | 'retry' | 'retryNow'>>;
export type ServerState = ConnState | 'paused';
export interface HubServer {
  id: string;
  server: ServerListItem;
  state: ServerState;
  reason: string;
  version: string;
  /** A newer release the server could install but has not (`auth_ok.update_available`); empty when up to date. */
  updateAvailable?: string;
  /** What the updater knows (`update_state`, decision 0154); absent from an older server. */
  update?: UpdateInfo;
  /** The update actions this server's helper accepts (`auth_ok.update_capabilities`); empty from an older server. */
  updateCapabilities?: string[];
  /** A check was asked for and the updater has not recorded its outcome yet. */
  checking?: boolean;
  /** The owner asked this server to update; it restarts when done, and the reconnect brings the new version. */
  updating?: boolean;
  backends: string[];
  /** The server serves workspace tabs and the tab messages. */
  tabs?: boolean;
  /** The agents the owner allows as ACP workspaces (T0); empty when the server runs none. */
  acpAgents: string[];
  /** Live window updates run (tmux 3.2+); false shows a notice in the server's status. */
  controlMode: boolean;
  tmuxVersion: string;
  /** tmux's prefix key (`C-b`); empty without tmux. */
  prefix: string;
  /** Server health (U14). */
  health?: ServerHealth;
  /** The path carrying the workspace (`Direct, LAN`, `Tunnel`); empty while there is none. */
  path?: string;
  /** The attention meter's daily buckets (decision 0169), oldest first; absent from an older server. */
  attentionStats?: AttentionDay[];
  /** Every path tried in the last failed attempt, with why and one fix (decision 0123). */
  failures?: PathFailure[];
}
export interface HubWorkspace extends WorkspaceInfo { serverId: string; key: string; serverName: string }
interface Entry extends HubServer {
  updateFrom?: string;
  checkBaseline?: number;
  checkTimer?: ReturnType<typeof setTimeout>;
  updateTimer?: ReturnType<typeof setTimeout>;
  connection: HubConnection | null;
  unsubscribe?: () => void;
  workspaces: WorkspaceInfo[];
  agents: AgentMap;
  windows: WindowMap;
  tabStates: Record<string, TabAgentState>;
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
export const workspaceKey = (serverId: string, sessionId: string) => `${serverId}/${sessionId}`;

/** Release SemVer precedence; build metadata never proves that a newer release was installed. */
function newerRelease(next: string, previous: string): boolean {
  const parse = (v: string) => /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(v);
  const a = parse(next), b = parse(previous);
  if (!a || !b) return false;
  for (let i = 1; i <= 3; i++) { if (+a[i] !== +b[i]) return +a[i] > +b[i]; }
  if (!a[4] || !b[4]) return !!b[4] && !a[4];
  const left = a[4].split('.'), right = b[4].split('.');
  for (let i = 0; i < Math.max(left.length, right.length); i++) {
    if (left[i] === undefined || right[i] === undefined) return right[i] === undefined;
    if (left[i] === right[i]) continue;
    const an = /^\d+$/.test(left[i]), bn = /^\d+$/.test(right[i]);
    if (an && bn) return left[i].length !== right[i].length ? left[i].length > right[i].length : left[i] > right[i];
    return an !== bn ? !an : left[i] > right[i];
  }
  return false;
}

/** A usable selection wins; otherwise the owner chooses among available peers. */
export function workspaceDestinations(servers: HubServer[], selected?: HubServer): HubServer[] {
  return selected?.state === 'connected' ? [selected] : servers.filter(s => s.state === 'connected');
}

/**
 * Where a new workspace can go: every connected server, with the one to preselect. `preferredId` is the server whose own
 * "+ New" was tapped, or else the open one; with several servers the sheet asks, so a plain "+ New" never picks silently.
 */
export function newWorkspaceTargets(servers: HubServer[], preferredId?: string): { servers: HubServer[]; serverId: string } {
  const connected = servers.filter(s => s.state === 'connected');
  return { servers: connected, serverId: (connected.find(s => s.id === preferredId) ?? connected[0])?.id ?? '' };
}

/** Owns connections for the device's saved servers. Only user actions pause/remove them; no cap drops a peer. */
export class ServerHub {
  private entries = new Map<string, Entry>();
  readonly servers = writable<HubServer[]>([]);
  readonly allWorkspaces = writable<HubWorkspace[]>([]);
  readonly agents = writable<AgentMap>({});
  readonly windows = writable<WindowMap>({});
  readonly tabStates = writable<Record<string, TabAgentState>>({});
  readonly connectedCount = derived(this.servers, servers => servers.filter(s => s.state === 'connected').length);
  readonly overLimit = derived(this.connectedCount, count => count > 6);

  constructor(private opts: Options) {
    // The user's own order of workspaces (lib/order.ts) changed: list again, in the new order.
    let seen: number | undefined;
    orderVersion.subscribe(n => { if (seen !== undefined && n !== seen) this.publish(); seen = n; });
  }

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
    const entry: Entry = { id, server, state: 'connecting', reason: '', version: '', updateAvailable: '', updating: false, backends: [], tabs: false, acpAgents: [], controlMode: false, tmuxVersion: '', prefix: '', connection: null, path: '', failures: [],
      workspaces: [], agents: {}, windows: {}, tabStates: {}, refresh: !server.record && !server.bundle && !!server.server_id, everReady: false };
    this.entries.set(id, entry);
    this.connect(entry);
  }
  connection(id: string) { return this.entries.get(id)?.connection ?? null; }
  /** This device's last round trip to a server, from its heartbeat; null before the first one. */
  rtt(id: string): number | null { return this.entries.get(id)?.connection?.rtt ?? null; }
  send(id: string, message: ClientMessage) { this.connection(id)?.send(message); }
  /** A tab message for the workspace a device shows: the wire carries the server-local workspace id. */
  sendTab(key: string, message: ClientMessage) { this.sendToWorkspace(key, message); }
  newTab(key: string, template: string, label = '', dir = '') {
    this.sendToWorkspace(key, { type: 'new_tab', workspace: key, template, label, dir });
  }
  renameTab(key: string, tab: string, label: string) {
    this.sendToWorkspace(key, { type: 'rename_tab', workspace: key, tab, label });
  }
  closeTab(key: string, tab: string) {
    this.sendToWorkspace(key, { type: 'close_tab', workspace: key, tab });
  }
  moveTab(key: string, tab: string, index: number) {
    this.sendToWorkspace(key, { type: 'move_tab', workspace: key, tab, index });
  }
  focusTab(key: string, tab: string) {
    this.sendToWorkspace(key, { type: 'focus_tab', workspace: key, tab });
  }
  /** The server's own order; every device follows the broadcast. */
  moveWorkspace(key: string, index: number) {
    const found = this.findWorkspace(key);
    if (found) this.send(found.serverId, { type: 'move_workspace', workspace: found.id, index });
  }
  /** Global keys stay in the UI; the wire always carries the original server-local workspace id. */
  sendToWorkspace(key: string, message: ClientMessage) {
    const workspace = this.findWorkspace(key);
    if (!workspace) return;
    const wire = 'workspace' in message ? { ...message, workspace: workspace.id }
      : 'workspace_id' in message ? { ...message, workspace_id: workspace.id } : message;
    this.send(workspace.serverId, wire);
  }
  /** Shows the new name at once and asks the server. The server's list confirms it for every device; a refusal is
   *  put right by asking for the list again, which brings the old name back. */
  rename(key: string, name: string) {
    const found = this.findWorkspace(key);
    if (!found) return;
    const e = this.entries.get(found.serverId);
    if (e) { e.workspaces = e.workspaces.map(s => (s.id === found.id ? { ...s, name } : s)); this.publish(); }
    this.sendToWorkspace(key, { type: 'rename_workspace', workspace: key, name });
    setTimeout(() => this.send(found.serverId, { type: 'list_workspaces' }), 2000);
  }
  /** Asks the server to install its newer release; the card shows "Updating" until the server comes back. */
  updateServer(id: string) {
    const e = this.entries.get(id);
    if (!e || e.state !== 'connected' || e.updating || !e.updateAvailable) return;
    this.beginUpdate(e);
    try { this.send(id, { type: 'update_server' }); }
    catch { this.finishUpdate(e, 'Server update: The request could not be sent.'); }
  }
  /** Whether this server's helper accepts `action`; a client offers no button an older helper would reject. */
  canUpdate(id: string, action: UpdateAction): boolean {
    const e = this.entries.get(id);
    return !!e && e.state === 'connected' && !!e.updateCapabilities?.includes(action);
  }
  /** Asks for a check without installing; `checking` holds until the updater records an attempt or the wait ends. */
  checkUpdate(id: string) {
    const e = this.entries.get(id);
    if (!e || e.checking || !this.canUpdate(id, 'check')) return;
    e.checkBaseline = e.update?.checked_at ?? 0;
    e.checking = true;
    clearTimeout(e.checkTimer);
    e.checkTimer = setTimeout(() => this.endCheck(e), CHECK_WAIT_MS);
    this.publishServers();
    try { this.send(id, { type: 'update_check' }); }
    catch { this.endCheck(e); }
  }
  private endCheck(e: Entry) {
    clearTimeout(e.checkTimer);
    if (!e.checking) return;
    e.checking = false;
    this.publishServers();
  }
  /** One more grace period; only while the server says it is allowed. */
  postponeUpdate(id: string) {
    if (!this.canUpdate(id, 'postpone') || !this.entries.get(id)?.update?.can_postpone) return;
    try { this.send(id, { type: 'update_postpone' }); } catch { /* the card still shows the deadline */ }
  }
  /** Stops offering the release the owner is looking at. */
  skipUpdate(id: string) {
    const e = this.entries.get(id);
    const version = e?.update?.available || e?.updateAvailable;
    if (!e || !version || !this.canUpdate(id, 'skip')) return;
    try { this.send(id, { type: 'update_skip', version }); } catch { /* the notice stays */ }
  }
  private beginUpdate(e: Entry) {
    if (e.updating) return;
    e.updateFrom = e.version.split('+')[0];
    e.updating = true;
    // This also expires while disconnected. Removing/pausing a server cancels it; an old timer cannot end a retry.
    const timer = setTimeout(() => {
      if (this.entries.get(e.id) === e && e.updateTimer === timer) {
        this.finishUpdate(e, 'Server update: No newer running version was confirmed. Reconnect and check the helper service on this server.');
      }
    }, 600_000);
    e.updateTimer = timer;
    this.publishServers();
  }
  private finishUpdate(e: Entry, error?: string) {
    clearTimeout(e.updateTimer); e.updateTimer = undefined; e.updateFrom = undefined;
    e.updating = false; this.publishServers();
    if (error) this.opts.onMessage?.(e.id, { type: 'error', message: error });
  }
  findWorkspace(key: string): HubWorkspace | undefined {
    for (const e of this.entries.values()) {
      if (e.state === 'paused') continue;
      const s = e.workspaces.find(s => workspaceKey(e.id, s.id) === key);
      if (s) return { ...s, serverId: e.id, key, serverName: e.server.server_name || 'Server' };
    }
  }
  pause(id: string) {
    const e = this.entries.get(id); if (!e || e.state === 'paused') return;
    this.finishUpdate(e);
    this.disconnect(e); e.state = 'paused'; e.reason = 'Paused by you'; this.publish();
  }
  resume(id: string) { const e = this.entries.get(id); if (e?.state === 'paused') this.connect(e); }
  retry(id: string) { const e = this.entries.get(id); if (e && e.state !== 'paused') { this.disconnect(e); this.connect(e); } }
  remove(id: string) {
    const e = this.entries.get(id); if (!e) return;
    clearTimeout(e.updateTimer);
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
        : storedBundle(e.server).then(bundle => ({ record: e.server.record, wsUrl: e.server.server_url || undefined, bundle, winner: e.server.winner })),
      credentials: this.opts.credentials, claim: e.server.claim, serverId: e.server.server_id,
      onState: (state, detail) => {
        if (!current()) return;
        e.state = state;
        if (state !== 'connected') e.health = undefined; // a verdict from before the link dropped is not today's
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
        e.version = msg.version ?? ''; e.updateAvailable = msg.update_available ?? '';
        e.updateCapabilities = msg.update_capabilities ?? [];
        this.endCheck(e);
        if (e.updating && newerRelease(e.version, e.updateFrom ?? '')) this.finishUpdate(e);
        e.backends = msg.backends ?? []; e.tabs = !!msg.tabs; e.acpAgents = msg.acp_agents ?? [];
        e.controlMode = !!msg.control_mode; e.tmuxVersion = msg.tmux_version ?? ''; e.prefix = msg.prefix ?? '';
        this.publish(); this.opts.onSaved?.(e.server);
        connection.send({ type: 'list_workspaces' });
        connection.send({ type: 'agent_sync', after: syncRequest(e.agents) });
        this.opts.onReady?.(e.id, msg, restoring);
      },
      onDrop: (reason, upMs) => {
        if (!current()) return;
        // A sample from before the drop is not the server's health now.
        e.reason = reason; e.health = undefined; this.publish(); this.opts.onDrop?.(e.id, reason, upMs);
      },
      onPath: path => { if (current()) { e.path = path?.label ?? ''; this.publishServers(); } },
      onFailures: failures => { if (current()) { e.failures = failures; this.publishServers(); } },
      onWinner: key => {
        if (!current() || e.server.winner === key) return;
        e.server = { ...e.server, winner: key };
        this.opts.onSaved?.(e.server);
      },
      onFatal: reason => { if (current()) { e.state = 'closed'; e.reason = reason; this.publish(); this.opts.onFatal?.(e.id, reason); } },
    });
    e.connection = connection;
    e.unsubscribe = connection.subscribe(msg => {
      if (!current()) return;
      const changed = msg.type === 'workspaces' || msg.type === 'agent_snapshot' || msg.type === 'agent_events' || msg.type === 'window_list' || msg.type === 'activity' || msg.type === 'attention' || msg.type === 'tab_states';
      if (msg.type === 'workspaces') { e.workspaces = msg.workspaces; e.windows = forgetWindows(e.windows, msg.workspaces.map(s => s.id)); }
      if (msg.type === 'window_list') e.windows = applyWindowList(e.windows, msg);
      if (msg.type === 'tab_states') e.tabStates = Object.fromEntries(msg.states.map(s => [`${s.workspace}/${s.tab}`, s]));
      if (msg.type === 'agent_snapshot') e.agents = applySnapshot(e.agents, msg.states);
      if (msg.type === 'activity') e.agents = applyActivity(e.agents, msg.workspaces, Date.now());
      if (msg.type === 'attention') e.agents = applyAttention(e.agents, msg.items);
      if (msg.type === 'attention_stats') { e.attentionStats = msg.days; this.publishServers(); }
      if (msg.type === 'agent_events') {
        const result = applyEvents(e.agents, msg.events); e.agents = result.map;
        if (result.gap) connection.send({ type: 'agent_sync', after: syncRequest(e.agents) });
      }
      if (msg.type === 'update_status') { e.updateAvailable = msg.available ?? ''; this.publishServers(); }
      if (msg.type === 'update_state') {
        const { type: _type, ...info } = msg;
        e.update = info;
        if (e.checking && checkAnswered(info, e.checkBaseline ?? 0)) e.checking = false;
        this.publishServers();
      }
      if (msg.type === 'update_requested') this.beginUpdate(e);
      if (msg.type === 'error' && e.updating && (msg.message.startsWith('Server update:') || msg.message === 'Only the owner can update this server')) this.finishUpdate(e);
      if (msg.type === 'candidates') void this.acceptBundle(e, msg.bundle, current);
      if (msg.type === 'server_health') { const { type: _, ...h } = msg; e.health = h; this.publishServers(); }
      if (changed) this.publish();
      this.opts.onMessage?.(e.id, msg);
    });
    this.publish(); connection.start();
  }
  /**
   * The server pushed its signed bundle after auth. It is kept only when it verifies against the pinned server key, is
   * not expired and its `seq` is above the stored one; anything else leaves the stored bundle as the hint it was.
   */
  private async acceptBundle(e: Entry, wire: string, current: () => boolean) {
    const pin = e.server.server_id;
    if (!pin) return;
    try {
      const bundle = await verifyBundle(wire, { nowSecs: Math.floor(Date.now() / 1000), pin, storedSeq: e.server.bundle_seq });
      if (!current()) return;
      e.server = { ...e.server, bundle: wire, bundle_seq: bundle.seq, published: bundle.published };
      this.opts.onSaved?.(e.server);
    } catch (err) {
      // The server pushes the same bundle after every authentication: the one already stored is not a problem.
      if (err instanceof BundleRejected && err.reason === 'stale' && this.isStored(e, wire)) return;
      console.warn('[termenbox] ignored a candidate bundle:', err instanceof Error ? err.message : err);
    }
  }
  private isStored(e: Entry, wire: string): boolean {
    try { return decodeBundle(wire).bundle.seq === e.server.bundle_seq; } catch { return false; }
  }
  /** The server list alone: a health sample every 10 s per server must not rebuild the workspace, agent and window stores. */
  private publishServers(entries = [...this.entries.values()]) {
    this.servers.set(entries.map(({ id, server, state, reason, version, updateAvailable, update, updateCapabilities, checking, updating, backends, tabs, acpAgents, controlMode, tmuxVersion, prefix, health, path, failures, attentionStats }) => ({ id, server, state, reason, version, updateAvailable, update, updateCapabilities, checking, updating, backends, tabs, acpAgents, controlMode, tmuxVersion, prefix, health, path, failures, attentionStats })));
  }
  private publish() {
    const entries = [...this.entries.values()];
    this.publishServers(entries);
    this.allWorkspaces.set(orderWorkspaces(entries.filter(e => e.state !== 'paused').flatMap(e => e.workspaces.map(s => ({ ...s,
      serverId: e.id, serverName: e.server.server_name || 'Server', key: workspaceKey(e.id, s.id) })))));
    this.agents.set(Object.fromEntries(entries.filter(e => e.state !== 'paused').flatMap(e => Object.entries(e.agents).map(([id, a]) => [workspaceKey(e.id, id), a]))));
    this.windows.set(Object.fromEntries(entries.filter(e => e.state !== 'paused').flatMap(e => Object.entries(e.windows).map(([id, w]) => [workspaceKey(e.id, id), w]))));
    this.tabStates.set(Object.fromEntries(entries.filter(e => e.state !== 'paused').flatMap(e => Object.values(e.tabStates).map(s => [`${workspaceKey(e.id, s.workspace)}/${s.tab}`, s]))));
  }
}
