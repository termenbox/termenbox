<script lang="ts">
  import AppShell from '../src/AppShell.svelte';
  import NavigationBar from '../src/NavigationBar.svelte';
  import MenuSheet from '../src/MenuSheet.svelte';
  import SettingsView from '../src/shell/SettingsView.svelte';
  import WorkspaceActions from '../src/WorkspaceActions.svelte';
  import WorkspaceSheet from '../src/WorkspaceSheet.svelte';
  import KeyRow from '../src/KeyRow.svelte';
  import WindowMenu from '../src/WindowMenu.svelte';
  import NeedsYouBanner from '../src/NeedsYouBanner.svelte';
  import { openTabs } from '../src/lib/tabs';
  import { moveWorkspace, placementOf } from '../src/lib/order';
  import { waitingList } from '../src/lib/notify';
  import type { WindowInfo } from '../src/lib/protocol';
  import type { SheetHeight } from '../src/lib/sheet';
  import { onMount } from 'svelte';
  import { ServerHub, type HubConnection } from '../src/lib/hub';
  import type { ConnectionOptions } from '../src/lib/connection';
  import type { ClientMessage, ServerMessage, WorkspaceInfo } from '../src/lib/protocol';

  const peers = new Map<string, Fake>();
  const messages: { server: string; message: ClientMessage }[] = [];
  class Fake implements HubConnection {
    listener?: (m: ServerMessage) => void;
    list: WorkspaceInfo[] = Array.from({ length: 4 }, (_, i) => ({ id: `workspace-${i}`, name: `workspace-${i}`, agent: null }));
    opts: ConnectionOptions;
    constructor(opts: ConnectionOptions) { this.opts = opts; }
    start() {
      this.opts.onState('connected'); this.opts.onReady({ type: 'auth_ok', user: 'test', server_name: this.opts.serverId! }, false);
      this.emit({ type: 'workspaces', workspaces: this.list });
      this.emit({ type: 'agent_snapshot', states: this.list.map((s, i) => ({ workspace: s.id, state: i === 1 ? 'needs_input' : i === 0 ? 'idle' : 'working', seq: 1, ts: Date.now(), stale: false, agent: 'claude', last_response_at: Date.now() - (i + 1) * 60000 })) });
      this.emit({ type: 'agent_events', events: [{ workspace: 'workspace-1', seq: 2, ts: Date.now(), source: 'hook', confidence: 'high', kind: 'prompt', id: `prompt-${this.opts.serverId}`, prompt_kind: 'choice', choices: ['Yes', 'No'] }] });
    }
    close() { this.opts.onState('closed'); }
    send(message: ClientMessage) { messages.push({ server: this.opts.serverId!, message }); }
    subscribe(fn: (m: ServerMessage) => void) { this.listener = fn; return () => { this.listener = undefined; }; }
    emit(m: ServerMessage) { this.listener?.(m); }
  }
  const hub = new ServerHub({ credentials: async () => ({ cert: '', sign: async () => '' }), factory: opts => { const peer = new Fake(opts); peers.set(opts.serverId!, peer); return peer; } });
  hub.start(['alpha', 'beta', 'gamma'].map(id => ({ server_id: id, server_name: id, server_url: '', registered_at: 0 })));
  const workspaces = hub.allWorkspaces, agents = hub.agents, servers = hub.servers, wins = hub.windows;
  let hidden = new Set<string>();
  let menuKey: string | null = null;
  let confirmClose = false;
  $: tabs = openTabs($workspaces, $agents, hidden);
  $: waiting = waitingList($workspaces, $agents);
  $: menuWorkspace = $workspaces.find(s => s.key === menuKey);
  let activeKey = 'gamma/workspace-3';
  let menu = false;
  let settings = false;
  let actions: string | null = null;
  let height: SheetHeight = 'closed';
  let mobile = true;
  let expanded = false;
  let sheetHeld = false;
  let mods = { ctrl: false, alt: false };
  onMount(() => {
    const mq = matchMedia('(max-width: 720px)'); const media = () => mobile = mq.matches; media(); mq.addEventListener('change', media);
    return () => mq.removeEventListener('change', media);
  });
  $: actionWorkspace = $workspaces.find(s => s.key === actions);
  Object.assign(window, { __navigation: {
    active: () => activeKey,
    sent: () => messages,
    changePrompt: () => peers.get('alpha')!.emit({ type: 'agent_events', events: [{ workspace: 'workspace-1', seq: 3, ts: Date.now(), source: 'hook', confidence: 'high', kind: 'prompt', id: 'changed-alpha', prompt_kind: 'choice', choices: ['Different'] }] }),
    select: (key: string) => activeKey = key,
    windows: (server: string, workspace: string, windows: WindowInfo[], representative: number | null) => peers.get(server)!.emit({ type: 'window_list', workspace, windows, representative }),
    hidden: () => [...hidden],
    change: () => {
      const peer = peers.get('alpha')!;
      peer.list = [...peer.list.filter(s => s.id !== 'workspace-0'), { id: 'new', name: 'new', agent: null }];
      peer.emit({ type: 'workspaces', workspaces: peer.list });
    },
    degrade: () => peers.get('beta')!.opts.onState('unreachable', 'Test server offline'),
  } });
