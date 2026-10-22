<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Terminal from './Terminal.svelte';
  import ServerList from './ServerList.svelte';
  import AddServerModal from './AddServerModal.svelte';
  import PairDialog from './PairDialog.svelte';
  import { Toast } from './lib/ui';
  import SessionSheet from './SessionSheet.svelte';
  import type { SheetHeight } from './lib/sheet';
  import NavigationBar from './NavigationBar.svelte';
  import MenuSheet from './MenuSheet.svelte';
  import InboxSheet from './InboxSheet.svelte';
  import { inbox, mergeAttention, markAllRead, unreadCount } from './lib/inbox';
  import { currentEndpoint, disablePush, enablePush, pushSupported } from './lib/push';
  import SessionActions from './SessionActions.svelte';
  import WindowMenu from './WindowMenu.svelte';
  import NeedsYouBanner from './NeedsYouBanner.svelte';
  import NewSessionSheet from './NewSessionSheet.svelte';
  import { SnapshotStore } from './lib/snapshots';
  import { serverAtLeast, NEW_WINDOW_VERSION } from './lib/serverVersion';
  import { loadHidden, openTabs, saveHidden } from './lib/tabs';
  import { waitingList, startAnnounce, startBrowserNotifications, type AnnounceHandle, type NotificationHandle } from './lib/notify';
  import { attentionToAck } from './lib/agents';
  import { cacheRemaining, expiringSoon } from './lib/cache';
  import { settings, applyTheme, type Skin } from './lib/settings';
  import type { ServerListItem } from './lib/protocol';
  import { parseClaim, parseServerLink, serverCode, serverLink } from './lib/onboard';
  import { sendServerLink } from './lib/share';
  import { ServerHub, newSessionTargets, serverKey, sessionDestinations, sessionKey, type HubServer } from './lib/hub';
  import { lookup } from './lib/directory';
  import { linksToAccount, mergePaired, parsePairLink, type PairLink } from './lib/pair';
  import { storedBundle } from './lib/paths';
  import { DeleteFailed, deleteMyData, registerServer, SIGN_IN_EXPIRED } from './lib/account';
  import { deviceKey, publicKeyB64, signChallenge, certExpiry, certUserId, avatarUrl } from './lib/device';
  import AppShell from './AppShell.svelte';
  import AppFrame from './shell/AppFrame.svelte';
  import SettingsView from './shell/SettingsView.svelte';
  import ActivityView from './shell/ActivityView.svelte';
  import { answeredHealth, askedHealth, type HealthMap } from './lib/health';
  import { answered, asked, DeleteBook, type HistoryDeletion, type HistoryMap, type Range } from './lib/history';
  import { Segmented } from './lib/ds';
  import SessionView from './SessionView.svelte';
  import AcpView from './AcpView.svelte';
  import { acpStreams, applyAcp, forgetAcp } from './lib/acpStore';
  import { openMessage, shellName } from './lib/acpStream';
  import { merge as mergeView, requestView, type ViewModel } from './lib/sessionView';
  import { loadPaneModes, withPaneMode, type PaneMode } from './lib/paneMode';
  import { rowOf } from './lib/home';
  import { answerIndex, nextNeedsYou, stepOf } from './lib/decision';
  import { fleetOf } from './lib/navlist';
  import { canSplit, commitLayout, layout, MIN_HALF, patchLayout, SPLIT, splitWidthFor, toggleSidebar, toggleSplit } from './lib/panes';
  import Resizer from './shell/Resizer.svelte';
  import Home from './Home.svelte';
  import SessionGroups from './SessionGroups.svelte';
  import { destination, navigate, startRouter, type Destination } from './lib/router';
  import ReportDialog from './ReportDialog.svelte';
  import { buildReport, noteError, recentErrors, scrub } from './lib/report';
  import { runningBundle } from './lib/update';
  import { startUsageMeter } from './lib/usageMeter';
  import { DEBUG_KEY, tapCounter, viewportReading } from './lib/viewport';
  import { crumb, crumbText } from './lib/breadcrumbs';
  import { identityOf } from './lib/identity';
  import { HookOffers } from './lib/hookOffer';
  import { applyHookReply, hookQueries, hookRows, markBusy, pendingOffer, waitingOnPrompt, type HookMap, type HookOffer, type HookRow } from './lib/hooks';
  import { watchForUpdates } from './lib/update';
  import { isIosBrowser } from './lib/pwa';

  const titleTap = tapCounter();
  /** Five quick taps on the title switch the viewport overlay on or off; an installed PWA has no address bar for ?debug. */
  function titleTapped() {
    if (!titleTap(Date.now())) return;
    const on = localStorage.getItem(DEBUG_KEY) !== '1';
    localStorage.setItem(DEBUG_KEY, on ? '1' : '0');
    window.dispatchEvent(new Event('termenbox-debug'));
    showToast(on ? 'Debug overlay on' : 'Debug overlay off');
  }

  let reportText: string | null = null;
  /** Version of the connected server (`0.1.0+abc1234`), shown so a stale image is noticed. */
  let serverVersion = '';
  function openReport() {
    crumb('report opened');
    const vv = window.visualViewport;
    reportText = buildReport({
      version: runningBundle(),
      userAgent: navigator.userAgent,
      inner: `${window.innerWidth}x${window.innerHeight}`,
      visual: viewportReading(vv, window, document.activeElement),
      steps: crumbText(),
      connection: selectedServer ? connState : 'no server selected',
      layout: ['.shell', '.workspace', '.terminal-pane', '.screen', '.xterm', '.xterm canvas'].map((q) => {
        const e = document.querySelector(q);
        if (!e) return `${q} -`;
        const b = e.getBoundingClientRect();
        return `${q} x${Math.round(b.left)} w${Math.round(b.width)} s${e.scrollLeft}`;
      }).join('; '),
      server: serverVersion || undefined,
      errors: $recentErrors,
    });
  }

  let user: string | null = null;
  $: avatar = deviceCert ? avatarUrl(certUserId(deviceCert)) : '';
  /** Device certificate from the hosted function; the GitHub token itself is never kept (DECISION-0062). */
  let deviceCert: string | null = null;
  let servers: ServerListItem[] = [];
  let activeKey: string | null = null;
  let activeServerId: string | null = null;
  let serversPage = false;
  let wide = false;
  let helpOpen = false;
  /** Off the terminal: another destination, or the servers page (which belongs to Settings). */
  $: away = serversPage || $destination !== 'sessions';
  $: if ($destination !== 'settings') serversPage = false;
  const hub = new ServerHub({
    credentials: async () => {
      const pair = await deviceKey();
      return { cert: deviceCert!, sign: (id, nonce) => signChallenge(pair, id, nonce) };
    },
    target: async (server, refresh) => {
      // A server added by a pairing link may never have been in the directory (LAN-only): do not ask it.
      if (refresh && server.server_id && !(server.bundle && !server.record)) {
        try { server.record = (await lookup(location.origin, server.server_id)) ?? server.record; }
        catch { /* Try the last verified address before reporting unreachable. */ }
      }
      return { record: server.record, wsUrl: server.server_url || undefined, bundle: await storedBundle(server), winner: server.winner };
    },
    onSaved: server => saveServers(servers.map(s => serverKey(s) === serverKey(server) ? server : s)),
    onReady: (id, msg, reconnect) => {
      hub.send(id, { type: 'push_info' });
      for (const q of hookQueries()) hub.send(id, q);
      if (!reconnect) showToast(`Connected to ${$hubServers.find(s => s.id === id)?.server.server_name || msg.server_name}`); // a reconnect is shown by the status line, not a toast each time
      if (!reconnect || away || id !== activeServerId || !activeKey) return;
      const key = activeKey;
      // Retry and Resume replace the connection: mount its terminal before accepting a repaint.
      void tick().then(() => {
        if (!away && id === activeServerId && key === activeKey) showSession(key);
      });
    },
    onDrop: (_id, reason, upMs) => noteError(`connection lost after ${Math.round(upMs / 1000)} s: ${reason}`),
    onFatal: (_id, reason) => { noteError(`connection: ${reason}`); showToast(reason, 'error'); },
    onMessage: (_id, msg) => {
      const id = _id;
      if (msg.type === 'snapshot') snapshots.apply(sessionKey(_id, msg.session), msg);
      else if (msg.type === 'session_attached') preview = null;
      else if (msg.type === 'session_view') { if (id === activeServerId && msg.session === activeSessionId) heldView = mergeView(heldView, msg); }
      else if (msg.type === 'acp_view') acpStreams.update(m => applyAcp(m, sessionKey(_id, msg.session), msg.session, msg));
      else if (msg.type === 'sessions') { openPending(_id, msg.sessions); acpStreams.update(m => forgetAcp(m, `${_id}/`, new Set(msg.sessions.map(x => sessionKey(_id, x.id))))); }
      else if (msg.type === 'attention') inbox.update(l => mergeAttention(l, _id, msg.items, s => sessionKey(_id, s), s => hub.findSession(sessionKey(_id, s))?.name ?? s));
      else if (msg.type === 'history') historyMap = answered(historyMap, _id, msg);
      else if (msg.type === 'health_history') healthMap = answeredHealth(healthMap, _id, msg);
      else if (msg.type === 'history_deleted') historyDeletes.reply(_id, msg.removed);
      else if (msg.type === 'push_info') pushInfos = { ...pushInfos, [_id]: msg };
      else if (msg.type === 'agent_hooks') {
        hookMap = applyHookReply(hookMap, _id, msg);
        // After a removal the next Enable needs the exact change again.
        if (msg.action === 'remove' && !msg.error) hub.send(_id, { type: 'agent_hooks', agent: msg.agent, action: 'preview' });
      }
      if (msg.type === 'error') showToast(msg.message, 'error');
      else if (msg.type === 'agent_event') showToast(`Agent ${msg.agent} ${msg.event} in ${msg.session_id}`);
    },
  });
  const snapshots = new SnapshotStore();
  // Activity (U12): each connected server is asked for the range on screen and answers from its own history.
  let historyMap: HistoryMap = {};
  // Server health (U14): the in-memory samples of each connected server, asked for with the history.
  let healthMap: HealthMap = {};
  function askHealth(restart = false) {
    for (const s of $hubServers) {
      if (s.state !== 'connected') continue;
      healthMap = askedHealth(healthMap, s.id, Date.now(), restart);
      hub.send(s.id, { type: 'health_history' });
    }
  }
  $: unhealthyServers = new Set($hubServers.filter(s => s.health && !s.health.healthy).map(s => s.id));
  const historyDeletes = new DeleteBook();
  function askHistory(range: Range) {
    for (const s of $hubServers) {
      if (s.state !== 'connected') continue;
      historyMap = asked(historyMap, s.id, range, Date.now());
      hub.send(s.id, { type: 'history', from: range.from, to: range.to });
    }
  }
  async function deleteHistory(): Promise<HistoryDeletion> {
    const ids = $hubServers.filter(s => s.state === 'connected').map(s => s.id);
    const r = await historyDeletes.run(ids, id => hub.send(id, { type: 'history_delete' }));
    historyMap = {};
    const nameOf = (id: string) => $hubServers.find(s => s.id === id)?.server.server_name || 'A server';
    return { servers: ids.length - r.failed.length, removed: r.removed, failedNames: r.failed.map(nameOf) };
  }
  // The hook offer (decision 0106): what each server says about each agent's hooks, and this device's answers to the offer.
  const hookOffers = new HookOffers();
  let hookMap: HookMap = {};
  let offerTick = 0;
  $: hookOffer = (offerTick, now, pendingOffer($hubServers, $allSessions, $allAgents, hookMap, hookOffers));
  $: hookSettings = (offerTick, hookRows($hubServers, hookMap, hookOffers));
  function enableHooks(serverId: string, agent: string) {
    hookOffers.clear(serverId, agent);
    hookMap = markBusy(hookMap, serverId, agent, 'apply');
    hub.send(serverId, { type: 'agent_hooks', agent, action: 'apply' });
    offerTick++;
  }
  function homeHook(choice: 'enable' | 'later' | 'never', o: HookOffer) {
    if (choice === 'enable') enableHooks(o.serverId, o.agent);
    else { if (choice === 'later') hookOffers.notNow(o.serverId, o.agent); else hookOffers.never(o.serverId, o.agent); offerTick++; }
  }
  function settingsHook(choice: 'preview' | 'enable' | 'remove' | 'again', row: HookRow) {
    if (choice === 'enable') enableHooks(row.serverId, row.agent);
    else if (choice === 'preview') hub.send(row.serverId, { type: 'agent_hooks', agent: row.agent, action: 'preview' });
    else if (choice === 'remove') { hookMap = markBusy(hookMap, row.serverId, row.agent, 'remove'); hub.send(row.serverId, { type: 'agent_hooks', agent: row.agent, action: 'remove' }); offerTick++; }
    else { hookOffers.clear(row.serverId, row.agent); offerTick++; }
  }
  /** Keys went to the open session: while its agent waits on a prompt that is the user answering by hand. */
  function typedByHand() {
    const a = activeKey ? $allAgents[activeKey] : undefined;
    const s = activeKey ? hub.findSession(activeKey) : undefined;
    if (!a?.agent || !s || !waitingOnPrompt(a)) return;
    hookOffers.manualAnswer(s.serverId, a.agent);
    offerTick++;
  }
  // The in-session screen: Terminal (default) or Decision, kept per session on this device. The terminal stays mounted
  // in both (hidden, never rebuilt), so a switch cannot lose its state or its focus rules.
  let paneModes = loadPaneModes();
  $: paneMode = (activeKey && paneModes[activeKey]) || 'terminal' as PaneMode;
  function setPaneMode(m: string) { if (activeKey) paneModes = withPaneMode(paneModes, activeKey, m === 'decision' ? 'decision' : 'terminal'); }
  let heldView: ViewModel | null = null;
  $: activeKey, (heldView = null);
  $: viewPolling = !!activeKey && !away && pageVisible && (paneMode === 'decision' || split) && connState === 'connected' && !activeIsAcp;
  let viewPoll: ReturnType<typeof setInterval> | undefined;
  function requestSessionView() { if (activeKey) hub.sendToSession(activeKey, requestView(activeSessionId ?? '', heldView)); }
  $: { activeKey; if (viewPoll) clearInterval(viewPoll); viewPoll = undefined; if (viewPolling) { requestSessionView(); viewPoll = setInterval(requestSessionView, 2000); } }
  $: activeRow = activeSession ? rowOf(activeSession, $allAgents[activeSession.key]) : null;
  /** The last screen seen for the session being opened, dimmed in the terminal until the live repaint replaces it. */
  let preview: string | null = null;
  const hubServers = hub.servers;
  const allWindows = hub.windows;
  const allSessions = hub.allSessions;
  const allAgents = hub.agents;
  const connectedCount = hub.connectedCount;
  const overLimit = hub.overLimit;
  $: selected = $hubServers.find(s => s.id === activeServerId);
  $: selectedServer = selected?.server ?? null;
  $: sessions = $allSessions.map(s => ({ ...s, id: s.key }));
  $: activeSessionId = $allSessions.find(s => s.key === activeKey)?.id ?? null;
  $: connection = selected ? hub.connection(selected.id) : null;
  $: connState = selected?.state ?? 'closed';
  $: serverVersion = selected?.version ?? '';
  let narrow = false;
  /** Beside the decision, the live terminal (U15): only on the desktop, for a session with a structured view, and wide enough for both halves. */
  let bodyWidth = 0;
  $: split = wide && $layout.splitOpen && !!activeSession && !!activeRow && (bodyWidth === 0 || canSplit(bodyWidth));
  $: splitW = splitWidthFor($layout.splitWidth, bodyWidth || 1200);
  $: splitMax = Math.max(SPLIT.min, Math.min(SPLIT.max, (bodyWidth || 1200) - MIN_HALF));
  let iosHint = isIosBrowser() && !localStorage.getItem('termenbox_ios_hint');
  let sheetHeight: SheetHeight = 'half';
  let now = Date.now();
  let hidden = loadHidden();
  let menuKey: string | null = null;
  let actionConfirm = false;
  let showNew = false;
  let pendingOpen: { serverId: string; name: string } | null = null;
  let quality = '';
  let keyboardOpen = false;
  let sheetHeld = false;
  let menuOpen = false;
  let inboxOpen = false;
  let pushInfos: Record<string, Extract<import('./lib/protocol').ServerMessage, { type: 'push_info' }>> = {};
  let pushOn = false;
  void currentEndpoint().then(e => (pushOn = !!e && typeof Notification !== 'undefined' && Notification.permission === 'granted'));
  // The server the menu speaks about: the open one, else the first that answered.
  $: pushServerId = activeServerId && pushInfos[activeServerId] ? activeServerId : Object.keys(pushInfos)[0] ?? null;
  $: pushView = pushSupported() && pushServerId ? {
    serverName: $hubServers.find(s => s.id === pushServerId)?.server.server_name || 'this server',
    configured: pushInfos[pushServerId].configured, on: pushOn,
    denied: typeof Notification !== 'undefined' && Notification.permission === 'denied',
  } : null;
  async function togglePush(enable: boolean) {
    const id = pushServerId;
    if (!id) return;
    const send = (m: import('./lib/protocol').ClientMessage) => hub.send(id, m);
    try {
      if (enable) {
        const r = await enablePush(send, pushInfos[id].public_key);
        pushOn = r === 'ok';
        if (r === 'denied') showToast('Notifications are blocked in the browser settings', 'error');
        else if (r === 'unsupported') showToast('This browser cannot send notifications', 'error');
      } else { await disablePush(send); pushOn = false; }
    } catch (e) { showToast(e instanceof Error ? e.message : 'Notifications failed', 'error'); }
  }
  // Tell the servers which session is in front, so a push is not sent for what someone is looking at.
  let reportedFg: { serverId: string; session: string } | null = null;
  function reportForeground() {
    const s = activeKey && !away && pageVisible ? hub.findSession(activeKey) : undefined;
    const next = s ? { serverId: s.serverId, session: s.id } : null;
    if (next?.serverId === reportedFg?.serverId && next?.session === reportedFg?.session) return;
    if (reportedFg && reportedFg.serverId !== next?.serverId) hub.send(reportedFg.serverId, { type: 'foreground', session: null });
    if (next) hub.send(next.serverId, { type: 'foreground', session: next.session });
    reportedFg = next;
  }
  $: activeKey, away, pageVisible, reportForeground();
  let actionKey: string | null = null;
  let cardExpanded = false;
  $: actionSession = $allSessions.find(s => s.key === actionKey);
  $: menuSession = $allSessions.find(s => s.key === menuKey);
  const backendOf = (s: { backend?: string; serverId: string } | undefined) => s ? (s.backend ?? $hubServers.find(h => h.id === s.serverId)?.backends[0] ?? '') : '';
  // An ACP session (T0) has no terminal: opening it shows its stream (decision 0111). The stream is per connection, so it
  // is asked for again whenever the session is shown, and dropped from the server when it is not.
  $: activeIsAcp = !!activeSession && backendOf(activeSession) === 'acp';
  let acpServer: string | null = null;
  function closeAcp() {
    if (acpServer) hub.send(acpServer, { type: 'acp_close' });
    acpServer = null;
  }
  /** Shows a session: an ACP session opens its stream, any other attaches its terminal. */
  function showSession(key: string) {
    const s = hub.findSession(key);
    if (!s) return;
    if (backendOf(s) === 'acp') {
      // A terminal still attached on this server would go on streaming to a view that is gone, keep its size and count
      // as someone looking: let go of it first.
      hub.send(s.serverId, { type: 'detach' });
      acpServer = s.serverId;
      hub.sendToSession(key, openMessage(key, $acpStreams[key]));
    } else {
      closeAcp();
      hub.sendToSession(key, { type: 'attach', session_id: key });
    }
  }
  const sendAcp = (m: import('./lib/protocol').ClientMessage) => { if (activeKey) hub.sendToSession(activeKey, m); };
  /** The companion shell (decision 0109): a plain tmux session in the ACP session's working directory, made on the first
   *  tap and opened on every tap after. ACP gives the owner no terminal of the agent's own. */
  function openShell(key: string) {
    const s = hub.findSession(key);
    if (!s) return;
    const wanted = shellName(s.name);
    const existing = $allSessions.find(x => x.serverId === s.serverId && backendOf(x) === 'tmux' && x.name === wanted);
    if (existing) { void attach(existing.key); return; }
    // Without the directory a new shell would open in the server's default one and look like the session's own.
    const dir = $acpStreams[key]?.info?.cwd;
    if (!dir) { showToast('The session’s directory is not known yet. Try again in a moment.', 'error'); return; }
    createSession({ serverId: s.serverId, backend: 'tmux', dir, name: wanted });
  }
  $: canShell = !!activeSession && ($hubServers.find(h => h.id === activeSession.serverId)?.backends.includes('tmux') ?? false);
  $: tabSessions = openTabs($allSessions, $allAgents, hidden);
  $: waiting = waitingList($allSessions, $allAgents);
  let announceHandle: AnnounceHandle | null = null;
  $: {
    announceHandle?.stop();
    announceHandle = startAnnounce(waiting.length);
  }
  let notificationHandle: NotificationHandle | null = null;
  $: {
    notificationHandle?.stop();
    notificationHandle = startBrowserNotifications(waiting, pageVisible);
  }
  // Looking at a session clears its finished or failed item on the server; a question stays until it is answered.
  let pageVisible = typeof document === 'undefined' || document.visibilityState !== 'hidden';
  const acked = new Set<string>();
  function sendAttention(key: string, m: import('./lib/protocol').ClientMessage) {
    const s = hub.findSession(key);
    if (s) hub.send(s.serverId, m);
  }
  $: if (activeKey && !away) {
    for (const id of attentionToAck($allAgents[activeKey], true, pageVisible, acked)) {
      acked.add(id);
      sendAttention(activeKey, { type: 'ack', attention_id: id });
    }
  }
  // Idle Claude Code sessions with a warm prompt cache, for Activity (an estimate from the last answer, decision 0083).
  $: warmCaches = $allSessions.flatMap(s => { const a = $allAgents[s.key]; const r = a?.agent === 'claude' ? cacheRemaining(a, now, $settings.cacheTtlMs) : null; return r !== null && r > 0 ? [{ key: s.key, name: s.name, server: s.serverName, remainingMs: r }] : []; }).sort((a, b) => a.remainingMs - b.remainingMs);
  $: expiring = new Set($allSessions.filter(s => expiringSoon(cacheRemaining($allAgents[s.key], now, $settings.cacheTtlMs))).map(s => s.key));
  $: activeSession = $allSessions.find(s => s.key === activeKey);
  $: profile = backendOf(activeSession) === 'tmux' ? 'tmux' as const : 'shell' as const;
  $: link = selected ? { ok: connState === 'connected', text: connState === 'connected' ? quality || 'connected' : connState } : null;
  $: if (hidden.size) { const live = new Set($allSessions.map(s => s.key)); if ([...hidden].some(k => !live.has(k)) && $allSessions.length) { hidden = new Set([...hidden].filter(k => live.has(k))); saveHidden(hidden); } }
  let showAddServer = false;
  /** A pairing link waiting for the user's confirmation (decision 0123). */
  let pairLink: PairLink | null = null;
  let addPrefill: string | null = null;
  let toast: { message: string; type: 'success' | 'error'; detail?: string } | null = null;

  const GITHUB_CLIENT_ID = import.meta.env.VITE_GITHUB_CLIENT_ID || '';

  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let toastHeld = false;
  /** A toast with `detail` (the real error) stays long enough to read and copy; hovering it keeps it. */
  function showToast(message: string, type: 'success' | 'error' = 'success', detail?: string) {
    toast = { message, type, detail };
    toastHeld = false;
    armToast(detail ? 20000 : 4000);
  }
  function armToast(ms: number) {
    clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { if (!toastHeld) toast = null; }, ms);
  }
  function holdToast(held: boolean) {
    toastHeld = held;
    if (!held && toast) armToast(4000);
  }

  // Called on click, never during render: each call replaces the stored state, which would break the callback check.
  function loginWithGitHub() {
    const state = crypto.randomUUID();
    sessionStorage.setItem('termenbox_oauth_state', state);
    const params = new URLSearchParams({
      client_id: GITHUB_CLIENT_ID,
      redirect_uri: `${location.origin}/callback`,
      scope: 'read:user',
      state,
    });
    window.location.assign(`https://github.com/login/oauth/authorize?${params}`);
  }

  function checkAuth() {
    try {
      const stored = JSON.parse(localStorage.getItem('termenbox_auth') || 'null');
      if (stored?.cert && stored?.login && certExpiry(stored.cert) * 1000 > Date.now()) {
        deviceCert = stored.cert;
        user = stored.login;
        return true;
      }
    } catch {}
    localStorage.removeItem('termenbox_auth'); // old token-era entry or an expired certificate: sign in again
    return false;
  }

  function loadServers() {
    try {
      servers = JSON.parse(localStorage.getItem('termenbox_servers') || '[]');
    } catch {
      servers = [];
    }
  }

  function saveServers(next: ServerListItem[]) {
    servers = next;
    localStorage.setItem('termenbox_servers', JSON.stringify(next));
  }

  function addServer(server: ServerListItem) {
    const same = (s: ServerListItem) => (server.server_id ? s.server_id === server.server_id : !s.server_id && s.server_url === server.server_url);
    saveServers([...servers.filter((s) => !same(s)), server]);
    hub.add(server);
    showWorkspace();
  }

  /**
   * Links a freshly set-up server to this account so "delete my data" can find it, before the first connection:
   * once the server has an owner it stops announcing the claim secret that proves who set it up. The server is added either way.
   */
  async function addAndLink(server: ServerListItem) {
    if (linksToAccount(server, !!deviceCert)) {
      try { await registerServer(location.origin, { cert: deviceCert!, pair: await deviceKey() }, server.claim!); }
      catch (e) {
        noteError(`account link: ${e instanceof Error ? e.message : e}`);
        showToast('Server added, but it could not be linked to your account, so "Delete my data" will not cover it. Its record is removed 30 days after it goes quiet.', 'error');
      }
    }
    addServer(server);
  }

  /** A server confirmed in the pairing dialog; one already saved keeps what this device knows about it. */
  async function addPaired(paired: ServerListItem) {
    const existing = servers.find((s) => s.server_id === paired.server_id);
    await addAndLink(mergePaired(existing, paired));
  }

  async function deleteData(): Promise<number> {
    if (!deviceCert) throw new DeleteFailed(SIGN_IN_EXPIRED, true);
    return deleteMyData(location.origin, { cert: deviceCert, pair: await deviceKey() });
  }

  async function handleOAuthCallback() {
    const params = new URLSearchParams(window.location.search);
    const code = params.get('code');
    const expected = sessionStorage.getItem('termenbox_oauth_state');
    sessionStorage.removeItem('termenbox_oauth_state');
    window.history.replaceState({}, '', '/');
    if (!code || !expected || params.get('state') !== expected) {
      showToast('Authentication failed', 'error');
      return false;
    }
    try {
      const res = await fetch('/api/oauth/exchange', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ code, device_key: await publicKeyB64(await deviceKey()) }),
      });
      if (!res.ok) throw new Error('OAuth exchange failed');
      const { cert, login } = await res.json();
      localStorage.setItem('termenbox_auth', JSON.stringify({ cert, login }));
      deviceCert = cert;
      user = login;
      showToast(`Welcome, ${login}!`);
      return true;
    } catch (e) {
      showToast('Authentication failed', 'error');
      return false;
    }
  }

  /** Re-adds a registered server by its public id: the record is signed by that key, so nothing secret is needed. */
  async function addServerById(id: string) {
    try {
      const record = await lookup(location.origin, id);
      if (!record) {
        showToast('That server is not in the directory yet. Is it running?', 'error');
        return;
      }
      const server: ServerListItem = { server_id: id, server_name: '', server_url: '', record, registered_at: Date.now() };
      addServer(server);
    } catch (e) {
      showToast(e instanceof Error ? e.message : 'Could not add the server', 'error');
    }
  }

  /** Sends the saved server to the owner's other device: the share sheet on a phone, a copied link elsewhere. */
  async function shareServer(index: number) {
    const server = servers[index];
    if (!server?.server_id) return;
    const result = await sendServerLink(serverLink(location.origin, server.server_id), server.server_name, serverCode(server.server_id));
    if (result === 'copied') showToast('Link and code copied. Open the link on your other device, or paste the code in Add Server in the app, and sign in with the same GitHub account.');
    else if (result === 'failed') showToast('Could not share the link', 'error');
  }

  const LAST_KEY = 'termenbox_last_session';
  // The app opens on Home; its Continue card offers the session last used (decision 0100). Nothing attaches by itself.
  let lastKey: string | null = localStorage.getItem(LAST_KEY);
  function forget(key: string) { if (localStorage.getItem(LAST_KEY) === key) { localStorage.removeItem(LAST_KEY); lastKey = null; } }

  async function attach(key: string) {
    const session = hub.findSession(key);
    if (!session) return;
    if ($hubServers.find(s => s.id === session.serverId)?.state !== 'connected') {
      showToast('This server is reconnecting. You can retry it in Servers.', 'error');
      return;
    }
    crumb('open session tab');
    closeAcp();
    sheetHeight = 'closed';
    prevAway = false; // this open sends its own attach below; the away/back watcher must not send a second
    if (hidden.has(key)) { hidden.delete(key); hidden = new Set(hidden); saveHidden(hidden); }
    if (activeServerId && activeServerId !== session.serverId) hub.send(activeServerId, { type: 'detach' });
    serversPage = false;
    navigate('sessions');
    preview = key === activeKey ? null : snapshots.text(key);
    activeKey = key;
    activeServerId = session.serverId;
    localStorage.setItem(LAST_KEY, key);
    lastKey = key;
    // Mount the receiving terminal before attach can produce a repaint on another server.
    await tick();
    if (activeKey === key) showSession(key);
  }

  /** Opens a window and shows it. The session is attached first when it is not the open one: attaching lands on the
   * tab's representative window, so attaching after the new window exists (or again on the open session) would
   * switch straight back to the old one. tmux moves the attached client to the new window by itself. */
  async function newWindow(key: string) {
    if (key !== activeKey) await attach(key);
    hub.sendToSession(key, { type: 'new_window', session: key });
  }

  /** Fetches a session's screen while the finger is still going down on its tab or card (decision 0084). */
  function peek(key: string) {
    if (key === activeKey) return;
    hub.sendToSession(key, { type: 'peek', session: key, hash: snapshots.hash(key) });
  }

  /** Detach: the tab closes and the stream stops; the session keeps running and stays in the Sessions sheet. */
  function detach(key: string) {
    crumb('detach');
    forget(key);
    if (key === activeKey) closeAcp();
    hidden = new Set(hidden).add(key); saveHidden(hidden);
    if (key === activeKey) {
      if (activeServerId) hub.send(activeServerId, { type: 'detach' });
      activeKey = null; activeServerId = null; preview = null;
      sheetHeight = 'half';
    }
  }

  /** Close session: it and everything in it ends (the dialog has asked first). */
  function closeSession(key: string) {
    crumb('close session');
    forget(key);
    if (key === activeKey) closeAcp();
    hub.sendToSession(key, { type: 'close_session', session: key });
    if (key === activeKey) { activeKey = null; activeServerId = null; preview = null; sheetHeight = 'half'; }
  }

  function openWindowMenu(key: string) {
    crumb('window menu opened');
    menuKey = key;
    const s = hub.findSession(key);
    if (backendOf(s) !== 'acp') hub.sendToSession(key, { type: 'windows', session: key }); // an ACP session has no windows
  }

  function createSession(req: { serverId: string; backend?: string; dir?: string; template?: string; name: string }) {
    pendingOpen = { serverId: req.serverId, name: req.name };
    hub.send(req.serverId, { type: 'new_session', name: req.name, backend: req.backend, dir: req.dir, template: req.template });
  }
  /** The session the user just created opens by itself once the server lists it. */
  function openPending(serverId: string, list: import('./lib/protocol').SessionInfo[]) {
    if (!pendingOpen || pendingOpen.serverId !== serverId) return;
    const made = list.find(s => s.name === pendingOpen!.name);
    if (!made) return;
    pendingOpen = null;
    void attach(sessionKey(serverId, made.id));
  }

  function keepWarm(key: string) {
    if ($settings.keepWarm) hub.sendToSession(key, { type: 'reply', session: key, text: $settings.keepWarm });
  }

  function sendSession(m: import('./lib/protocol').ClientMessage) {
    if ('session' in m) hub.sendToSession(m.session, m);
  }

  function removeServer(index: number) {
    const id = serverKey(servers[index]);
    hub.remove(id);
    saveServers(servers.filter((_, i) => i !== index));
    if (activeServerId === id) { activeKey = null; activeServerId = null; }
  }

  /** `serverId` comes from a server's own "+ New" (the sheet is then locked to it); without it the sheet asks when there are several. */
  let newTarget = { serverId: '', locked: false };
  function newSession(serverId?: string) {
    const asked = typeof serverId === 'string' ? serverId : undefined;
    const t = newSessionTargets($hubServers, asked ?? selected?.id);
    if (!t.servers.length || (asked && t.serverId !== asked)) { showToast('Wait for a server to connect or retry it in Servers.', 'error'); return; }
    newTarget = { serverId: t.serverId, locked: !!asked };
    showNew = true;
  }

  function logout() {
    closeAcp();
    localStorage.removeItem('termenbox_auth');
    hub.close();
    user = null;
    deviceCert = null;
    activeKey = null;
    activeServerId = null;
  }

  /** Back to the sessions destination (the servers page belongs to Settings). Coming back from away re-attaches (syncAway). */
  function showWorkspace() {
    serversPage = false;
    navigate('sessions');
  }

  /** The servers page is part of Settings; leaving the screen detaches the open session (syncAway). */
  function leaveServer() {
    serversPage = true;
    navigate('settings');
    sheetHeight = 'closed';
  }

  function go(dest: Destination) {
    if (dest !== 'settings') serversPage = false;
    navigate(dest);
  }

  /** Away from the terminal (another destination or the servers page): detach; back: attach again. */
  let prevAway = false;
  $: syncAway(away);
  function syncAway(now: boolean) {
    if (now === prevAway) return;
    prevAway = now;
    if (now) { closeAcp(); if (activeServerId) hub.send(activeServerId, { type: 'detach' }); }
    else if (activeKey) {
      const key = activeKey;
      void tick().then(() => {
        if (!away && activeKey === key && connState === 'connected') showSession(key);
      });
    }
  }

  function openServer(id: string) {
    const first = $allSessions.find(s => s.serverId === id);
    if (first) void attach(first.key); else leaveServer();
  }
  function runAction(id: string) {
    const [name, arg] = id.split(':');
    if (name === 'new-session') newSession();
    else if (name === 'pause' && arg) hub.pause(arg);
    else if (name === 'resume' && arg) hub.resume(arg);
    else if (['approve-decision', 'deny-decision', 'next-needs-you', 'focus-composer'].includes(name)) setTimeout(() => handleShortcut(name), 0); // after the palette has closed and let go of focus
  }

  /** The reply field to type in: the visible, enabled composer, the open session's first. Others (Home lists several) are not reached from the keyboard here. */
  function visibleComposer(): HTMLInputElement | null {
    const shown = Array.from(document.querySelectorAll<HTMLInputElement>('input[data-composer]:not([disabled])'))
      .filter((el) => el.getClientRects().length > 0 && !el.closest('[inert], [aria-hidden=true], .pane-hidden'));
    return shown.find((el) => el.closest('[data-testid=session-view]')) ?? shown[0] ?? null;
  }
  /** Taps the plain Yes or No of the decision on screen. Nothing is guessed: with no such option the answer stays with the user. */
  function answerShownDecision(kind: 'yes' | 'no') {
    const options = Array.from(document.querySelectorAll<HTMLElement>('[data-hero-options]')).find((el) => el.getClientRects().length > 0 && !el.closest('[inert], .pane-hidden'));
    if (!options) { showToast('No decision is open here. Open a session that needs you.'); return; }
    const buttons = Array.from(options.querySelectorAll<HTMLButtonElement>('button'));
    const i = answerIndex(buttons.map((b) => b.querySelector('b')?.textContent ?? ''), kind);
    if (i === null) { showToast(`This decision has no plain ${kind === 'yes' ? 'Yes' : 'No'} option. Choose one yourself.`); return; }
    if (!buttons[i].disabled) buttons[i].click();
  }
  /** Keyboard shortcuts the frame passes on (palette and pane toggles are the frame's own). */
  function handleShortcut(id: string) {
    if (id === 'approve-decision') answerShownDecision('yes');
    else if (id === 'deny-decision') answerShownDecision('no');
    else if (id === 'focus-composer') { const c = visibleComposer(); if (c) c.focus(); else showToast('No reply field is open here.'); }
    else if (id === 'next-needs-you') {
      const next = nextNeedsYou($allSessions, (k) => fleetOf($allAgents[k]) === 'needs-you', (k) => $allAgents[k]?.waitingSince ?? $allAgents[k]?.ts ?? 0, activeKey);
      if (next) void attach(next.key); else showToast('Nothing else needs you.');
    }
  }

  onMount(() => {
    const visible = () => (pageVisible = document.visibilityState !== 'hidden');
    document.addEventListener('visibilitychange', visible);
    const mq = window.matchMedia('(max-width: 720px)');
    const media = () => (narrow = mq.matches);
    media();
    mq.addEventListener('change', media);
    watchForUpdates();
    const stopRouter = startRouter();
    const clock = setInterval(() => {
      now = Date.now();
      const c = connection;
      const kind = c?.kind;
      quality = kind ? `${c?.pathLabel ?? (kind === 'rtc' ? 'Direct' : 'Tunnel')}${c?.rtt != null ? ` · ${c.rtt} ms` : ''}` : '';
    }, 1000);
    const vv = window.visualViewport;
    const kb = () => { keyboardOpen = !!vv && window.innerHeight - vv.height > 150; };
    vv?.addEventListener('resize', kb); kb();
    const themeMedia = window.matchMedia('(prefers-color-scheme: light)');
    let currentTheme = 'dark' as 'dark' | 'light' | 'system';
    let currentSkin = 'navy' as Skin;
    const themeChanged = () => applyTheme(currentTheme, themeMedia.matches, currentSkin);
    const stopTheme = settings.subscribe(value => { currentTheme = value.theme; currentSkin = value.skin; themeChanged(); });
    themeMedia.addEventListener('change', themeChanged);
    const stopUsage = startUsageMeter(() => !!selectedServer && connState === 'connected' && !!activeSessionId);
    // Last line of defence: an error nobody caught must not leave a dead screen without a word.
    let lastReport = 0;
    const report = (e: ErrorEvent | PromiseRejectionEvent) => {
      const text = String('message' in e ? e.message : (e.reason?.message ?? e.reason ?? ''));
      console.error('uncaught', text);
      noteError(text);
      if (text.includes('ResizeObserver') || Date.now() - lastReport < 5000) return;
      lastReport = Date.now();
      showToast('Something went wrong. If the screen looks stuck, go back to Servers.', 'error', scrub(text).slice(0, 300) || undefined);
    };
    window.addEventListener('error', report);
    window.addEventListener('unhandledrejection', report);
    return () => {
      clearInterval(clock);
      stopRouter();
      document.removeEventListener("visibilitychange", visible);
      vv?.removeEventListener('resize', kb);
      stopTheme();
      themeMedia.removeEventListener('change', themeChanged);
      stopUsage();
      hub.close();
      window.removeEventListener('error', report);
      window.removeEventListener('unhandledrejection', report);
      mq.removeEventListener('change', media);
    };
  });

  /**
   * A link pasted into a tab where the app is already open only changes the fragment (no reload), so
   * the load-time handling below never sees it.
   */
  onMount(() => {
    const onHash = () => {
      const claim = parseClaim(window.location.hash);
      const serverId = parseServerLink(window.location.hash);
      const pair = parsePairLink(window.location.hash);
      if (!claim && !serverId && !pair) return;
      window.history.replaceState({}, '', window.location.pathname);
      if (pair) {
        if (!user) sessionStorage.setItem('termenbox_pending_pair', `#/pair/${pair.payload}${pair.claim ? `?claim=${pair.claim}` : ''}`);
        else pairLink = pair;
      } else if (!user) {
        // Signed out: keep it across the GitHub login, as a load with the fragment does.
        if (serverId) sessionStorage.setItem('termenbox_pending_server', serverId);
        else if (claim) sessionStorage.setItem('termenbox_pending_claim', claim);
      } else if (serverId) {
        void addServerById(serverId);
      } else if (claim) {
        addPrefill = claim;
        showAddServer = true;
      }
    };
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  });

  onMount(async () => {
    // A claim link (#claim=...), from a QR opened in the camera app, is kept across the GitHub login redirect.
    const claim = parseClaim(window.location.hash);
    if (claim) {
      sessionStorage.setItem('termenbox_pending_claim', claim);
      window.history.replaceState({}, '', window.location.pathname);
    }

    // A pairing link (#/pair/...): kept across the GitHub login redirect, then verified and previewed before anything connects.
    const pair = parsePairLink(window.location.hash);
    if (pair) {
      sessionStorage.setItem('termenbox_pending_pair', `#/pair/${pair.payload}${pair.claim ? `?claim=${pair.claim}` : ''}`);
      window.history.replaceState({}, '', window.location.pathname);
    }

    // A re-add link (#server=...) from the installer or a QR, kept across the GitHub login redirect.
    const serverId = parseServerLink(window.location.hash);
    if (serverId) {
      sessionStorage.setItem('termenbox_pending_server', serverId);
      window.history.replaceState({}, '', window.location.pathname);
    }

    let authed = false;
    if (window.location.pathname === '/callback') authed = await handleOAuthCallback();
    else authed = checkAuth();
    if (!authed) return;

    loadServers();
    hub.start(servers);
    const pendingPair = parsePairLink(sessionStorage.getItem('termenbox_pending_pair') ?? '');
    if (pendingPair) {
      sessionStorage.removeItem('termenbox_pending_pair');
      pairLink = pendingPair;
      return;
    }
    const pendingServer = sessionStorage.getItem('termenbox_pending_server');
    if (pendingServer) {
      sessionStorage.removeItem('termenbox_pending_server');
      void addServerById(pendingServer);
      return;
    }
    const pending = sessionStorage.getItem('termenbox_pending_claim');
    if (pending) {
      sessionStorage.removeItem('termenbox_pending_claim');
      addPrefill = pending;
      showAddServer = true;
    }
  });
