<script lang="ts">
  // The real app frame (bottom bar / sidebar / palette), NavigationBar, SessionSheet, SessionStrip and
  // SettingsView around a stand-in terminal, fed by the real ServerHub with two fake servers of ten sessions each.
  import AppShell from '../src/AppShell.svelte';
  import AppFrame from '../src/shell/AppFrame.svelte';
  import SettingsView from '../src/shell/SettingsView.svelte';
  import NavigationBar from '../src/NavigationBar.svelte';
  import SessionSheet from '../src/SessionSheet.svelte';
  import { EmptyState, SessionStrip, type StripItem } from '../src/lib/ds';
  import { destination, navigate, startRouter, type Destination } from '../src/lib/router';
  import { fleetOf } from '../src/lib/navlist';
  import { openTabs } from '../src/lib/tabs';
  import { onMount } from 'svelte';
  import { layout, toggleSidebar } from '../src/lib/panes';
  import { ServerHub, type HubConnection } from '../src/lib/hub';
  import type { ConnectionOptions } from '../src/lib/connection';
  import type { ClientMessage, ServerMessage, SessionInfo } from '../src/lib/protocol';
  import type { SheetHeight } from '../src/lib/sheet';

  const peers = new Map<string, Fake>();
  const messages: { server: string; message: ClientMessage }[] = [];
  const stateOf = (server: string, id: string) => (states[`${server}/${id}`] ?? 'idle') as 'idle' | 'working' | 'needs_input';
  const states: Record<string, string> = { 'alpha/svc-2': 'needs_input', 'alpha/svc-5': 'working', 'beta/job-1': 'working' };
  class Fake implements HubConnection {
    listener?: (m: ServerMessage) => void;
    list: SessionInfo[];
    opts: ConnectionOptions;
    constructor(opts: ConnectionOptions) {
      this.opts = opts;
      this.list = Array.from({ length: 10 }, (_, i) => { const name = opts.serverId === 'beta' ? (i === 7 ? 'needle' : `job-${i}`) : `svc-${i}`; return { id: name, name, agent: null }; });
    }
    snapshot() {
      return { type: 'agent_snapshot', states: this.list.map((s) => ({ session: s.id, state: stateOf(this.opts.serverId!, s.id), seq: 1, ts: Date.now(), stale: false, agent: 'claude', last_response_at: Date.now() - 60000 })) } as ServerMessage;
    }
    start() {
      this.opts.onState('connected'); this.opts.onReady({ type: 'auth_ok', user: 'test', server_name: this.opts.serverId! }, false);
      this.emit({ type: 'sessions', sessions: this.list });
      this.emit(this.snapshot());
    }
    close() { this.opts.onState('closed'); }
    send(message: ClientMessage) { messages.push({ server: this.opts.serverId!, message }); }
    subscribe(fn: (m: ServerMessage) => void) { this.listener = fn; return () => { this.listener = undefined; }; }
    emit(m: ServerMessage) { this.listener?.(m); }
  }
  const hub = new ServerHub({ credentials: async () => ({ cert: '', sign: async () => '' }), factory: (opts) => { const peer = new Fake(opts); peers.set(opts.serverId!, peer); return peer; } });
  hub.start(['alpha', 'beta'].map((id) => ({ server_id: id, server_name: id, server_url: '', registered_at: 0 })));
  const sessions = hub.allSessions, agents = hub.agents, servers = hub.servers, wins = hub.windows;

  let activeKey: string | null = $state(null);
  let wide = $state(false);
  let helpOpen = $state(false);
  let keyboardOpen = $state(false);
  let height: SheetHeight = $state('closed');
  let stripOpened: string[] = $state([]);
  let log: { key: string; ctrl: boolean; prevented: boolean }[] = [];
  let actions: string[] = [];
  const tabs = $derived(openTabs($sessions, $agents, new Set()));
  const strip = $derived<StripItem[]>($sessions.filter((s) => !activeKey || s.serverId === activeKey.split('/')[0]).map((s) => ({ id: s.key, name: s.name, agent: 'claude', state: fleetOf($agents[s.key]) })));

  // The probe stands in for the session workspace; the real app opens on Home (its own e2e is cards.mjs).
  destination.set('sessions');
  onMount(() => startRouter());
  const open = (key: string) => { activeKey = key; height = 'closed'; navigate('sessions'); };
  const keydown = (e: KeyboardEvent) => { log.push({ key: e.key, ctrl: e.ctrlKey, prevented: false }); };
  onMount(() => {
    const after = (e: KeyboardEvent) => { const last = log[log.length - 1]; if (last && last.key === e.key) last.prevented = e.defaultPrevented; };
    window.addEventListener('keydown', after);
    return () => window.removeEventListener('keydown', after);
  });
  Object.assign(window, { __frame: {
    active: () => activeKey,
    destination: () => { let d = ''; destination.subscribe((v) => (d = v))(); return d; },
    keys: () => log,
    actions: () => actions,
    sent: () => messages,
    paused: () => messages.filter((m) => m.message.type !== 'list_sessions'),
    keyboard: (v: boolean) => { keyboardOpen = v; },
    setState: (server: string, id: string, state: string) => { states[`${server}/${id}`] = state; const p = peers.get(server)!; p.emit(p.snapshot()); },
    remove: (server: string, id: string) => { const p = peers.get(server)!; p.list = p.list.filter((s) => s.id !== id); p.emit({ type: 'sessions', sessions: p.list }); },
    opened: () => stripOpened,
    sheet: (h: SheetHeight) => { height = h; },
    clearKeys: () => { log.length = 0; },
  } });
  const inSession = $derived(!!activeKey && $destination === 'sessions');