</script>
<AppShell pinned>
  <NavigationBar workspaces={tabs} agents={$agents} servers={$servers} windows={$wins} ontabmenu={key => { menuKey = key; hub.sendToWorkspace(key, { type: 'windows', workspace: key }); }} {activeKey} held={!!actions || expanded || sheetHeld} onopen={key => { activeKey = key; height = 'closed'; }} onactions={key => actions = key} onmenu={() => menu = true} onworkspaces={() => { settings = false; height = height === 'closed' ? 'half' : 'closed'; }} />
  <NeedsYouBanner {waiting} now={Date.now()} onopen={key => { activeKey = key; }} onanswer={(key, choice, prompt) => hub.sendToWorkspace(key, { type: 'answer', workspace: key, prompt_id: prompt.id, seq: prompt.seq, choice })} />
  <main class:mobile>
    <div class="terminal"><p>{activeKey}</p><div class="screen"><textarea class="xterm-helper-textarea" aria-label="Terminal input"></textarea></div><KeyRow bind:mods onkey={() => {}} onpaste={() => {}} oncopy={() => {}} /></div>
    {#if settings}<div class="settings"><SettingsView user="test" onservers={() => {}} onshortcuts={() => {}} onreport={() => {}} onlogout={() => {}} ondeletedata={async () => 0} historyServers={0} ondeletehistory={async () => ({ servers: 0, removed: 0, failedNames: [] })} /></div>{/if}
    <WorkspaceSheet workspaces={$workspaces} agents={$agents} servers={$servers} {activeKey} {mobile} bind:height held={!!actions} onactions={key => actions = key} onexpanded={value => expanded = value} onheld={value => sheetHeld = value} onopen={key => { activeKey = key; height = 'closed'; }} onnew={() => {}} onrefresh={() => {}} onretry={id => hub.retry(id)} send={m => { if ('workspace' in m) hub.sendToWorkspace(m.workspace, m); }} />
  </main>
  {#if menu}<MenuSheet user="test" onservers={() => {}} onsettings={() => settings = true} onreport={() => {}} onlogout={() => {}} onclose={() => menu = false} />{/if}
  {#if actionWorkspace}<WorkspaceActions workspace={actionWorkspace} confirmFirst={confirmClose} onrename={name => hub.rename(actionWorkspace.key, name)} placement={placementOf($workspaces, actionWorkspace.key)} onmove={to => moveWorkspace($workspaces, actionWorkspace.key, to)} ondetach={() => { hidden = new Set(hidden).add(actionWorkspace.key); }} oncloseworkspace={() => hub.sendToWorkspace(actionWorkspace.key, { type: 'close_workspace', workspace: actionWorkspace.key })} onclose={() => actions = null} />{/if}
  {#if menuWorkspace && menuKey}
    <WindowMenu workspace={menuWorkspace} data={$wins[menuWorkspace.key]}
      onselect={index => hub.sendToWorkspace(menuWorkspace.key, { type: 'select_window', workspace: menuWorkspace.key, index })}
      onnewwindow={() => hub.sendToWorkspace(menuWorkspace.key, { type: 'new_window', workspace: menuWorkspace.key })}
      onclosewindow={index => hub.sendToWorkspace(menuWorkspace.key, { type: 'close_window', workspace: menuWorkspace.key, index })}
      ondetach={() => { hidden = new Set(hidden).add(menuWorkspace.key); }} oncloseworkspace={() => { confirmClose = true; actions = menuWorkspace.key; }} onclose={() => menuKey = null} />
  {/if}
</AppShell>
<style>main { position: relative; display: flex; flex: 1; min-height: 0; overflow: hidden; } main.mobile { flex-direction: column; } .terminal { display: flex; flex-direction: column; flex: 1; min-height: 0; min-width: 0; overflow: hidden; } .screen { flex: 1; min-height: 0; } .settings { position: absolute; inset: 0; z-index: 5; display: flex; flex-direction: column; background: var(--bg, #000); }</style>