</script>

<AppShell pinned={!!user && servers.length > 0}>
  {#if !user}
    <div class="auth-screen">
      <div class="card" style="max-width: 400px; margin: auto; margin-top: 10vh;">
        <h1 style="margin-bottom: 8px;">Termenbox</h1>
        <p style="color: var(--fg-muted); margin-bottom: 24px;">Terminal for remote AI coding agents</p>
        <a href="https://github.com/login/oauth/authorize" on:click|preventDefault={loginWithGitHub}>
          <button style="width: 100%; display: flex; align-items: center; justify-content: center; gap: 8px;">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor"><path d="M12 0C5.374 0 0 5.373 0 12c0 5.302 3.438 9.8 8.207 11.387.599.111.793-.261.793-.577v-2.234c-3.338.726-4.033-1.416-4.033-1.416-.546-1.387-1.333-1.756-1.333-1.756-1.089-.745.083-.729.083-.729 1.205.084 1.839 1.237 1.839 1.237 1.07 1.834 2.807 1.304 3.492.997.107-.775.418-1.305.762-1.604-2.665-.305-5.467-1.334-5.467-5.931 0-1.311.469-2.381 1.236-3.221-.124-.303-.535-1.524.117-3.176 0 0 1.008-.322 3.301 1.23A11.509 11.509 0 0112 5.803c1.02.005 2.047.138 3.006.404 2.291-1.552 3.297-1.23 3.297-1.23.653 1.653.242 2.874.118 3.176.77.84 1.235 1.911 1.235 3.221 0 4.609-2.807 5.624-5.479 5.921.43.372.823 1.102.823 2.222v3.293c0 .319.192.694.801.576C20.566 21.797 24 17.3 24 12c0-6.627-5.373-12-12-12z"/></svg>
            Continue with GitHub
          </button>
        </a>
      </div>
    </div>
  {:else}
    <AppFrame current={$destination} onnavigate={go} servers={$hubServers} sessions={$allSessions} agents={$allAgents} {activeKey} chrome={servers.length > 0} inSession={!!activeKey && !away} {keyboardOpen}
      canCreate={sessionDestinations($hubServers, selected).length > 0} bind:wide bind:helpOpen onopen={attach} onserver={openServer} onaction={runAction} onshortcut={handleShortcut}
      onback={() => { showWorkspace(); sheetHeight = 'half'; }}>
    {#if $destination === 'sessions' || servers.length === 0}
    <NavigationBar sessions={tabSessions} agents={$allAgents} servers={$hubServers} windows={$allWindows} {expiring} {link} {activeKey} held={!!actionSession || !!menuSession || cardExpanded || sheetHeld} onopen={attach} onpeek={peek} ontabmenu={openWindowMenu} onactions={(key) => { actionConfirm = false; actionKey = key; }}
      onlink={() => link && showToast(link.text, link.ok ? 'success' : 'error')} onmenu={() => { titleTapped(); menuOpen = true; }} expanded={wide ? $layout.sidebarOpen : sheetHeight !== 'closed'} controls={wide && $layout.sidebarOpen ? 'workspace-sidebar' : undefined} onsessions={() => { if (wide) { toggleSidebar(); return; } crumb('sessions sheet toggled'); showWorkspace(); sheetHeight = sheetHeight === 'closed' ? 'half' : 'closed'; }} />
    {/if}
    <NeedsYouBanner {waiting} {now} collapsed={keyboardOpen} onopen={attach} onhome={() => go('home')} ondismiss={(key, id) => sendAttention(key, { type: 'dismiss', attention_id: id })} onanswer={(key, choice, prompt) => sendSession({ type: 'answer', session: key, prompt_id: prompt.id, seq: prompt.seq, choice })} />

    {#if iosHint && !selectedServer}
      <div style="display: flex; align-items: center; gap: 12px; padding: 10px 16px; background: var(--bg-secondary); border-bottom: 1px solid var(--border); font-size: 0.85rem;">
        <span style="flex: 1;">Install Termenbox: tap <strong>Share</strong>, then <strong>Add to Home Screen</strong>. It starts faster and can notify you.</span>
        <button class="secondary" style="padding: 4px 10px;" on:click={() => { iosHint = false; localStorage.setItem('termenbox_ios_hint', '1'); }}>Dismiss</button>
      </div>
    {/if}

    {#if $overLimit}
      <div class="connection-notice" role="status">{$connectedCount} servers connected. More than 6 may use extra battery. <button class="secondary" on:click={leaveServer}>Manage connections</button></div>
    {/if}
    <main style="flex: 1; min-height: 0; display: flex; flex-direction: column;">
      <svelte:boundary onerror={(e) => { console.error('screen failed', e); noteError(String((e as Error)?.message ?? e)); }}>
      {#snippet failed(_error, reset)}
        <div class="card" style="margin: auto; max-width: 360px; text-align: center;">
          <p style="margin-bottom: 12px;">This screen failed to load.</p>
          <button on:click={() => { leaveServer(); reset(); }}>Back to servers</button>
          <button class="secondary" on:click={openReport}>Report a problem</button>
        </div>
      {/snippet}
      {#if serversPage || servers.length === 0}
        {#if servers.length > 0}<button class="secondary" on:click={showWorkspace}>← Sessions</button>{/if}
        <ServerList {servers} states={$hubServers} onpause={(id) => { hub.pause(id); }} onresume={(id) => hub.resume(id)} onretry={(id) => hub.retry(id)} onadd={() => { addPrefill = null; showAddServer = true; }} onremove={removeServer} onshare={shareServer} />
      {:else if $destination === 'home'}
        <Home sessions={$allSessions} agents={$allAgents} servers={$hubServers} {now} {lastKey} offer={hookOffer} onhook={homeHook} onopen={attach} onservers={leaveServer} onnew={newSession}
          onanswer={(key, choice, prompt) => sendSession({ type: 'answer', session: key, prompt_id: prompt.id, seq: prompt.seq, choice })}
          onreply={(key, text) => sendSession({ type: 'reply', session: key, text })} />
      {:else if $destination === 'activity'}
        <ActivityView servers={$hubServers} history={historyMap} {now} onask={askHistory} caches={warmCaches} ttlMs={$settings.cacheTtlMs} health={healthMap} onhealth={askHealth} rttOf={(id) => hub.rtt(id)} />
      {:else if $destination === 'settings'}
        <SettingsView user={user ?? ''} {serverVersion} hooks={hookSettings} onhook={settingsHook} onservers={leaveServer} onmenu={() => { titleTapped(); menuOpen = true; }} onshortcuts={() => (helpOpen = true)} onreport={openReport} onlogout={logout} ondeletedata={deleteData} historyServers={$connectedCount} ondeletehistory={deleteHistory} />
      {:else}
        <div class="workspace" class:mobile={narrow}>
          <div class="terminal-pane">
            {#if !activeKey}
              <SessionGroups sessions={$allSessions} agents={$allAgents} {now} unhealthy={unhealthyServers} onopen={attach} onnew={newSession} />
            {:else if connection && connState !== 'paused' && activeIsAcp && activeSession}
              {#key activeKey}
                <AcpView sessionKey={activeSession.key} name={activeSession.name} agent={activeRow?.agent} fleet={activeRow?.state} stream={$acpStreams[activeSession.key]} {connState} {now} {canShell}
                  onprompt={(text) => sendAcp({ type: 'acp_prompt', session: activeSession.key, text })} oninterrupt={() => sendAcp({ type: 'acp_interrupt', session: activeSession.key })}
                  onanswer={(promptId, optionId) => sendAcp({ type: 'acp_answer', session: activeSession.key, prompt_id: promptId, option_id: optionId })}
                  onmode={(mode) => sendAcp({ type: 'acp_mode', session: activeSession.key, mode })} onshell={() => openShell(activeSession.key)} />
              {/key}
            {:else if connection && connState !== 'paused'}
              {#if !(keyboardOpen && paneMode === 'terminal')}
                <div class="pane-switch">
                  {#if !split}<Segmented label="Session view" value={paneMode} onchange={setPaneMode} options={[{ value: 'decision', label: 'Decision' }, { value: 'terminal', label: 'Terminal' }]} />{/if}
                  {#if wide && activeRow}
                    <button type="button" class="secondary split-toggle" aria-pressed={$layout.splitOpen} on:click={toggleSplit}>Terminal beside the decision</button>
                    {#if $layout.splitOpen && !split}<span class="split-note" role="status">Too narrow to show both. Hide the sidebar or widen the window.</span>{/if}
                  {/if}
                </div>
              {/if}
              <div class="pane-body" class:split data-pane-body data-split={split} bind:clientWidth={bodyWidth}>
                {#if (paneMode === 'decision' || split) && activeSession && activeRow}
                  <div class="pane-decision" data-pane="decision">
                    <SessionView name={activeSession.name} agent={activeRow.agent} fleet={activeRow.state} task={activeRow.title} startedAt={$allAgents[activeSession.key]?.task?.started_at}
                      waitingSince={activeRow.since} step={stepOf($allAgents[activeSession.key])} {now} view={heldView}
                      onreply={(line) => sendSession({ type: 'reply', session: activeSession.key, text: line })}
                      onanswer={(m) => hub.sendToSession(activeSession.key, m)} onterminal={() => setPaneMode('terminal')} />
                  </div>
                {/if}
                {#if split}
                  <Resizer label="Terminal width" controls="pane-terminal" value={splitW} min={SPLIT.min} max={splitMax} def={Math.min(SPLIT.def, splitMax)} step={SPLIT.step} grow={-1}
                    onchange={(w) => patchLayout({ splitWidth: w }, false)} oncommit={commitLayout} />
                {/if}
                <!-- One terminal for the session, always here: it is only restyled between full width, hidden and the right half, so crossing 720 px or toggling the split never rebuilds it. -->
                <div class="pane-terminal" id="pane-terminal" class:pane-hidden={paneMode === 'decision' && !split} inert={paneMode === 'decision' && !split} data-pane="terminal" style:width={split ? `${splitW}px` : undefined}>
                  {#key identityOf(connection)}<Terminal {connection} {activeSessionId} {connState} ontyped={typedByHand} {profile} prefix={selected?.prefix || 'C-b'} {preview} />{/key}
                </div>
              </div>
            {:else}
              <div class="card" style="margin: auto;">{activeKey ? 'This server is paused. Resume it in Servers.' : 'Choose a session to open its terminal.'}<button class="secondary" on:click={leaveServer}>Servers</button></div>
            {/if}
          </div>
          {#if !wide && activeKey}
            <SessionSheet sessions={$allSessions} agents={$allAgents} servers={$hubServers} windows={$allWindows} onpeek={peek} onwarm={keepWarm} {activeKey} mobile={narrow} bind:height={sheetHeight} held={!!actionSession}
              onexpanded={(open) => cardExpanded = open} onheld={(held) => sheetHeld = held} onopen={attach} onnew={newSession} onrefresh={() => { for (const s of $hubServers) hub.send(s.id, { type: 'list_sessions' }); }} onretry={(id) => hub.retry(id)} send={sendSession} />
          {/if}
        </div>
      {/if}
      </svelte:boundary>
    </main>
    </AppFrame>
  {/if}

  {#if menuOpen && user}<MenuSheet {user} {avatar} {serverVersion} onservers={leaveServer} onreport={openReport} onlogout={logout} onclose={() => menuOpen = false} unread={unreadCount($inbox)} oninbox={() => inboxOpen = true} push={pushView} onpush={togglePush} />{/if}
  {#if inboxOpen}<InboxSheet items={$inbox} {now} agents={$allAgents} onanswer={(key, choice, prompt) => sendSession({ type: 'answer', session: key, prompt_id: prompt.id, seq: prompt.seq, choice })} onreply={(key, text) => sendSession({ type: 'reply', session: key, text })} onopen={attach} onhome={() => go('home')} onread={() => inbox.update(markAllRead)} onclear={() => inbox.set([])} onclose={() => { inboxOpen = false; inbox.update(markAllRead); }} />{/if}
  {#if actionSession}<SessionActions session={actionSession} canClose={['tmux', 'acp'].includes(backendOf(actionSession))} confirmFirst={actionConfirm} ondetach={() => detach(actionSession.key)} onclosesession={() => closeSession(actionSession.key)} onclose={() => actionKey = null} />{/if}
  {#if menuSession && menuKey}
    <WindowMenu session={menuSession} data={$allWindows[menuSession.key]} supported={backendOf(menuSession) === 'tmux'}
      onselect={(index) => hub.sendToSession(menuSession.key, { type: 'select_window', session: menuSession.key, index })}
      canNewWindow={serverAtLeast($hubServers.find(h => h.id === menuSession.serverId)?.version ?? '', NEW_WINDOW_VERSION)}
      onnewwindow={() => newWindow(menuSession.key)}
      onclosewindow={(index) => hub.sendToSession(menuSession.key, { type: 'close_window', session: menuSession.key, index })}
      ondetach={() => detach(menuSession.key)} onclosesession={() => { actionConfirm = true; actionKey = menuSession.key; }} onclose={() => menuKey = null} />
  {/if}
  {#if showNew}
    <NewSessionSheet servers={newSessionTargets($hubServers).servers} initialServerId={newTarget.serverId} locked={newTarget.locked} sessions={$allSessions} windows={$allWindows} oncreate={createSession} onclose={() => showNew = false} />
  {/if}

  {#if reportText !== null}
    <ReportDialog text={reportText} onclose={() => (reportText = null)} />
  {/if}

  {#if pairLink}
    <PairDialog link={pairLink} onclose={() => pairLink = null} onadd={(server) => { pairLink = null; void addPaired(server); }} />
  {/if}
  {#if showAddServer}
    <AddServerModal origin={location.origin} prefillSecret={addPrefill} onserver={(id) => { showAddServer = false; void addServerById(id); }} onclose={() => showAddServer = false} onadd={(server) => void addAndLink(server)} />
  {/if}

  {#if toast}
    <Toast {toast} onclose={() => toast = null} onhold={holdToast} />
  {/if}
</AppShell>

<style>
  .workspace { display: flex; flex: 1; min-height: 0; overflow: hidden; position: relative; }
  .workspace.mobile { flex-direction: column; }
  .terminal-pane { flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }
  .pane-switch { padding: 6px 12px; border-bottom: 1px solid var(--line); background: var(--bg); display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  .split-toggle { min-height: 44px; }
  .split-note { font-size: 0.85rem; color: var(--fg-muted); }
  .pane-body { position: relative; flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .pane-terminal { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  /* Hidden, not detached and not resized: xterm keeps its size, its buffer and its connection. */
  .pane-terminal.pane-hidden { visibility: hidden; pointer-events: none; }
  .pane-decision { position: absolute; inset: 0; background: var(--bg); }
  .pane-body.split { flex-direction: row; }
  .pane-body.split .pane-decision { position: static; flex: 1 1 0; min-width: 0; }
  .pane-body.split .pane-terminal { flex: 0 0 auto; min-width: 0; }
  .connection-notice { padding: 8px; font-size: 0.8rem; overflow-wrap: anywhere; }

  .auth-screen {
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg);
  }
</style>