</script>

<AppShell pinned>
  <AppFrame current={$destination} onnavigate={(d: Destination) => navigate(d)} servers={$servers} sessions={$sessions} agents={$agents} {activeKey} {inSession} {keyboardOpen}
    bind:wide bind:helpOpen onopen={open} onserver={(id) => { const s = $sessions.find((x) => x.serverId === id); if (s) open(s.key); }}
    onaction={(id) => actions.push(id)} onback={() => (height = 'half')}>
    {#if $destination === 'sessions'}
      <NavigationBar sessions={tabs} agents={$agents} servers={$servers} windows={$wins} {activeKey} onopen={open} onactions={() => {}} onmenu={() => {}} expanded={wide ? $layout.sidebarOpen : height !== 'closed'} onsessions={() => { if (wide) toggleSidebar(); else height = height === 'closed' ? 'half' : 'closed'; }} />
      <main class="flex min-h-0 flex-1 flex-col">
        <p data-testid="active">{activeKey ?? 'none'}</p>
        <SessionStrip items={strip} selected={activeKey ?? undefined} onselect={(id) => { stripOpened.push(id); open(id); }} />
        <div class="min-h-0 flex-1"><textarea class="xterm-helper-textarea" aria-label="Terminal input" onkeydown={keydown}></textarea></div>
        {#if !wide}
          <SessionSheet sessions={$sessions} agents={$agents} servers={$servers} {activeKey} mobile bind:height onopen={open} onnew={() => {}} onrefresh={() => {}} onretry={() => {}} send={() => {}} />
        {/if}
      </main>
    {:else if $destination === 'home'}
      <EmptyState title="Home" body="What needs you will gather here." />
    {:else if $destination === 'activity'}
      <EmptyState title="Activity" body="What your agents did will appear here." />
    {:else}
      <SettingsView user="test" onservers={() => {}} onmenu={() => {}} onshortcuts={() => (helpOpen = true)} onreport={() => {}} onlogout={() => {}} ondeletedata={async () => 0} />
    {/if}
  </AppFrame>
</AppShell>
