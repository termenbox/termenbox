<script lang="ts">
  import { onDestroy, onMount, tick } from 'svelte';
  import Terminal from './Terminal.svelte';
  import ServerList from './ServerList.svelte';
  import AddServerModal from './AddServerModal.svelte';
  import PairDialog from './PairDialog.svelte';
  import { Toast } from './lib/ui';
  import WorkspaceSheet from './WorkspaceSheet.svelte';
  import type { SheetHeight } from './lib/sheet';
  import NavigationBar from './NavigationBar.svelte';
  import MenuSheet from './MenuSheet.svelte';
  import InboxSheet from './InboxSheet.svelte';
  import { inbox, mergeAttention, markAllRead, unreadCount } from './lib/inbox';
  import { currentEndpoint, disablePush, enablePush, pushSupported } from './lib/push';
  import WorkspaceActions from './WorkspaceActions.svelte';
  import WindowMenu from './WindowMenu.svelte';
  import NeedsYouBanner from './NeedsYouBanner.svelte';
  import NewWorkspaceSheet from './NewWorkspaceSheet.svelte';
  import NewTabSheet from './NewTabSheet.svelte';
  import RenameDialog from './RenameDialog.svelte';
  import TabMenu from './TabMenu.svelte';
  import { SnapshotStore } from './lib/snapshots';
  import { serverAtLeast, NEW_WINDOW_VERSION } from './lib/serverVersion';
  import { loadHidden, needsYou, openTabs, saveHidden } from './lib/tabs';
  import { focusedTab, liveTab, loadActiveTab, saveActiveTab, tabForPrompt, tabOrder } from './lib/workspaceTabs';
  import { windowToOpen } from './lib/windows';
  import { installAudioUnlock, onAudioRunning, playChime } from './lib/bell';
  import { announce, waitingList, createAttention, createNotifier, createAlerter } from './lib/notify';
  import { attentionToAck } from './lib/agents';
  import { cacheRemaining, expiringSoon } from './lib/cache';
  import { settings, applyTheme, type Skin } from './lib/settings';
  import type { ServerListItem } from './lib/protocol';
  import { parseClaim, parseServerLink, serverCode, serverLink } from './lib/onboard';
  import { sendServerLink } from './lib/share';
  import { DirClient } from './lib/dirClient';
  import { ServerHub, newWorkspaceTargets, serverKey, workspaceDestinations, workspaceKey, type HubServer, type HubWorkspace } from './lib/hub';
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
  import WorkspaceGroups from './WorkspaceGroups.svelte';
  import { moveWorkspace, placementOf } from './lib/order';
  import { destination, navigate, parseFocusLink, parseFocusTab, startRouter, type Destination } from './lib/router';
  import ReportDialog from './ReportDialog.svelte';
  import ServerDiagnosticsDialog from './ServerDiagnosticsDialog.svelte';
  import ExitBanner from './ExitBanner.svelte';
  import { BundleReader, SeenExits, exitText, type ExitNotice } from './lib/diagnostics';
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

  // Server diagnostics (decision 0144): asked for by the owner, read before it is downloaded or attached to an issue.
  const seenExits = new SeenExits((() => { try { return localStorage; } catch { return null; } })());
  const bundleReader = new BundleReader();
  let exitNotice: ExitNotice | null = null;
  let diagnostics: { serverId: string; text: string | null; error: string } | null = null;
  function openDiagnostics(serverId?: string) {
    exitNotice = null;
    const id = serverId ?? activeServerId ?? $hubServers.find(s => s.state === 'connected')?.id;
    if (!id || $hubServers.find(s => s.id === id)?.state !== 'connected') {
      diagnostics = { serverId: id ?? '', text: null, error: 'Connect to a server first.' };
      return;
    }
    crumb('server diagnostics opened');
    diagnostics = { serverId: id, text: null, error: '' };
    hub.send(id, { type: 'diagnostics_bundle' });
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
  /** The Workspaces list page is showing instead of the open workspace (the bottom bar's Workspaces); attach() clears it. */
  let listPage = false;
  $: away = serversPage || $destination !== 'workspaces' || listPage;
  $: if ($destination !== 'settings') serversPage = false;
  // `list_dir` for the folder picker: one in-order request queue per server; its replies come back through onMessage.
  const dirClients = new Map<string, DirClient>();
  const listDir = (serverId: string, req: { path?: string; prefix?: string; hidden?: boolean }) => {
    let c = dirClients.get(serverId);
    if (!c) dirClients.set(serverId, c = new DirClient(m => hub.send(serverId, m)));
    return c.list(req);
  };
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
        if (!away && id === activeServerId && key === activeKey) showWorkspace(key, activeTabId ?? undefined);
      });
    },
    onDrop: (_id, reason, upMs) => { dirClients.get(_id)?.fail('The connection was lost.'); noteError(`connection lost after ${Math.round(upMs / 1000)} s: ${reason}`); },
    onFatal: (_id, reason) => { noteError(`connection: ${reason}`); showToast(reason, 'error'); },
    onMessage: (_id, msg) => {
      const id = _id;
      dirClients.get(_id)?.handle(msg);
      if (msg.type === 'snapshot') snapshots.apply(workspaceKey(_id, msg.workspace), msg);
      else if (msg.type === 'workspace_attached') { preview = null; if (id === activeServerId && msg.workspace_id === activeWorkspaceId && msg.tab) { activeTab = msg.tab; saveActiveTab(workspaceKey(id, msg.workspace_id), msg.tab); } }
      else if (msg.type === 'workspace_view') { if (id === activeServerId && msg.workspace === activeWorkspaceId) heldView = mergeView(heldView, msg); }
      else if (msg.type === 'acp_view') acpStreams.update(m => applyAcp(m, workspaceKey(_id, msg.workspace), msg.workspace, msg));
      else if (msg.type === 'workspaces') { openPending(_id, msg.workspaces); openPendingTab(_id, msg.workspaces); syncActiveTab(_id, msg.workspaces); acpStreams.update(m => forgetAcp(m, `${_id}/`, new Set(msg.workspaces.map(x => workspaceKey(_id, x.id))))); }
      else if (msg.type === 'attention') inbox.update(l => mergeAttention(l, _id, msg.items, s => workspaceKey(_id, s), s => hub.findWorkspace(workspaceKey(_id, s))?.name ?? s));
      else if (msg.type === 'history') historyMap = answered(historyMap, _id, msg);
      else if (msg.type === 'health_history') healthMap = answeredHealth(healthMap, _id, msg);
      else if (msg.type === 'history_deleted') historyDeletes.reply(_id, msg.removed);
      else if (msg.type === 'push_info') pushInfos = { ...pushInfos, [_id]: msg };
      else if (msg.type === 'push_settings') {
        pushSettings = { ...pushSettings, [_id]: msg.settings };
        if (pushSaving) showToast('Push reminders saved');
        pushSaving = false;
      }
      else if (msg.type === 'push_settings_rejected') {
        pushSaving = false;
        showToast(msg.reason, 'error');
      }
      else if (msg.type === 'agent_hooks') {
        hookMap = applyHookReply(hookMap, _id, msg);
        // After a removal the next Enable needs the exact change again.
        if (msg.action === 'remove' && !msg.error) hub.send(_id, { type: 'agent_hooks', agent: msg.agent, action: 'preview' });
      }
      if (msg.type === 'previous_exit') { if (seenExits.announce(_id, msg.at_ms)) exitNotice = { serverId: _id, atMs: msg.at_ms, streak: msg.streak, crashed: msg.crashed }; }
      else if (msg.type === 'diagnostics_bundle') {
        const whole = bundleReader.add(msg.part, msg.parts, msg.text);
        if (whole !== null && diagnostics?.serverId === _id) diagnostics = { ...diagnostics, text: whole };
      }
      else if (msg.type === 'error' && diagnostics && diagnostics.serverId === _id && diagnostics.text === null && !diagnostics.error) diagnostics = { ...diagnostics, error: msg.message };
      else if (msg.type === 'error') showToast(msg.message, 'error');
      else if (msg.type === 'agent_event') showToast(`Agent ${msg.agent} ${msg.event} in ${msg.workspace_id}`);
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
  $: hookOffer = (void offerTick, void now, pendingOffer($hubServers, $allWorkspaces, $allAgents, hookMap, hookOffers));
  $: hookSettings = (void offerTick, hookRows($hubServers, hookMap, hookOffers));
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
  /** Keys went to the open workspace: while its agent waits on a prompt that is the user answering by hand. */
  function typedByHand() {
    const a = activeKey ? $allAgents[activeKey] : undefined;
    const s = activeKey ? hub.findWorkspace(activeKey) : undefined;
    if (!a?.agent || !s || !waitingOnPrompt(a)) return;
    hookOffers.manualAnswer(s.serverId, a.agent);
    offerTick++;
  }
  // The in-workspace screen: Terminal (default) or Decision, kept per workspace on this device. The terminal stays mounted
  // in both (hidden, never rebuilt), so a switch cannot lose its state or its focus rules.
  let paneModes = loadPaneModes();
  $: paneMode = (activeKey && paneModes[activeKey]) || 'terminal' as PaneMode;
  function setPaneMode(m: string) { if (activeKey) paneModes = withPaneMode(paneModes, activeKey, m === 'decision' ? 'decision' : 'terminal'); }
  let heldView: ViewModel | null = null;
  $: activeKey, (heldView = null);
  $: viewPolling = !!activeKey && !away && pageVisible && (paneMode === 'decision' || split) && connState === 'connected' && !activeIsAcp;
  let viewPoll: ReturnType<typeof setInterval> | undefined;
  function requestWorkspaceView() { if (activeKey) hub.sendToWorkspace(activeKey, requestView(activeWorkspaceId ?? '', heldView)); }
  $: { activeKey; if (viewPoll) clearInterval(viewPoll); viewPoll = undefined; if (viewPolling) { requestWorkspaceView(); viewPoll = setInterval(requestWorkspaceView, 2000); } }
  $: activeRow = activeWorkspace ? rowOf(activeWorkspace, $allAgents[activeWorkspace.key], false, $allTabStates) : null;
  /** The last screen seen for the workspace being opened, dimmed in the terminal until the live repaint replaces it. */
  let preview: string | null = null;
  const hubServers = hub.servers;
  const allWindows = hub.windows;
  const allWorkspaces = hub.allWorkspaces;
  $: workspaceCounts = $allWorkspaces.reduce<Record<string, number>>((n, s) => ({ ...n, [s.serverId]: (n[s.serverId] ?? 0) + 1 }), {});
  const allAgents = hub.agents;
  const allTabStates = hub.tabStates;
  const connectedCount = hub.connectedCount;
  const overLimit = hub.overLimit;
  $: selected = $hubServers.find(s => s.id === activeServerId);
  $: selectedServer = selected?.server ?? null;
  $: tabsAnywhere = $hubServers.some((s) => s.tabs);
  /** Use the tab UI for a server that serves tabs; an older server keeps the old strip and Workspaces button. */
  $: tabsCapable = !!selected?.tabs || (!activeKey && tabsAnywhere);
  /** Whether a server can take a new workspace now: the sidebar, the palette and the strip hide their create action rather than offering a toast that says no. */
  $: canCreate = workspaceDestinations($hubServers, selected).length > 0;
  $: workspaces = $allWorkspaces.map(s => ({ ...s, id: s.key }));
  $: activeWorkspaceId = $allWorkspaces.find(s => s.key === activeKey)?.id ?? null;
  $: connection = selected ? hub.connection(selected.id) : null;
  $: connState = selected?.state ?? 'closed';
  $: serverVersion = selected?.version ?? '';
  let narrow = false;
  /** Beside the decision, the live terminal (U15): only on the desktop, for a workspace with a structured view, and wide enough for both halves. */
  let bodyWidth = 0;
  $: split = wide && $layout.splitOpen && !!activeWorkspace && !!activeRow && (bodyWidth === 0 || canSplit(bodyWidth));
  $: splitW = splitWidthFor($layout.splitWidth, bodyWidth || 1200);
  $: splitMax = Math.max(SPLIT.min, Math.min(SPLIT.max, (bodyWidth || 1200) - MIN_HALF));
  let iosHint = isIosBrowser() && !localStorage.getItem('termenbox_ios_hint');
  let sheetHeight: SheetHeight = 'half';
  let now = Date.now();
  let hidden = loadHidden();
  let menuKey: string | null = null;
  /** The tab this device opened in the workspace; the server's focus is the fallback for a new device. */
  let activeTab: string | null = null;
  let tabMenuOpen = false;
  let tabMenuAll = false;
  let newTabFor: HubWorkspace | null = null;
  let renameTarget: { kind: 'workspace' | 'tab'; key: string; tab?: string; title: string; field: string; value: string } | null = null;
  /** A tab the user just asked for, opened once the server lists it with its id. */
  let pendingTab: { serverId: string; workspaceId: string; before: string[] } | null = null;
  let actionConfirm = false;
  let showNew = false;
  let pendingOpen: { serverId: string; name: string; before: string[] } | null = null;
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
  let pushSettings: Record<string, import('./lib/protocol').PushSettings> = {};
  let pushSaving = false;
  // Older servers do not answer push_settings_get, so ask only those that say they do (push_info.settings).
  $: if ($destination === 'settings' && pushServerId && pushInfos[pushServerId]?.settings) hub.send(pushServerId, { type: 'push_settings_get' });
  function savePushSettings(settings: import('./lib/protocol').PushSettings) {
    if (!pushServerId) return;
    pushSaving = true;
    hub.send(pushServerId, { type: 'push_settings_set', settings });
  }
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
  // Tell the servers which workspace is in front, so a push is not sent for what someone is looking at.
  let reportedFg: { serverId: string; workspace: string; tab?: string | null } | null = null;
  function reportForeground() {
    const s = activeKey && !away && pageVisible ? hub.findWorkspace(activeKey) : undefined;
    const next = s ? { serverId: s.serverId, workspace: s.id, tab: activeTabId } : null;
    if (next?.serverId === reportedFg?.serverId && next?.workspace === reportedFg?.workspace && next?.tab === reportedFg?.tab) return;
    if (reportedFg && reportedFg.serverId !== next?.serverId) hub.send(reportedFg.serverId, { type: 'foreground', workspace: null });
    if (next) hub.send(next.serverId, { type: 'foreground', workspace: next.workspace, tab: next.tab });
    reportedFg = next;
  }
  $: activeKey, away, pageVisible, activeTabId, reportForeground();
  let actionKey: string | null = null;
  let cardExpanded = false;
  $: actionWorkspace = $allWorkspaces.find(s => s.key === actionKey);
  $: menuWorkspace = $allWorkspaces.find(s => s.key === menuKey);
  const backendOf = (s: { backend?: string; serverId: string } | undefined) => s ? (s.backend ?? $hubServers.find(h => h.id === s.serverId)?.backends[0] ?? '') : '';
  // An ACP workspace (T0) has no terminal: opening it shows its stream (decision 0111). The stream is per connection, so it
  // is asked for again whenever the workspace is shown, and dropped from the server when it is not.
  $: activeIsAcp = !!activeWorkspace && backendOf(activeWorkspace) === 'acp';
  /** A single-agent ACP workspace has one agent tab and no tab actions until R75 (decision 0149). */
  $: canTab = !!activeWorkspace && backendOf(activeWorkspace) === 'tmux';
  let acpServer: string | null = null;
  function closeAcp() {
    if (acpServer) hub.send(acpServer, { type: 'acp_close' });
    acpServer = null;
  }
  /** Shows a workspace: an ACP workspace opens its stream, any other attaches its terminal. */
  function showWorkspace(key: string, tab?: string) {
    const s = hub.findWorkspace(key);
    if (!s) return;
    if (backendOf(s) === 'acp') {
      // A terminal still attached on this server would go on streaming to a view that is gone, keep its size and count
      // as someone looking: let go of it first.
      hub.send(s.serverId, { type: 'detach' });
      acpServer = s.serverId;
      hub.sendToWorkspace(key, openMessage(key, $acpStreams[key]));
    } else {
      closeAcp();
      hub.sendToWorkspace(key, { type: 'attach', workspace_id: key, tab });
    }
  }
  const sendAcp = (m: import('./lib/protocol').ClientMessage) => { if (activeKey) hub.sendToWorkspace(activeKey, m); };
  /** The companion shell (decision 0109): a plain tmux workspace in the ACP workspace's working directory, made on the first
   *  tap and opened on every tap after. ACP gives the owner no terminal of the agent's own. */
  function openShell(key: string) {
    const s = hub.findWorkspace(key);
    if (!s) return;
    const wanted = shellName(s.name);
    const existing = $allWorkspaces.find(x => x.serverId === s.serverId && backendOf(x) === 'tmux' && x.name === wanted);
    if (existing) { void attach(existing.key); return; }
    // Without the directory a new shell would open in the server's default one and look like the workspace's own.
    const dir = $acpStreams[key]?.info?.cwd;
    if (!dir) { showToast('The workspace’s directory is not known yet. Try again in a moment.', 'error'); return; }
    createWorkspace({ serverId: s.serverId, backend: 'tmux', dir, name: wanted });
  }
  $: canShell = !!activeWorkspace && ($hubServers.find(h => h.id === activeWorkspace.serverId)?.backends.includes('tmux') ?? false);
  $: tabWorkspaces = openTabs($allWorkspaces, $allAgents, hidden);
  $: waiting = waitingList($allWorkspaces, $allAgents);
  $: announce(waiting.length);
  const attention = typeof document === 'undefined' ? null : createAttention({
    get title() { return document.title; }, set title(t) { document.title = t; },
    icon: document.querySelector<HTMLLinkElement>('link[rel="icon"]'),
  });
  $: attention?.update(waiting.length, !pageVisible);
  const notifyWaiting = typeof Notification === 'undefined' ? null : createNotifier(
    (title, o) => new Notification(title, o),
    () => Notification.permission,
    (key) => { window.focus(); void openWaiting(key); },
  );
  $: notifyWaiting?.(waiting, !pageVisible, pushOn);
  const alertWaiting = createAlerter((kind) => playChime($settings.alertSoundId, kind), (pattern) => typeof navigator.vibrate === 'function' && navigator.vibrate(pattern), Date.now, (line) => console.info(line));
  $: alertWaiting(waiting, { sound: $settings.alertSound, vibrate: $settings.alertVibrate });
  // Looking at a workspace clears its finished or failed item on the server; a question stays until it is answered.
  let pageVisible = typeof document === 'undefined' || document.visibilityState !== 'hidden';
  const acked = new Set<string>();
  function sendAttention(key: string, m: import('./lib/protocol').ClientMessage) {
    const s = hub.findWorkspace(key);
    if (s) hub.send(s.serverId, m);
  }
  $: if (activeKey && !away) {
    for (const id of attentionToAck($allAgents[activeKey], true, pageVisible, acked, activeTabId)) {
      acked.add(id);
      sendAttention(activeKey, { type: 'ack', attention_id: id });
    }
  }
  // Idle Claude Code workspaces with a warm prompt cache, for Activity (an estimate from the last answer, decision 0083).
  $: warmCaches = $allWorkspaces.flatMap(s => { const a = $allAgents[s.key]; const r = a?.agent === 'claude' ? cacheRemaining(a, now, $settings.cacheTtlMs) : null; return r !== null && r > 0 ? [{ key: s.key, name: s.name, server: s.serverName, remainingMs: r }] : []; }).sort((a, b) => a.remainingMs - b.remainingMs);
  $: expiring = new Set($allWorkspaces.filter(s => expiringSoon(cacheRemaining($allAgents[s.key], now, $settings.cacheTtlMs))).map(s => s.key));
  $: activeWorkspace = $allWorkspaces.find(s => s.key === activeKey);
  $: activeTabs = activeWorkspace?.tabs ?? [];
  $: activeTabId = activeWorkspace?.tabs
    ? (liveTab(activeWorkspace.tabs, activeTab, activeWorkspace.focus)?.id ?? null)
    : null;
  $: tabMenuGroups = (tabMenuAll ? $allWorkspaces.filter((w) => (w.tabs?.length ?? 0) > 0) : (activeWorkspace ? [activeWorkspace] : []))
    .map((w) => ({
      workspace: w,
      tabs: w.tabs ?? [],
      // A single-agent ACP workspace has one agent tab and no tab actions until R75.
      editable: (w.backend ?? $hubServers.find((s) => s.id === w.serverId)?.backends[0] ?? '') === 'tmux',
    }));
  $: profile = backendOf(activeWorkspace) === 'tmux' ? 'tmux' as const : 'shell' as const;
  $: link = selected ? { ok: connState === 'connected', text: connState === 'connected' ? quality || 'connected' : connState } : null;
  $: if (hidden.size) { const live = new Set($allWorkspaces.map(s => s.key)); if ([...hidden].some(k => !live.has(k)) && $allWorkspaces.length) { hidden = new Set([...hidden].filter(k => live.has(k))); saveHidden(hidden); } }
  let showAddServer = false;
  /** A pairing link waiting for the user's confirmation (decision 0123). */
  let pairLink: PairLink | null = null;
  let addPrefill: string | null = null;
  let addStart: 'install' | 'scan' = 'install';
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
    showWorkspaceList();
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

  /** A push opened the app on a decision (U11): Home shows that workspace's card first, whether or not its server is connected yet. */
  let focusKey: string | null = null;
  /** A push that names a tab waits for that workspace to arrive from its server, then opens the tab. */
  const DEEP_TAB_WAIT_MS = 30_000;
  let pendingDeepTab: { key: string; tab: string } | null = null;
  let pendingDeepTimer: ReturnType<typeof setTimeout> | undefined;
  function clearPendingDeepTab() {
    if (pendingDeepTimer) clearTimeout(pendingDeepTimer);
    pendingDeepTimer = undefined;
    pendingDeepTab = null;
  }
  function holdPendingDeepTab(key: string, tab: string) {
    clearPendingDeepTab();
    pendingDeepTab = { key, tab };
    pendingDeepTimer = setTimeout(() => {
      pendingDeepTimer = undefined;
      if (pendingDeepTab?.key !== key || pendingDeepTab.tab !== tab) return;
      pendingDeepTab = null;
      showToast('The workspace from that notification did not open. Open it from Workspaces.', 'error');
    }, DEEP_TAB_WAIT_MS);
  }
  $: if (pendingDeepTab && hub.findWorkspace(pendingDeepTab.key)) {
    const p = pendingDeepTab;
    clearPendingDeepTab();
    void openWaiting(p.key, p.tab);
  }
  // Leaving Home for another top-level page or the servers list abandons the deep link instead of opening it later.
  $: if (pendingDeepTab && (serversPage || $destination === 'settings' || $destination === 'activity')) clearPendingDeepTab();
  onDestroy(clearPendingDeepTab);
  function openDecision(key: string) {
    focusKey = key;
    navigate('home');
  }
  $: if ($destination !== 'home') focusKey = null;

  const LAST_KEY = 'termenbox_last_session';
  // The app opens on Home; its Continue card offers the workspace last used (decision 0100). Nothing attaches by itself.
  let lastKey: string | null = localStorage.getItem(LAST_KEY);
  function forget(key: string) { if (localStorage.getItem(LAST_KEY) === key) { localStorage.removeItem(LAST_KEY); lastKey = null; } }

  /** Opens a workspace that needs you on the window the need is in: the tab's window (the agent window used last), not
   *  whichever one tmux has current. Chosen before the attach, so the terminal comes up on it. */
  async function openWaiting(key: string, tabWanted?: string) {
    const workspace = hub.findWorkspace(key);
    const tab = tabWanted ?? (workspace?.tabs ? liveTab(workspace.tabs, loadActiveTab(key), workspace.focus)?.id : null);
    if (!tab && workspace && needsYou($allAgents[key]) && backendOf(workspace) === 'tmux') {
      // An older server has windows, not tabs.
      const index = windowToOpen($allWindows[key]);
      if (index !== null) hub.sendToWorkspace(key, { type: 'select_window', workspace: key, index });
    }
    await attach(key, tab ?? undefined);
  }

  async function attach(key: string, tab?: string) {
    const workspace = hub.findWorkspace(key);
    if (!workspace) return;
    if ($hubServers.find(s => s.id === workspace.serverId)?.state !== 'connected') {
      showToast('This server is reconnecting. You can retry it in Servers.', 'error');
      return;
    }
    crumb('open workspace tab');
    closeAcp();
    sheetHeight = 'closed';
    prevAway = false; // this open sends its own attach below; the away/back watcher must not send a second
    if (hidden.has(key)) { hidden.delete(key); hidden = new Set(hidden); saveHidden(hidden); }
    if (activeServerId && activeServerId !== workspace.serverId) hub.send(activeServerId, { type: 'detach' });
    serversPage = false;
    listPage = false;
    navigate('workspaces');
    const chosen = tab ?? (workspace.tabs ? liveTab(workspace.tabs, loadActiveTab(key), workspace.focus)?.id : null);
    preview = key === activeKey ? null : snapshots.text(key);
    activeKey = key;
    activeServerId = workspace.serverId;
    activeTab = chosen ?? null;
    if (chosen) saveActiveTab(key, chosen);
    localStorage.setItem(LAST_KEY, key);
    lastKey = key;
    // Mount the receiving terminal before attach can produce a repaint on another server.
    await tick();
    if (activeKey === key) showWorkspace(key, chosen ?? undefined);
  }

  /** Opens one tab of the open workspace; a tab of another workspace opens that workspace first. */
  function openTab(key: string, tab: string) {
    if (key !== activeKey) { void attach(key, tab); return; }
    activeTab = tab;
    saveActiveTab(key, tab);
    preview = null;
    showWorkspace(key, tab);
  }

  /** Opens a window and shows it. The workspace is attached first when it is not the open one: attaching lands on the
   * tab's representative window, so attaching after the new window exists (or again on the open workspace) would
   * switch straight back to the old one. tmux moves the attached client to the new window by itself. */
  async function newWindow(key: string) {
    if (key !== activeKey) await attach(key);
    hub.sendToWorkspace(key, { type: 'new_window', workspace: key });
  }

  /** Fetches a workspace's screen while the finger is still going down on its tab or card (decision 0084). */
  function peek(key: string) {
    if (key === activeKey) return;
    const workspace = hub.findWorkspace(key);
    const tab = workspace?.tabs ? liveTab(workspace.tabs, loadActiveTab(key), workspace.focus)?.id : undefined;
    hub.sendToWorkspace(key, { type: 'peek', workspace: key, tab, hash: snapshots.hash(key) });
  }

  /** Detach: the tab closes and the stream stops; the workspace keeps running and stays in the Workspaces sheet. */
  function detach(key: string) {
    crumb('detach');
    forget(key);
    if (key === activeKey) closeAcp();
    hidden = new Set(hidden).add(key); saveHidden(hidden);
    if (key === activeKey) {
      if (activeServerId) hub.send(activeServerId, { type: 'detach' });
      activeKey = null; activeServerId = null; activeTab = null; preview = null;
      sheetHeight = 'half';
    }
  }

  /** Close workspace: it and everything in it ends (the dialog has asked first). */
  function closeWorkspace(key: string) {
    crumb('close workspace');
    forget(key);
    if (key === activeKey) closeAcp();
    hub.sendToWorkspace(key, { type: 'close_workspace', workspace: key });
    if (key === activeKey) { activeKey = null; activeServerId = null; activeTab = null; preview = null; sheetHeight = 'half'; }
  }

  /** The name clients show; the workspace's id, and so its tabs, layout and events, stay as they were. */
  function renameWorkspace(key: string, name: string) {
    crumb('rename workspace');
    hub.rename(key, name);
  }

  /** The sidebar's close button: the dialog asks first where the manager can close, and offers only Detach where it cannot. */
  function askCloseWorkspace(key: string) {
    actionConfirm = ['tmux', 'acp'].includes(backendOf(hub.findWorkspace(key)));
    actionKey = key;
  }

  function openWindowMenu(key: string) {
    crumb('window menu opened');
    menuKey = key;
    const s = hub.findWorkspace(key);
    if (backendOf(s) !== 'acp') hub.sendToWorkspace(key, { type: 'windows', workspace: key }); // an ACP workspace has no windows
  }

  function openTabMenu() {
    tabMenuAll = false;
    tabMenuOpen = true;
  }

  function openPageTabs() {
    tabMenuAll = true;
    tabMenuOpen = true;
  }

  function newTab(workspace: HubWorkspace) {
    newTabFor = workspace;
  }

  function createTab(req: { template: string; label: string; dir: string }) {
    const workspace = newTabFor;
    if (!workspace) return;
    pendingTab = {
      serverId: workspace.serverId,
      workspaceId: workspace.id,
      before: (workspace.tabs ?? []).map((t) => t.id),
    };
    hub.newTab(workspace.key, req.template, req.label, req.dir);
    newTabFor = null;
  }

  function askRenameWorkspace(workspace: HubWorkspace) {
    renameTarget = { kind: 'workspace', key: workspace.key, title: 'Rename workspace', field: 'Name', value: workspace.name };
  }

  function askRenameTab(workspaceKey: string, tab: { id: string; label: string }) {
    renameTarget = { kind: 'tab', key: workspaceKey, tab: tab.id, title: 'Rename tab', field: 'Name', value: tab.label };
  }

  function saveRename(name: string) {
    if (!renameTarget) return;
    if (renameTarget.kind === 'workspace') renameWorkspace(renameTarget.key, name);
    else hub.renameTab(renameTarget.key, renameTarget.tab!, name);
  }

  function moveTab(workspaceKey: string, tab: string, index: number) {
    hub.moveTab(workspaceKey, tab, index);
  }

  /** A freshly created tab is focused by the server; open it once the list carries its id. */
  function openPendingTab(serverId: string, list: import('./lib/protocol').WorkspaceInfo[]) {
    if (!pendingTab || pendingTab.serverId !== serverId) return;
    const workspace = list.find((w) => w.id === pendingTab!.workspaceId);
    const made = workspace?.tabs?.find((t) => !pendingTab!.before.includes(t.id));
    if (!made) return;
    pendingTab = null;
    void attach(workspaceKey(serverId, workspace!.id), made.id);
  }

  /** The visible tab was closed or moved: follow the focus the server chose, without a blank terminal. */
  function syncActiveTab(serverId: string, list: import('./lib/protocol').WorkspaceInfo[]) {
    if (!activeKey || !activeTab) return;
    const workspace = list.find((w) => w.id === activeWorkspaceId);
    if (!workspace?.tabs || workspace.tabs.some((t) => t.id === activeTab)) return;
    const next = liveTab(workspace.tabs, null, workspace.focus)?.id ?? null;
    activeTab = next;
    if (next) saveActiveTab(activeKey, next);
    if (!away && !listPage) {
      if (next) showWorkspace(activeKey, next);
    }
  }

  function createWorkspace(req: { serverId: string; backend?: string; dir?: string; template?: string; name: string }) {
    // The server chooses the id; the new workspace is the listed one with this name that was not there before.
    pendingOpen = { serverId: req.serverId, name: req.name, before: $allWorkspaces.filter(w => w.serverId === req.serverId).map(w => w.id) };
    hub.send(req.serverId, { type: 'new_workspace', name: req.name, backend: req.backend, dir: req.dir, template: req.template });
  }
  /** The workspace the user just created opens by itself once the server lists it. */
  function openPending(serverId: string, list: import('./lib/protocol').WorkspaceInfo[]) {
    if (!pendingOpen || pendingOpen.serverId !== serverId) return;
    const made = list.find(s => s.name === pendingOpen!.name && !pendingOpen!.before.includes(s.id));
    if (!made) return;
    pendingOpen = null;
    void attach(workspaceKey(serverId, made.id));
  }

  function keepWarm(key: string) {
    if ($settings.keepWarm) hub.sendToWorkspace(key, { type: 'reply', workspace: key, text: $settings.keepWarm });
  }

  function sendWorkspace(m: import('./lib/protocol').ClientMessage) {
    if (!('workspace' in m) || !m.workspace) return;
    let out = m;
    // An answer from Home or the inbox belongs to the tab that asked; the workspace answer would otherwise be
    // ambiguous when several tabs of one workspace are open (decision 0149).
    if (m.type === 'answer' && !m.tab) {
      const s = hub.findWorkspace(m.workspace);
      const states = s ? Object.values($allTabStates).filter((t) => t.workspace === s.id) : [];
      // An older server has no per-tab states: keep its workspace-level answer. With tabs, an answer is only sent
      // when the prompt id still names the tab that asked; another waiting tab is never guessed.
      if (states.length) {
        const tab = tabForPrompt(states, m.prompt_id);
        if (!tab) {
          showToast('That question is no longer waiting. Open the tab that asked.', 'error');
          return;
        }
        out = { ...m, tab };
      }
    }
    hub.sendToWorkspace(m.workspace, out);
  }

  /** A name given on this device wins over the server's own (the hub keeps a saved name when it connects). */
  function renameServer(index: number, name: string) {
    const renamed = { ...servers[index], server_name: name };
    saveServers(servers.map((s, i) => (i === index ? renamed : s)));
    hub.add(renamed);
  }

  function removeServer(index: number) {
    const id = serverKey(servers[index]);
    hub.remove(id);
    saveServers(servers.filter((_, i) => i !== index));
    if (activeServerId === id) { activeKey = null; activeServerId = null; activeTab = null; }
  }

  /** `serverId` comes from a server's own "+ New" (the sheet is then locked to it); without it the sheet asks when there are several. */
  let newTarget = { serverId: '', locked: false };
  function newWorkspace(serverId?: string) {
    const asked = typeof serverId === 'string' ? serverId : undefined;
    const t = newWorkspaceTargets($hubServers, asked ?? selected?.id);
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
    activeTab = null;
  }

  /** Back to the workspaces destination (the servers page belongs to Settings). Coming back from away re-attaches (syncAway). */
  function showWorkspaceList() {
    serversPage = false;
    navigate('workspaces');
  }

  /** The servers page is part of Settings; leaving the screen detaches the open workspace (syncAway). */
  function leaveServer() {
    serversPage = true;
    navigate('settings');
    sheetHeight = 'closed';
  }

  function go(dest: Destination) {
    if (dest !== 'settings') serversPage = false;
    listPage = dest === 'workspaces' && !!activeKey; // Workspaces is one page, wherever it is pressed from; opening a workspace leaves it
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
        if (!away && activeKey === key && connState === 'connected') showWorkspace(key, activeTabId ?? undefined);
      });
    }
  }

  function openServer(id: string) {
    const first = $allWorkspaces.find(s => s.serverId === id);
    if (first) void attach(first.key); else leaveServer();
  }
  function runAction(id: string) {
    const [name, arg] = id.split(':');
    if (name === 'new-workspace') newWorkspace();
    else if (name === 'pause' && arg) hub.pause(arg);
    else if (name === 'resume' && arg) hub.resume(arg);
    else if (['approve-decision', 'deny-decision', 'next-needs-you', 'focus-composer'].includes(name)) setTimeout(() => handleShortcut(name), 0); // after the palette has closed and let go of focus
  }

  /** The reply field to type in: the visible, enabled composer, the open workspace's first. Others (Home lists several) are not reached from the keyboard here. */
  function visibleComposer(): HTMLInputElement | null {
    const shown = Array.from(document.querySelectorAll<HTMLInputElement>('input[data-composer]:not([disabled])'))
      .filter((el) => el.getClientRects().length > 0 && !el.closest('[inert], [aria-hidden=true], .pane-hidden'));
    return shown.find((el) => el.closest('[data-testid=session-view]')) ?? shown[0] ?? null;
  }
  /** Taps the plain Yes or No of the decision on screen. Nothing is guessed: with no such option the answer stays with the user. */
  function answerShownDecision(kind: 'yes' | 'no') {
    const options = Array.from(document.querySelectorAll<HTMLElement>('[data-hero-options]')).find((el) => el.getClientRects().length > 0 && !el.closest('[inert], .pane-hidden'));
    if (!options) { showToast('No decision is open here. Open a workspace that needs you.'); return; }
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
      const next = nextNeedsYou($allWorkspaces, (k) => fleetOf($allAgents[k]) === 'needs-you', (k) => $allAgents[k]?.waitingSince ?? $allAgents[k]?.ts ?? 0, activeKey);
      if (next) void openWaiting(next.key); else showToast('Nothing else needs you.');
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
    const stopAudioUnlock = installAudioUnlock(); // the needs-you sound plays only once a tap has unlocked audio
    const stopReplay = onAudioRunning(() => alertWaiting.replay({ sound: $settings.alertSound })); // a sound that found audio locked plays on the unlocking tap
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
    const stopUsage = startUsageMeter(() => !!selectedServer && connState === 'connected' && !!activeWorkspaceId);
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
      stopRouter(); stopAudioUnlock(); stopReplay();
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
      const focus = parseFocusLink(window.location.hash);
      if (focus) {
        const tab = parseFocusTab(window.location.hash);
        if (user) {
          if (tab) {
            if (hub.findWorkspace(focus)) void openWaiting(focus, tab);
            else holdPendingDeepTab(focus, tab);
          } else openDecision(focus);
        } else {
          sessionStorage.setItem('termenbox_pending_focus', focus);
          if (tab) sessionStorage.setItem('termenbox_pending_focus_tab', tab);
        }
        return;
      }
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

    // A push's deep link (#/home?focus=...&tab=...), kept across the GitHub login redirect like the links below.
    const focus = parseFocusLink(window.location.hash);
    if (focus) {
      sessionStorage.setItem('termenbox_pending_focus', focus);
      const tab = parseFocusTab(window.location.hash);
      if (tab) sessionStorage.setItem('termenbox_pending_focus_tab', tab);
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

    const pendingFocus = sessionStorage.getItem('termenbox_pending_focus');
    if (pendingFocus) {
      sessionStorage.removeItem('termenbox_pending_focus');
      const pendingTab = sessionStorage.getItem('termenbox_pending_focus_tab');
      sessionStorage.removeItem('termenbox_pending_focus_tab');
      if (pendingTab) holdPendingDeepTab(pendingFocus, pendingTab);
      else openDecision(pendingFocus);
    }

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
    <AppFrame current={$destination} onnavigate={go} servers={$hubServers} workspaces={$allWorkspaces} agents={$allAgents} tabStates={$allTabStates} {activeKey} chrome={servers.length > 0} inWorkspace={!!activeKey && !away} {keyboardOpen}
      canCreate={canCreate} bind:wide bind:helpOpen onopen={attach} onserver={openServer} onrename={renameWorkspace} oncloseworkspace={askCloseWorkspace} onaction={runAction} onshortcut={handleShortcut}
      onback={() => { showWorkspaceList(); sheetHeight = 'half'; }}>
    {#if $destination === 'workspaces' || servers.length === 0}
    <NavigationBar workspaces={tabWorkspaces} agents={$allAgents} servers={$hubServers} windows={$allWindows} {expiring} {link} {activeKey} held={!!actionWorkspace || !!menuWorkspace || cardExpanded || sheetHeld} onopen={attach} onpeek={peek} ontabmenu={openWindowMenu} onactions={(key) => { actionConfirm = false; actionKey = key; }}
      strip={!wide || !$layout.sidebarOpen || (!!activeKey && !listPage)} onnew={wide && canCreate ? () => newWorkspace() : undefined}
      tabsCapable={tabsCapable} workspace={activeWorkspace ?? null} tabs={activeTabs} activeTab={activeTabId} page={!activeKey || listPage} {wide}
      ontabopen={(tab) => activeKey && openTab(activeKey, tab)}
      ontabmenuopen={() => { if (!activeKey || listPage) openPageTabs(); else openTabMenu(); }}
      onnewtab={wide && activeKey && !listPage && tabsCapable && canTab ? () => activeWorkspace && newTab(activeWorkspace) : undefined}
      onlink={() => link && showToast(link.text, link.ok ? 'success' : 'error')} onmenu={() => { titleTapped(); menuOpen = true; }} expanded={wide ? $layout.sidebarOpen : sheetHeight !== 'closed'} controls={wide && $layout.sidebarOpen ? 'workspace-sidebar' : undefined} onworkspaces={() => { if (wide) { toggleSidebar(); return; } crumb('workspaces sheet toggled'); showWorkspaceList(); sheetHeight = sheetHeight === 'closed' ? 'half' : 'closed'; }} />
    {/if}
    <NeedsYouBanner {waiting} {now} collapsed={keyboardOpen} onopen={openWaiting} onhome={() => go('home')} ondismiss={(key, id) => sendAttention(key, { type: 'dismiss', attention_id: id })} onanswer={(key, choice, prompt) => sendWorkspace({ type: 'answer', workspace: key, prompt_id: prompt.id, seq: prompt.seq, choice })} />

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
        <ServerList {servers} states={$hubServers} counts={workspaceCounts} onpause={(id) => { hub.pause(id); }} onresume={(id) => hub.resume(id)} onretry={(id) => hub.retry(id)} onadd={() => { addPrefill = null; addStart = 'install'; showAddServer = true; }} onscan={() => { addPrefill = null; addStart = 'scan'; showAddServer = true; }} onremove={removeServer} onshare={shareServer} onrename={renameServer} onupdate={(id) => hub.updateServer(id)} />
      {:else if $destination === 'home'}
        <Home workspaces={$allWorkspaces} agents={$allAgents} servers={$hubServers} tabStates={$allTabStates} {now} {lastKey} {focusKey} offer={hookOffer} onhook={homeHook} onopen={openWaiting} onservers={leaveServer} onnew={newWorkspace}
          onanswer={(key, choice, prompt) => sendWorkspace({ type: 'answer', workspace: key, prompt_id: prompt.id, seq: prompt.seq, choice })}
          onreply={(key, text) => sendWorkspace({ type: 'reply', workspace: key, text })} />
      {:else if $destination === 'activity'}
        <ActivityView servers={$hubServers} history={historyMap} {now} onask={askHistory} caches={warmCaches} ttlMs={$settings.cacheTtlMs} health={healthMap} onhealth={askHealth} rttOf={(id) => hub.rtt(id)} />
      {:else if $destination === 'settings'}
        <SettingsView user={user ?? ''} hooks={hookSettings} onhook={settingsHook} onservers={leaveServer} push={pushView} onpush={togglePush} onshortcuts={() => (helpOpen = true)} onreport={openReport} ondiagnostics={() => openDiagnostics()} onlogout={logout} ondeletedata={deleteData} historyServers={$connectedCount} ondeletehistory={deleteHistory} pushSettings={pushServerId ? pushSettings[pushServerId] : undefined} {pushSaving} onpushsettings={savePushSettings} />
      {:else}
        <div class="workspace" class:mobile={narrow}>
          <div class="terminal-pane">
            {#if !activeKey || listPage}
              <WorkspaceGroups workspaces={$allWorkspaces} agents={$allAgents} {now} unhealthy={unhealthyServers} tabStates={$allTabStates} onopen={attach} onnew={newWorkspace}
                onactions={tabsAnywhere ? undefined : (key) => { actionConfirm = false; actionKey = key; }}
                onrename={(key) => { const w = hub.findWorkspace(key); if (w) askRenameWorkspace(w); }}
                onmove={tabsAnywhere ? (key, index) => hub.moveWorkspace(key, index) : undefined} />
            {:else if connection && connState !== 'paused' && activeIsAcp && activeWorkspace}
              {#key activeKey}
                <AcpView workspaceKey={activeWorkspace.key} name={activeWorkspace.name} agent={activeRow?.agent} fleet={activeRow?.state} stream={$acpStreams[activeWorkspace.key]} {connState} {now} {canShell}
                  onprompt={(text) => sendAcp({ type: 'acp_prompt', workspace: activeWorkspace.key, text })} oninterrupt={() => sendAcp({ type: 'acp_interrupt', workspace: activeWorkspace.key })}
                  onanswer={(promptId, optionId) => sendAcp({ type: 'acp_answer', workspace: activeWorkspace.key, prompt_id: promptId, option_id: optionId })}
                  onmode={(mode) => sendAcp({ type: 'acp_mode', workspace: activeWorkspace.key, mode })} onshell={() => openShell(activeWorkspace.key)} />
              {/key}
            {:else if connection && connState !== 'paused'}
              {#if !(keyboardOpen && paneMode === 'terminal')}
                <div class="pane-switch">
                  {#if !split}<Segmented label="Workspace view" value={paneMode} onchange={setPaneMode} options={[{ value: 'decision', label: 'Decision' }, { value: 'terminal', label: 'Terminal' }]} />{/if}
                  {#if wide && activeRow}
                    <button type="button" class="secondary split-toggle" aria-pressed={$layout.splitOpen} on:click={toggleSplit}>Terminal beside the decision</button>
                    {#if $layout.splitOpen && !split}<span class="split-note" role="status">Too narrow to show both. Hide the sidebar or widen the window.</span>{/if}
                  {/if}
                </div>
              {/if}
              <div class="pane-body" class:split data-pane-body data-split={split} bind:clientWidth={bodyWidth}>
                {#if (paneMode === 'decision' || split) && activeWorkspace && activeRow}
                  <div class="pane-decision" data-pane="decision">
                    <SessionView name={activeWorkspace.name} agent={activeRow.agent} fleet={activeRow.state} task={activeRow.title} startedAt={$allAgents[activeWorkspace.key]?.task?.started_at}
                      waitingSince={activeRow.since} step={stepOf($allAgents[activeWorkspace.key])} {now} view={heldView}
                      onreply={(line) => sendWorkspace({ type: 'reply', workspace: activeWorkspace.key, text: line })}
                      onanswer={(m) => hub.sendToWorkspace(activeWorkspace.key, m)} onterminal={() => setPaneMode('terminal')} />
                  </div>
                {/if}
                {#if split}
                  <Resizer label="Terminal width" controls="pane-terminal" value={splitW} min={SPLIT.min} max={splitMax} def={Math.min(SPLIT.def, splitMax)} step={SPLIT.step} grow={-1}
                    onchange={(w) => patchLayout({ splitWidth: w }, false)} oncommit={commitLayout} />
                {/if}
                <!-- One terminal for the workspace, always here: it is only restyled between full width, hidden and the right half, so crossing 720 px or toggling the split never rebuilds it. -->
                <div class="pane-terminal" id="pane-terminal" class:pane-hidden={paneMode === 'decision' && !split} inert={paneMode === 'decision' && !split} data-pane="terminal" style:width={split ? `${splitW}px` : undefined}>
                  {#key identityOf(connection)}<Terminal {connection} {activeWorkspaceId} {connState} ontyped={typedByHand} {profile} prefix={selected?.prefix || 'C-b'} {preview} />{/key}
                </div>
              </div>
            {:else}
              <div class="card" style="margin: auto;">{activeKey ? 'This server is paused. Resume it in Servers.' : 'Choose a workspace to open its terminal.'}<button class="secondary" on:click={leaveServer}>Servers</button></div>
            {/if}
          </div>
          {#if !wide && activeKey && !listPage && !tabsCapable}
            <WorkspaceSheet workspaces={$allWorkspaces} agents={$allAgents} servers={$hubServers} windows={$allWindows} onpeek={peek} onwarm={keepWarm} {activeKey} mobile={narrow} bind:height={sheetHeight} held={!!actionWorkspace} onactions={(key) => { actionConfirm = false; actionKey = key; }}
              onexpanded={(open) => cardExpanded = open} onheld={(held) => sheetHeld = held} onopen={attach} onnew={newWorkspace} onrefresh={() => { for (const s of $hubServers) hub.send(s.id, { type: 'list_workspaces' }); }} onretry={(id) => hub.retry(id)} send={sendWorkspace} />
          {/if}
        </div>
      {/if}
      </svelte:boundary>
    </main>
    </AppFrame>
  {/if}

  {#if menuOpen && user}<MenuSheet {user} {avatar} onservers={leaveServer} onsettings={() => go('settings')} onreport={openReport} ondiagnostics={() => openDiagnostics()} onlogout={logout} onclose={() => menuOpen = false} unread={unreadCount($inbox)} oninbox={() => inboxOpen = true} />{/if}
  {#if inboxOpen}<InboxSheet items={$inbox} {now} agents={$allAgents} onanswer={(key, choice, prompt) => sendWorkspace({ type: 'answer', workspace: key, prompt_id: prompt.id, seq: prompt.seq, choice })} onreply={(key, text) => sendWorkspace({ type: 'reply', workspace: key, text })} onopen={openWaiting} onhome={() => go('home')} onread={() => inbox.update(markAllRead)} onclear={() => inbox.set([])} onclose={() => { inboxOpen = false; inbox.update(markAllRead); }} />{/if}
  {#if actionWorkspace}<WorkspaceActions workspace={actionWorkspace} canClose={['tmux', 'acp'].includes(backendOf(actionWorkspace))} confirmFirst={actionConfirm} onrename={(name) => renameWorkspace(actionWorkspace.key, name)} placement={placementOf($allWorkspaces, actionWorkspace.key)} onmove={(to) => moveWorkspace($allWorkspaces, actionWorkspace.key, to)} ondetach={() => detach(actionWorkspace.key)} oncloseworkspace={() => closeWorkspace(actionWorkspace.key)} onclose={() => actionKey = null} />{/if}
  {#if tabMenuOpen}
    <TabMenu groups={tabMenuGroups} activeKey={activeKey} activeTab={activeTabId} states={$allTabStates} cacheTtlMs={$settings.cacheTtlMs} onopen={openTab}
      onrename={canTab ? askRenameTab : undefined}
      onclosetab={canTab ? (key, tab) => hub.closeTab(key, tab.id) : undefined}
      onmove={canTab ? moveTab : undefined}
      onnew={canTab ? (key) => { const w = hub.findWorkspace(key); if (w) newTab(w); } : undefined}
      oncloseworkspace={(key) => closeWorkspace(key)} onclose={() => (tabMenuOpen = false)} />
  {/if}
  {#if newTabFor}
    <NewTabSheet workspace={newTabFor} oncreate={createTab} onclose={() => (newTabFor = null)} />
  {/if}
  {#if renameTarget}
    {#key `${renameTarget.kind}:${renameTarget.key}:${renameTarget.tab ?? ''}`}
      <RenameDialog title={renameTarget.title} field={renameTarget.field} value={renameTarget.value} onsave={saveRename} onclose={() => (renameTarget = null)} />
    {/key}
  {/if}
  {#if menuWorkspace && menuKey}
    <WindowMenu workspace={menuWorkspace} data={$allWindows[menuWorkspace.key]} supported={backendOf(menuWorkspace) === 'tmux'}
      onselect={(index) => hub.sendToWorkspace(menuWorkspace.key, { type: 'select_window', workspace: menuWorkspace.key, index })}
      canNewWindow={serverAtLeast($hubServers.find(h => h.id === menuWorkspace.serverId)?.version ?? '', NEW_WINDOW_VERSION)}
      onnewwindow={() => newWindow(menuWorkspace.key)}
      onclosewindow={(index) => hub.sendToWorkspace(menuWorkspace.key, { type: 'close_window', workspace: menuWorkspace.key, index })}
      ondetach={() => detach(menuWorkspace.key)} oncloseworkspace={() => { actionConfirm = true; actionKey = menuWorkspace.key; }} onclose={() => menuKey = null} />
  {/if}
  {#if showNew}
    <NewWorkspaceSheet servers={newWorkspaceTargets($hubServers).servers} initialServerId={newTarget.serverId} locked={newTarget.locked} workspaces={$allWorkspaces} windows={$allWindows} oncreate={createWorkspace} {listDir} onclose={() => showNew = false} />
  {/if}

  {#if diagnostics}
    <ServerDiagnosticsDialog text={diagnostics.text} error={diagnostics.error} onclose={() => (diagnostics = null)} />
  {:else if exitNotice}
    <ExitBanner message={exitText(exitNotice, $hubServers.find(s => s.id === exitNotice?.serverId)?.server.server_name ?? '')} onopen={() => openDiagnostics(exitNotice?.serverId)} ondismiss={() => (exitNotice = null)} />
  {/if}
  {#if reportText !== null}
    <ReportDialog text={reportText} onclose={() => (reportText = null)} />
  {/if}

  {#if pairLink}
    <PairDialog link={pairLink} onclose={() => pairLink = null} onadd={(server) => { pairLink = null; void addPaired(server); }} />
  {/if}
  {#if showAddServer}
    <AddServerModal origin={location.origin} prefillSecret={addPrefill} start={addPrefill ? 'install' : addStart} onserver={(id) => { showAddServer = false; void addServerById(id); }} onclose={() => showAddServer = false} onadd={(server) => void addAndLink(server)} />
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
