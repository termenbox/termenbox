<script lang="ts">
  import { onMount } from 'svelte';
  import { NativeSelect } from './lib/ui/native-select';
  import WorkspaceCard from './WorkspaceCard.svelte';
  import ServerStatus from './ServerStatus.svelte';
  import QuickReply from './QuickReply.svelte';
  import { FrozenOrder } from './lib/cards';
  import { adaptiveDefault, applyView, loadView, PRESETS, presetView, saveView, withServerStatus, type SessionView, type ViewGroup } from './lib/views';
  import { HEIGHTS, sheetHeight, snapHeight, type SheetHeight } from './lib/sheet';
  import { settings } from './lib/settings';
  import { representativeWindow, type WindowMap } from './lib/windows';
  import type { AgentMap, WorkspaceAgent } from './lib/agents';
  import type { HubServer, HubWorkspace } from './lib/hub';
  import type { AgentState, ClientMessage } from './lib/protocol';

  export let workspaces: HubWorkspace[];
  export let agents: AgentMap;
  export let servers: HubServer[];
  export let activeKey: string | null;
  export let mobile = true;
  export let height: SheetHeight = 'half';
  export let held = false;
  export let windows: WindowMap = {};
  export let onpeek: (key: string) => void = () => {};
  export let onwarm: (key: string) => void = () => {};
  export let onopen: (key: string) => void;
  export let onnew: (serverId?: string) => void;
  export let onrefresh: () => void;
  export let onretry: (id: string) => void;
  export let send: (message: ClientMessage) => void;
  export let onexpanded: (open: boolean) => void = () => {};
  export let onheld: (held: boolean) => void = () => {};

  let root: HTMLElement;
  let content: HTMLDivElement;
  let available = 0;
  let now = Date.now();
  let revision = 0;
  let forceRender = false; // the user's own view changes apply at once; only unsolicited reorders wait
  let view = loadView();
  let expanded: string | null = null;
  let shown: ViewGroup[] = [];
  let shownAgents: AgentMap = {};
  let shownServers: HubServer[] = [];
  let shownWorkspaces: HubWorkspace[] = [];
  let shownGroupBy: SessionView['group'] = 'server';
  let shownNow = now;
  let shownWindows: WindowMap = {};
  let rendered = false;
  let controls = false;
  let drag: { pointer: number; y: number; start: number; moved: boolean } | null = null;
  let dragPixels: number | null = null;
  let suppressCycle = false;
  let externalHold = false;
  let touches = 0;
  let focusPending = false;
  const frozen = new FrozenOrder();
  $: if (held !== externalHold) { frozen.sheet(held); externalHold = held; revision++; }
  $: if ((height === 'peek' || height === 'closed') && expanded) closeExpanded();
  $: paths = Object.fromEntries(workspaces.map(s => [s.key, representativeWindow(windows[s.key])?.path ?? '']));
  $: effectiveView = view ?? adaptiveDefault(Object.fromEntries(workspaces.filter(s => agents[s.key]).map(s => [s.key, agents[s.key]])));
  $: agentNames = [...new Set(workspaces.map(s => agents[s.key]?.agent ?? s.agent ?? 'shell'))].sort();
  $: pixels = dragPixels ?? sheetHeight(height, available);
  $: {
    revision;
    const proposed = applyView(workspaces, agents, effectiveView, now, { ttlMs: $settings.cacheTtlMs, paths });
    // An empty/reconnecting server still has its state and Retry in the unfiltered By server view.
    if (effectiveView.group === 'server' && !effectiveView.filter.statuses.length && !effectiveView.filter.agents.length && !effectiveView.filter.query) {
      for (const s of servers) if (s.state !== 'paused' && (!effectiveView.filter.servers.length || effectiveView.filter.servers.includes(s.id)) && !proposed.some(g => g.serverId === s.id)) {
        proposed.push({ key: `server:${s.id}`, label: s.server.server_name || 'Server', serverId: s.id, workspaces: [] });
      }
    }
    frozen.order(proposed.flatMap(g => g.workspaces.map(s => s.key)), {}, now);
    onheld(frozen.frozen(now));
    if (forceRender || !frozen.frozen(now) || !rendered) {
      forceRender = false;
      shown = withServerStatus(proposed, servers);
      shownAgents = { ...agents }; shownServers = servers; shownNow = now; rendered = true;
      shownWorkspaces = workspaces; shownGroupBy = effectiveView.group; shownWindows = windows;
    }
  }
  function closeExpanded() { if (expanded) { frozen.sheet(false); expanded = null; onexpanded(false); revision++; } }
  function toggle(key: string) {
    const next = expanded === key ? null : key;
    closeExpanded(); expanded = next;
    if (expanded) { frozen.sheet(true); onexpanded(true); }
    revision++;
  }
  function choose(next: SessionView) { closeExpanded(); view = next; saveView(next); forceRender = true; revision++; }
  function change(patch: Partial<SessionView>) { choose({ ...effectiveView, ...patch, preset: undefined }); }
  function filter(patch: Partial<SessionView['filter']>) { change({ filter: { ...effectiveView.filter, ...patch } }); }
  function focusField(e: FocusEvent) {
    if (!mobile || !(e.target as HTMLElement)?.matches('input,select,textarea')) return;
    if (touches) focusPending = true;
    else height = 'full';
  }
  function startDrag(e: PointerEvent) {
    if (e.button !== 0) return;
    drag = { pointer: e.pointerId, y: e.clientY, start: pixels, moved: false };
    suppressCycle = false;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function moveDrag(e: PointerEvent) {
    if (!drag || drag.pointer !== e.pointerId) return;
    const delta = drag.y - e.clientY;
    if (Math.abs(delta) > 6) drag.moved = true;
    if (drag.moved) dragPixels = Math.max(sheetHeight('peek', available), Math.min(available, drag.start + delta));
  }
  function endDrag(e: PointerEvent, cancelled = false) {
    if (!drag || drag.pointer !== e.pointerId) return;
    suppressCycle = drag.moved || cancelled;
    if (!cancelled && drag.moved) height = snapHeight(dragPixels ?? drag.start, available);
    drag = null; dragPixels = null;
  }
  function cycle() { if (!suppressCycle) height = HEIGHTS[(HEIGHTS.indexOf(height) + 1) % HEIGHTS.length]; suppressCycle = false; }
  function answer(key: string, choice: number, prompt: NonNullable<WorkspaceAgent['prompt']>) {
    send({ type: 'answer', workspace: key, prompt_id: prompt.id, seq: prompt.seq, choice });
  }
  onMount(() => {
    const parent = root.parentElement!;
    const measure = () => { available = parent.clientHeight; };
    const observer = new ResizeObserver(measure); observer.observe(parent); measure();
    const timer = setInterval(() => now = Date.now(), 250);
    const release = (e: PointerEvent) => {
      if (!frozen.hasPointer(e.pointerId)) return;
      setTimeout(() => {
        frozen.touchUp(e.pointerId); touches = frozen.pointerCount;
        if (!touches && focusPending) { if (mobile) height = 'full'; focusPending = false; }
        revision++;
      }, 0);
    };
    window.addEventListener('pointerup', release); window.addEventListener('pointercancel', release);
    return () => { observer.disconnect(); clearInterval(timer); window.removeEventListener('pointerup', release); window.removeEventListener('pointercancel', release); onexpanded(false); onheld(false); };
  });
</script>

<section bind:this={root} class="workspace-sheet" class:mobile class:docked={!mobile} class:peek={mobile && height === 'peek'} class:closed={mobile && height === 'closed'} aria-label="Workspace views" data-height={mobile ? height : 'docked'}
  style={mobile ? `height: ${pixels}px; flex-basis: ${pixels}px;` : ''}
  on:pointerdown|capture={(e) => { frozen.touchDown(e.pointerId); touches = frozen.pointerCount; revision++; }} on:focusin={focusField}>
  {#if mobile}
    <button class="handle" aria-label={`Resize workspaces sheet, ${height}`} on:pointerdown={startDrag} on:pointermove={moveDrag} on:pointerup={(e) => endDrag(e)} on:pointercancel={(e) => endDrag(e, true)} on:click={cycle}
      on:keydown={(e) => { if (e.key === 'ArrowUp' || e.key === 'ArrowDown') { e.preventDefault(); const i = HEIGHTS.indexOf(height); height = HEIGHTS[Math.max(0, Math.min(2, i + (e.key === 'ArrowUp' ? 1 : -1)))]; } }}><span aria-hidden="true"></span></button>
  {/if}
  <div class="sheet-header"><h2>Workspaces</h2><button class="secondary icon" on:click={onrefresh}>Refresh</button><button class="secondary icon" on:click={() => onnew()}>+ New</button>{#if mobile}<button class="secondary" aria-label="Minimize workspaces" on:click={() => height = 'closed'}>⌄</button>{/if}</div>
  <div class="content" bind:this={content} on:scroll={() => { frozen.scrolled(Date.now()); revision++; }}>
    <div class="presets" on:scroll={() => { frozen.scrolled(Date.now()); revision++; }}>
      {#each PRESETS as preset}<button class="secondary" aria-pressed={effectiveView.preset === preset.id} on:click={() => choose(presetView(preset.id))}>{preset.label}</button>{/each}
    </div>
    <div class="body">
      <button class="secondary view-button" aria-expanded={controls} on:click={() => controls = !controls}>Group, sort and filter</button>
      {#if controls}
        <div class="controls">
          <label>Group <NativeSelect aria-label="Group" value={effectiveView.group} onchange={(e) => change({ group: e.currentTarget.value as SessionView['group'] })}><option value="server">Server</option><option value="agent">Agent</option><option value="status">Status</option><option value="project">Project</option><option value="none">None</option></NativeSelect></label>
          <label>Sort <NativeSelect aria-label="Sort" value={effectiveView.sort} onchange={(e) => change({ sort: e.currentTarget.value as SessionView['sort'] })}><option value="human">Needs you first</option><option value="response">Last response</option><option value="cache">Cache expiry</option><option value="name">Name</option></NativeSelect></label>
          <label>Status <NativeSelect aria-label="Status" value={effectiveView.filter.statuses.length > 1 ? 'multiple' : effectiveView.filter.statuses[0] ?? ''} onchange={(e) => filter({ statuses: e.currentTarget.value ? [e.currentTarget.value as AgentState] : [] })}><option value="">All statuses</option>{#if effectiveView.filter.statuses.length > 1}<option value="multiple" disabled>Multiple statuses</option>{/if}<option value="needs_input">Needs you</option><option value="error">Error</option><option value="working">Working</option><option value="idle">Idle</option><option value="done">Done</option><option value="unknown">No status</option></NativeSelect></label>
          <label>Agent <NativeSelect aria-label="Agent" value={effectiveView.filter.agents[0] ?? ''} onchange={(e) => filter({ agents: e.currentTarget.value ? [e.currentTarget.value] : [] })}><option value="">All agents</option>{#each agentNames as name}<option value={name}>{name}</option>{/each}</NativeSelect></label>
          <label>Server <NativeSelect aria-label="Server" value={effectiveView.filter.servers[0] ?? ''} onchange={(e) => filter({ servers: e.currentTarget.value ? [e.currentTarget.value] : [] })}><option value="">All servers</option>{#each servers as s}<option value={s.id}>{s.server.server_name || 'Server'}</option>{/each}</NativeSelect></label>
          <label>Find <input aria-label="Find" type="search" value={effectiveView.filter.query} on:input={(e) => filter({ query: e.currentTarget.value })} placeholder="Workspace, server or agent" /></label>
          <label><input type="checkbox" aria-label="Cache expiring only" checked={!!effectiveView.filter.cacheExpiring} on:change={(e) => filter({ cacheExpiring: e.currentTarget.checked })} />Cache expiring only</label>
        </div>
      {/if}
      {#if shownGroupBy !== 'server'}
        {#each shownServers.filter(s => s.state !== 'connected' && s.state !== 'paused') as server (server.id)}<div class="group-header"><h3>{server.server.server_name || 'Server'}</h3><ServerStatus {server} {onretry} /></div>{/each}
      {/if}
      {#each shown as group (group.key)}
        <section class="workspace-group" data-group={group.key}>
          {#if group.label || group.server}<div class="group-header"><h3>{group.label}</h3>{#if group.server}<ServerStatus server={group.server} {onretry} />{#if group.server.state === 'connected'}<button class="secondary icon" aria-label={`New workspace on ${group.server.server.server_name || 'Server'}`} on:click={() => onnew(group.server?.id)}>+ New</button>{/if}{/if}</div>{/if}
          {#each group.workspaces as workspace (workspace.key)}
            {@const agent = shownAgents[workspace.key]}
            {#if agent}
              <WorkspaceCard workspace={workspace} {agent} windows={shownWindows[workspace.key]} ttlMs={$settings.cacheTtlMs} keepWarm={$settings.keepWarm} onpeek={() => onpeek(workspace.key)} onwarm={() => onwarm(workspace.key)} serverName={shownGroupBy !== 'server' && shownServers.length > 1 ? workspace.serverName : ''} now={shownNow} active={workspace.key === activeKey} expanded={workspace.key === expanded} onopen={() => onopen(workspace.key)} ontoggle={() => toggle(workspace.key)} onanswer={(choice, prompt) => answer(workspace.key, choice, prompt)} />
            {:else}<button class="plain" class:active={workspace.key === activeKey} on:click={() => onopen(workspace.key)}>{workspace.name} <small>{workspace.serverName}</small></button>{/if}
          {/each}
          {#if !group.workspaces.length}<p class="empty">No workspaces yet</p>{/if}
        </section>
      {/each}
      {#if !shown.length}<p class="empty">No workspaces in this view</p>{/if}
      <QuickReply workspaces={shownWorkspaces} {activeKey} {send} />
    </div>
  </div>
</section>

<style>
  .workspace-sheet { display: flex; flex-direction: column; min-height: 0; flex-shrink: 0; background: var(--bg-secondary); color: var(--fg); overflow: hidden; }
  .mobile { border: 1px solid var(--border); border-radius: 16px 16px 0 0; width: 100%; }
  .docked { width: 320px; height: 100%; border-right: 1px solid var(--border); order: -1; }
  .handle { display: flex; align-items: center; justify-content: center; min-height: 44px; flex: 0 0 44px; width: 100%; background: transparent; color: var(--fg); touch-action: none; padding: 0; }
  .handle span { width: 36px; height: 4px; border-radius: 2px; background: var(--fg-muted); }
  .sheet-header { display: flex; align-items: center; gap: 6px; padding: 0 8px 8px; flex-shrink: 0; }
  h2 { flex: 1; font-size: 0.95rem; }
  h3 { font-size: 0.85rem; overflow-wrap: anywhere; }
  button, input { min-height: 44px; min-width: 44px; }
  .sheet-header button { padding: 6px 8px; }
  .content { overflow-y: auto; min-height: 0; flex: 1; overscroll-behavior-y: contain; }
  .peek .content { display: none; }
  .closed { display: none; }
  .presets { position: sticky; top: 0; z-index: 2; display: flex; gap: 6px; overflow-x: auto; padding: 6px 8px; background: var(--bg-secondary); scrollbar-width: none; }
  .presets button { flex-shrink: 0; }
  .presets button[aria-pressed=true] { border-color: var(--accent); color: var(--accent); }
  .body { padding: 0 8px 8px; }
  .view-button { width: 100%; margin-bottom: 8px; }
  .controls { display: grid; gap: 6px; margin-bottom: 12px; }
  label { display: flex; align-items: center; justify-content: space-between; gap: 8px; font-size: 0.8rem; }
  input { max-width: 70%; min-width: 0; }
  .group-header { padding: 12px 4px 6px; }
  .plain { display: flex; align-items: center; justify-content: space-between; width: 100%; margin-bottom: 6px; background: var(--bg); color: var(--fg); text-align: left; border: 1px solid var(--border); overflow-wrap: anywhere; }
  .plain.active { border-color: var(--accent); }
  small { color: var(--fg-muted); }
  .empty { padding: 16px 4px; color: var(--fg-muted); font-size: 0.85rem; }
</style>
